use crate::imap::{ConnectionSecurity, ProxyConfig};
use lettre::message::header::ContentType;
use lettre::message::{Attachment, Body, Mailbox, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::transport::smtp::client::{AsyncSmtpConnection, AsyncTokioStream, Tls, TlsParameters};
use lettre::transport::smtp::extension::ClientId;
use lettre::{AsyncSmtpTransport, AsyncTransport};
use pebble_core::{PebbleError, Result};
use std::fmt;
use std::net::SocketAddr;
use std::path::Path;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_native_tls as async_native_tls;

pub struct SmtpSender {
    host: String,
    port: u16,
    credentials: Credentials,
    security: ConnectionSecurity,
    accept_invalid_certs: bool,
    proxy: Option<ProxyConfig>,
}

impl SmtpSender {
    pub fn new(
        host: String,
        port: u16,
        username: String,
        password: String,
        security: ConnectionSecurity,
        accept_invalid_certs: bool,
        proxy: Option<ProxyConfig>,
    ) -> Self {
        Self {
            host,
            port,
            credentials: Credentials::new(username, password),
            security,
            accept_invalid_certs,
            proxy,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn send(
        &self,
        from: &pebble_core::EmailAddress,
        to: &[String],
        cc: &[String],
        bcc: &[String],
        subject: &str,
        body_text: &str,
        body_html: Option<&str>,
        in_reply_to: Option<&str>,
        attachment_paths: &[String],
    ) -> Result<()> {
        let email = build_message(
            from,
            to,
            cc,
            bcc,
            subject,
            body_text,
            body_html,
            in_reply_to,
            attachment_paths,
        )?;

        if let Some(ref proxy) = self.proxy {
            self.send_via_proxy(&email, proxy).await
        } else {
            self.send_direct(&email).await
        }
    }

    /// Send without a proxy using the standard lettre AsyncSmtpTransport.
    async fn send_direct(&self, email: &lettre::Message) -> Result<()> {
        let tls_parameters = || smtp_tls_parameters(&self.host, self.accept_invalid_certs);
        let transport = match self.security {
            ConnectionSecurity::Tls => {
                AsyncSmtpTransport::<lettre::Tokio1Executor>::builder_dangerous(&self.host)
                    .tls(Tls::Wrapper(tls_parameters()?))
                    .port(self.port)
                    .credentials(self.credentials.clone())
                    .build()
            }
            ConnectionSecurity::StartTls => {
                AsyncSmtpTransport::<lettre::Tokio1Executor>::builder_dangerous(&self.host)
                    .tls(Tls::Required(tls_parameters()?))
                    .port(self.port)
                    .credentials(self.credentials.clone())
                    .build()
            }
            ConnectionSecurity::Plain => {
                AsyncSmtpTransport::<lettre::Tokio1Executor>::builder_dangerous(&self.host)
                    .tls(Tls::None)
                    .port(self.port)
                    .credentials(self.credentials.clone())
                    .build()
            }
        };

        transport
            .send(email.clone())
            .await
            .map_err(|e| PebbleError::Network(format!("SMTP send failed: {e}")))?;

        Ok(())
    }

    /// Send via a SOCKS5 proxy using a manually managed AsyncSmtpConnection.
    async fn send_via_proxy(&self, email: &lettre::Message, proxy: &ProxyConfig) -> Result<()> {
        let proxy_addr = format!("{}:{}", proxy.host, proxy.port);
        let target = format!("{}:{}", self.host, self.port);

        // For TLS (implicit), connect through SOCKS5, then wrap in TLS before the SMTP handshake.
        // For STARTTLS / Plain, connect through SOCKS5 plain, then upgrade as needed.
        let hello_name = ClientId::default();

        match self.security {
            ConnectionSecurity::Tls => {
                // Connect through SOCKS5 to the SMTP host:port
                let socks_stream =
                    tokio_socks::tcp::Socks5Stream::connect(proxy_addr.as_str(), target.as_str())
                        .await
                        .map_err(|e| PebbleError::Network(format!("SOCKS5 connect failed: {e}")))?;

                // Try rustls first, fall back to native-tls if handshake fails
                let rustls_config = build_rustls_client_config(self.accept_invalid_certs)?;
                let connector = tokio_rustls::TlsConnector::from(rustls_config);
                let domain = rustls::pki_types::ServerName::try_from(self.host.clone())
                    .map_err(|e| PebbleError::Network(format!("Invalid TLS server name: {e}")))?
                    .to_owned();

                match connector.connect(domain, socks_stream.into_inner()).await {
                    Ok(tls_stream) => {
                        let mut conn = AsyncSmtpConnection::connect_with_transport(
                            Box::new(DebugStream(tls_stream)),
                            &hello_name,
                        )
                        .await
                        .map_err(|e| PebbleError::Network(format!("SMTP handshake failed: {e}")))?;
                        authenticate_and_send(&mut conn, &self.credentials, email).await?;
                    }
                    Err(rustls_err) => {
                        tracing::debug!(
                            "SMTP rustls handshake failed ({}), retrying with native-tls",
                            rustls_err
                        );
                        // Reconnect SOCKS5 and try native-tls
                        let socks_stream = tokio_socks::tcp::Socks5Stream::connect(
                            proxy_addr.as_str(),
                            target.as_str(),
                        )
                        .await
                        .map_err(|e| {
                            PebbleError::Network(format!("SOCKS5 reconnect failed: {e}"))
                        })?;
                        let native_connector =
                            build_native_tls_connector(self.accept_invalid_certs)?;
                        let tls_stream = native_connector
                            .connect(&self.host, socks_stream.into_inner())
                            .await
                            .map_err(|e| {
                                PebbleError::Network(format!(
                                    "TLS failed with both backends — rustls: {rustls_err}, native-tls: {e}"
                                ))
                            })?;
                        let mut conn = AsyncSmtpConnection::connect_with_transport(
                            Box::new(DebugStream(tls_stream)),
                            &hello_name,
                        )
                        .await
                        .map_err(|e| PebbleError::Network(format!("SMTP handshake failed: {e}")))?;
                        authenticate_and_send(&mut conn, &self.credentials, email).await?;
                    }
                }
            }
            ConnectionSecurity::StartTls => {
                let socks_stream =
                    tokio_socks::tcp::Socks5Stream::connect(proxy_addr.as_str(), target.as_str())
                        .await
                        .map_err(|e| PebbleError::Network(format!("SOCKS5 connect failed: {e}")))?;

                let mut conn = AsyncSmtpConnection::connect_with_transport(
                    Box::new(Socks5TokioStream(socks_stream)),
                    &hello_name,
                )
                .await
                .map_err(|e| PebbleError::Network(format!("SMTP handshake failed: {e}")))?;

                if !conn.can_starttls() {
                    return Err(PebbleError::Network(
                        "STARTTLS required but server does not support it — refusing to send in plaintext".to_string(),
                    ));
                }
                let tls_params = smtp_tls_parameters(&self.host, self.accept_invalid_certs)?;
                conn.starttls(tls_params, &hello_name)
                    .await
                    .map_err(|e| PebbleError::Network(format!("STARTTLS failed: {e}")))?;

                authenticate_and_send(&mut conn, &self.credentials, email).await?;
            }
            ConnectionSecurity::Plain => {
                let socks_stream =
                    tokio_socks::tcp::Socks5Stream::connect(proxy_addr.as_str(), target.as_str())
                        .await
                        .map_err(|e| PebbleError::Network(format!("SOCKS5 connect failed: {e}")))?;

                let mut conn = AsyncSmtpConnection::connect_with_transport(
                    Box::new(Socks5TokioStream(socks_stream)),
                    &hello_name,
                )
                .await
                .map_err(|e| PebbleError::Network(format!("SMTP handshake failed: {e}")))?;

                authenticate_and_send(&mut conn, &self.credentials, email).await?;
            }
        }

        Ok(())
    }
}

fn smtp_tls_parameters(host: &str, accept_invalid_certs: bool) -> Result<TlsParameters> {
    TlsParameters::builder(host.to_string())
        .dangerous_accept_invalid_certs(accept_invalid_certs)
        .dangerous_accept_invalid_hostnames(accept_invalid_certs)
        .build()
        .map_err(|e| PebbleError::Network(format!("TLS parameters error: {e}")))
}

/// Authenticate on an open `AsyncSmtpConnection` and send one message.
async fn authenticate_and_send(
    conn: &mut AsyncSmtpConnection,
    credentials: &Credentials,
    email: &lettre::Message,
) -> Result<()> {
    let mechanisms = &[Mechanism::Plain, Mechanism::Login];
    conn.auth(mechanisms, credentials)
        .await
        .map_err(|e| PebbleError::Network(format!("SMTP auth failed: {e}")))?;

    let envelope = email.envelope().clone();
    let raw = email.formatted();
    conn.send(&envelope, &raw)
        .await
        .map_err(|e| PebbleError::Network(format!("SMTP send failed: {e}")))?;

    let _ = conn.quit().await;
    Ok(())
}

/// Build a rustls ClientConfig that trusts the system/webpki roots.
fn build_rustls_client_config(
    accept_invalid_certs: bool,
) -> Result<std::sync::Arc<rustls::ClientConfig>> {
    let provider = std::sync::Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let mut root_store = rustls::RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let builder = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| PebbleError::Network(format!("TLS protocol versions: {e}")))?;
    let config = if accept_invalid_certs {
        builder
            .dangerous()
            .with_custom_certificate_verifier(std::sync::Arc::new(
                crate::imap::AcceptInvalidCertsVerifier::new(root_store),
            ))
            .with_no_client_auth()
    } else {
        builder
            .with_root_certificates(root_store)
            .with_no_client_auth()
    };
    Ok(std::sync::Arc::new(config))
}

/// Build a native-tls connector (OS TLS backend: SChannel/SecureTransport/OpenSSL).
fn build_native_tls_connector(
    accept_invalid_certs: bool,
) -> Result<async_native_tls::TlsConnector> {
    let mut builder = native_tls::TlsConnector::builder();
    if accept_invalid_certs {
        builder.danger_accept_invalid_certs(true);
        builder.danger_accept_invalid_hostnames(true);
        builder.min_protocol_version(Some(native_tls::Protocol::Tlsv10));
    }
    let connector = builder
        .build()
        .map_err(|e| PebbleError::Network(format!("native-tls init: {e}")))?;
    Ok(async_native_tls::TlsConnector::from(connector))
}

// ---------------------------------------------------------------------------
// Stream wrappers that implement AsyncTokioStream for lettre
// ---------------------------------------------------------------------------

/// Wraps `tokio_socks::tcp::Socks5Stream<tokio::net::TcpStream>` so it can be
/// passed to `AsyncSmtpConnection::connect_with_transport`.
struct Socks5TokioStream(tokio_socks::tcp::Socks5Stream<tokio::net::TcpStream>);

impl fmt::Debug for Socks5TokioStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Socks5TokioStream").finish_non_exhaustive()
    }
}

impl AsyncTokioStream for Socks5TokioStream {
    fn peer_addr(&self) -> std::io::Result<SocketAddr> {
        // Deref to the inner TcpStream via the Deref impl on Socks5Stream.
        use std::ops::Deref;
        self.0.deref().peer_addr()
    }
}

impl AsyncRead for Socks5TokioStream {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl AsyncWrite for Socks5TokioStream {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.0).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.0).poll_shutdown(cx)
    }
}

// Socks5TokioStream wraps a TcpStream (which is Unpin) and the socks stream is also Unpin.
impl Unpin for Socks5TokioStream {}

// ---------------------------------------------------------------------------
// DebugStream: generic wrapper that adds Debug + peer_addr for any stream
// that implements AsyncRead + AsyncWrite + Unpin + Send + Sync but lacks
// a native peer_addr (e.g. TLS-wrapped streams).
// ---------------------------------------------------------------------------

struct DebugStream<S>(S);

impl<S: fmt::Debug> fmt::Debug for DebugStream<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<S> AsyncTokioStream for DebugStream<S>
where
    S: AsyncRead + AsyncWrite + Send + Sync + Unpin + fmt::Debug + 'static,
{
    fn peer_addr(&self) -> std::io::Result<SocketAddr> {
        // For TLS-over-proxy streams we don't have a meaningful peer_addr; return a dummy.
        Ok("0.0.0.0:0".parse().unwrap())
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for DebugStream<S> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for DebugStream<S> {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.0).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.0).poll_shutdown(cx)
    }
}

impl<S: Unpin> Unpin for DebugStream<S> {}

/// Map common file extensions to MIME content types.
fn mime_type_from_extension(ext: &str) -> ContentType {
    match ext.to_ascii_lowercase().as_str() {
        "pdf" => ContentType::parse("application/pdf").unwrap(),
        "zip" => ContentType::parse("application/zip").unwrap(),
        "gz" | "gzip" => ContentType::parse("application/gzip").unwrap(),
        "doc" => ContentType::parse("application/msword").unwrap(),
        "docx" => ContentType::parse(
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        )
        .unwrap(),
        "xls" => ContentType::parse("application/vnd.ms-excel").unwrap(),
        "xlsx" => {
            ContentType::parse("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet")
                .unwrap()
        }
        "ppt" => ContentType::parse("application/vnd.ms-powerpoint").unwrap(),
        "pptx" => ContentType::parse(
            "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        )
        .unwrap(),
        "png" => ContentType::parse("image/png").unwrap(),
        "jpg" | "jpeg" => ContentType::parse("image/jpeg").unwrap(),
        "gif" => ContentType::parse("image/gif").unwrap(),
        "svg" => ContentType::parse("image/svg+xml").unwrap(),
        "webp" => ContentType::parse("image/webp").unwrap(),
        "mp3" => ContentType::parse("audio/mpeg").unwrap(),
        "mp4" => ContentType::parse("video/mp4").unwrap(),
        "txt" => ContentType::TEXT_PLAIN,
        "html" | "htm" => ContentType::TEXT_HTML,
        "csv" => ContentType::parse("text/csv").unwrap(),
        "json" => ContentType::parse("application/json").unwrap(),
        "xml" => ContentType::parse("application/xml").unwrap(),
        "eml" => ContentType::parse("message/rfc822").unwrap(),
        _ => ContentType::parse("application/octet-stream").unwrap(),
    }
}

#[cfg(test)]
mod tls_config_tests {
    use super::{build_native_tls_connector, build_rustls_client_config};

    #[test]
    fn build_rustls_client_config_returns_result() {
        assert!(build_rustls_client_config(false).is_ok());
        assert!(build_rustls_client_config(true).is_ok());
    }

    #[test]
    fn build_native_tls_connector_returns_result() {
        assert!(build_native_tls_connector(false).is_ok());
        assert!(build_native_tls_connector(true).is_ok());
    }
}

#[allow(clippy::too_many_arguments)]
fn build_message(
    from: &pebble_core::EmailAddress,
    to: &[String],
    cc: &[String],
    bcc: &[String],
    subject: &str,
    body_text: &str,
    body_html: Option<&str>,
    in_reply_to: Option<&str>,
    attachment_paths: &[String],
) -> Result<lettre::Message> {
    if to.is_empty() {
        return Err(PebbleError::Internal("No recipients".to_string()));
    }

    let from_mailbox = crate::sender::sender_mailbox(from)?;

    let mut builder = lettre::Message::builder()
        .from(from_mailbox)
        .subject(subject);

    for addr in to {
        let mailbox: Mailbox = addr
            .parse()
            .map_err(|e| PebbleError::Internal(format!("Invalid to address '{addr}': {e}")))?;
        builder = builder.to(mailbox);
    }

    for addr in cc {
        let mailbox: Mailbox = addr
            .parse()
            .map_err(|e| PebbleError::Internal(format!("Invalid cc address '{addr}': {e}")))?;
        builder = builder.cc(mailbox);
    }

    for addr in bcc {
        let mailbox: Mailbox = addr
            .parse()
            .map_err(|e| PebbleError::Internal(format!("Invalid bcc address '{addr}': {e}")))?;
        builder = builder.bcc(mailbox);
    }

    if let Some(reply_to) = in_reply_to {
        builder = builder.in_reply_to(reply_to.to_string());
    }

    let alternative_body = MultiPart::alternative()
        .singlepart(
            SinglePart::builder()
                .content_type(ContentType::TEXT_PLAIN)
                .body(body_text.to_string()),
        )
        .singlepart(
            SinglePart::builder()
                .content_type(ContentType::TEXT_HTML)
                .body(body_html.unwrap_or(body_text).to_string()),
        );

    let email = if attachment_paths.is_empty() {
        builder
            .multipart(alternative_body)
            .map_err(|e| PebbleError::Internal(format!("Failed to build email: {e}")))?
    } else {
        let mut mixed = MultiPart::mixed().multipart(alternative_body);

        for path_str in attachment_paths {
            let path = Path::new(path_str);
            let filename = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("attachment")
                .to_string();

            let file_bytes = std::fs::read(path).map_err(|e| {
                PebbleError::Internal(format!("Failed to read attachment '{}': {e}", path_str))
            })?;

            let content_type =
                mime_type_from_extension(path.extension().and_then(|e| e.to_str()).unwrap_or(""));

            let attachment = Attachment::new(filename).body(Body::new(file_bytes), content_type);

            mixed = mixed.singlepart(attachment);
        }

        builder
            .multipart(mixed)
            .map_err(|e| PebbleError::Internal(format!("Failed to build email: {e}")))?
    };

    Ok(email)
}

#[cfg(test)]
mod sender_identity_tests {
    use super::*;

    #[test]
    fn smtp_wire_name_is_encoded_without_changing_envelope_address() {
        let from = pebble_core::EmailAddress {
            name: Some("张三, \"财务\"".into()),
            address: "sender@example.com".into(),
        };
        let message = build_message(
            &from,
            &["to@example.com".into()],
            &[],
            &[],
            "Subject",
            "Body",
            None,
            None,
            &[],
        )
        .unwrap();
        assert_eq!(
            message.envelope().from().unwrap().to_string(),
            "sender@example.com"
        );
        let raw = message.formatted();
        let parsed = mail_parser::MessageParser::default().parse(&raw).unwrap();
        let sender = parsed.from().unwrap().first().unwrap();
        assert_eq!(sender.name.as_deref(), from.name.as_deref());
        assert_eq!(sender.address.as_deref(), Some("sender@example.com"));
    }
}
