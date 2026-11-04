use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use pebble_core::{PebbleError, Result};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, ReadBuf};
use tokio::net::TcpStream;
use tokio_native_tls as async_native_tls;
use tracing::debug;

use crate::imap::{ConnectionSecurity, ProxyConfig};

const POP3_CONNECT_TIMEOUT_SECS: u64 = 15;
const POP3_COMMAND_TIMEOUT_SECS: u64 = 45;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Pop3Config {
    pub host: String,
    pub port: u16,
    pub username: String,
    #[serde(skip_serializing)]
    pub password: String,
    pub security: ConnectionSecurity,
    #[serde(default, skip_serializing_if = "is_false")]
    pub accept_invalid_certs: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy: Option<ProxyConfig>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pop3MessageRef {
    pub number: u32,
    pub uid: String,
    pub size: Option<u64>,
}

enum Pop3Stream {
    Plain(TcpStream),
    Rustls(Box<tokio_rustls::client::TlsStream<TcpStream>>),
    NativeTls(Box<async_native_tls::TlsStream<TcpStream>>),
}

impl AsyncRead for Pop3Stream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match &mut *self {
            Self::Plain(stream) => Pin::new(stream).poll_read(cx, buf),
            Self::Rustls(stream) => Pin::new(stream.as_mut()).poll_read(cx, buf),
            Self::NativeTls(stream) => Pin::new(stream.as_mut()).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for Pop3Stream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match &mut *self {
            Self::Plain(stream) => Pin::new(stream).poll_write(cx, buf),
            Self::Rustls(stream) => Pin::new(stream.as_mut()).poll_write(cx, buf),
            Self::NativeTls(stream) => Pin::new(stream.as_mut()).poll_write(cx, buf),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match &mut *self {
            Self::Plain(stream) => Pin::new(stream).poll_flush(cx),
            Self::Rustls(stream) => Pin::new(stream.as_mut()).poll_flush(cx),
            Self::NativeTls(stream) => Pin::new(stream.as_mut()).poll_flush(cx),
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match &mut *self {
            Self::Plain(stream) => Pin::new(stream).poll_shutdown(cx),
            Self::Rustls(stream) => Pin::new(stream.as_mut()).poll_shutdown(cx),
            Self::NativeTls(stream) => Pin::new(stream.as_mut()).poll_shutdown(cx),
        }
    }
}

pub struct Pop3Provider {
    config: Pop3Config,
}

impl Pop3Provider {
    pub fn new(config: Pop3Config) -> Self {
        Self { config }
    }

    pub fn config(&self) -> Pop3Config {
        self.config.clone()
    }

    pub async fn test_connection(config: &Pop3Config) -> Result<String> {
        let mut client = Self::connect_client(config).await?;
        let messages = client.list_messages().await?;
        let _ = client.quit().await;
        Ok(format!(
            "POP3 connection successful ({} messages)",
            messages.len()
        ))
    }

    pub async fn list_messages(&self) -> Result<Vec<Pop3MessageRef>> {
        let mut client = Self::connect_client(&self.config).await?;
        let messages = client.list_messages().await;
        let _ = client.quit().await;
        messages
    }

    pub async fn list_and_retrieve_selected<F>(
        &self,
        select: F,
    ) -> Result<(Vec<Pop3MessageRef>, Vec<(Pop3MessageRef, Vec<u8>)>)>
    where
        F: FnOnce(&[Pop3MessageRef]) -> Result<Vec<Pop3MessageRef>>,
    {
        let mut client = Self::connect_client(&self.config).await?;
        let messages = match client.list_messages().await {
            Ok(messages) => messages,
            Err(e) => {
                let _ = client.quit().await;
                return Err(e);
            }
        };
        let selected = match select(&messages) {
            Ok(selected) => selected,
            Err(e) => {
                let _ = client.quit().await;
                return Err(e);
            }
        };

        let mut retrieved = Vec::with_capacity(selected.len());
        for message_ref in selected {
            match client.retrieve(message_ref.number).await {
                Ok(raw) => retrieved.push((message_ref, raw)),
                Err(e) => {
                    let _ = client.quit().await;
                    return Err(e);
                }
            }
        }
        let _ = client.quit().await;
        Ok((messages, retrieved))
    }

    pub async fn retrieve_message(&self, number: u32) -> Result<Vec<u8>> {
        let mut client = Self::connect_client(&self.config).await?;
        let message = client.retrieve(number).await;
        let _ = client.quit().await;
        message
    }

    async fn connect_client(config: &Pop3Config) -> Result<Pop3Client> {
        match config.security {
            ConnectionSecurity::Tls => Self::connect_implicit_tls(config).await,
            ConnectionSecurity::StartTls => match Self::connect_starttls(config, false).await {
                Ok(client) => Ok(client),
                Err(rustls_err) => {
                    debug!(
                        "POP3 STARTTLS with rustls failed ({}), retrying with native-tls",
                        rustls_err
                    );
                    Self::connect_starttls(config, true)
                        .await
                        .map_err(|native_err| {
                            PebbleError::Network(format!(
                                    "POP3 STARTTLS failed with both TLS backends — rustls: {rustls_err}, native-tls: {native_err}"
                                ))
                        })
                }
            },
            ConnectionSecurity::Plain => {
                let tcp = connect_tcp(config).await?;
                let mut client = Pop3Client::new(Pop3Stream::Plain(tcp));
                client.read_status().await?;
                client.login(&config.username, &config.password).await?;
                Ok(client)
            }
        }
    }

    async fn connect_implicit_tls(config: &Pop3Config) -> Result<Pop3Client> {
        let tcp = connect_tcp(config).await?;
        let stream = match rustls_connect(&config.host, tcp, config.accept_invalid_certs).await {
            Ok(tls) => Pop3Stream::Rustls(Box::new(tls)),
            Err(rustls_err) => {
                debug!(
                    "POP3 rustls handshake failed ({}), retrying with native-tls",
                    rustls_err
                );
                let tcp = connect_tcp(config).await?;
                let tls = native_tls_connect(&config.host, tcp, config.accept_invalid_certs)
                    .await
                    .map_err(|e| {
                        PebbleError::Network(format!(
                            "POP3 TLS failed with both backends — rustls: {rustls_err}, native-tls: {e}"
                        ))
                    })?;
                Pop3Stream::NativeTls(Box::new(tls))
            }
        };
        let mut client = Pop3Client::new(stream);
        client.read_status().await?;
        client.login(&config.username, &config.password).await?;
        Ok(client)
    }

    async fn connect_starttls(config: &Pop3Config, use_native_tls: bool) -> Result<Pop3Client> {
        let tcp = connect_tcp(config).await?;
        let mut client = Pop3Client::new(Pop3Stream::Plain(tcp));
        client.read_status().await?;
        client.command("STLS").await?;

        let stream = client.into_inner();
        let Pop3Stream::Plain(tcp) = stream else {
            return Err(PebbleError::Network(
                "POP3 STARTTLS attempted on TLS stream".to_string(),
            ));
        };

        let upgraded = if use_native_tls {
            let tls = native_tls_connect(&config.host, tcp, config.accept_invalid_certs).await?;
            Pop3Stream::NativeTls(Box::new(tls))
        } else {
            let tls = rustls_connect(&config.host, tcp, config.accept_invalid_certs).await?;
            Pop3Stream::Rustls(Box::new(tls))
        };

        let mut client = Pop3Client::new(upgraded);
        client.login(&config.username, &config.password).await?;
        Ok(client)
    }
}

struct Pop3Client {
    stream: BufReader<Pop3Stream>,
}

impl Pop3Client {
    fn new(stream: Pop3Stream) -> Self {
        Self {
            stream: BufReader::new(stream),
        }
    }

    fn into_inner(self) -> Pop3Stream {
        self.stream.into_inner()
    }

    async fn login(&mut self, username: &str, password: &str) -> Result<()> {
        validate_command_arg("POP3 username", username)?;
        validate_command_arg("POP3 password", password)?;
        self.command(&format!("USER {username}")).await?;
        self.command(&format!("PASS {password}")).await?;
        Ok(())
    }

    async fn quit(&mut self) -> Result<()> {
        self.command("QUIT").await.map(|_| ())
    }

    async fn list_messages(&mut self) -> Result<Vec<Pop3MessageRef>> {
        let uidl = parse_uidl_lines(&self.command_multiline("UIDL").await?)?;
        let sizes = parse_list_lines(&self.command_multiline("LIST").await?)?;
        let mut messages = uidl
            .into_iter()
            .map(|(number, uid)| Pop3MessageRef {
                number,
                uid,
                size: sizes.get(&number).copied(),
            })
            .collect::<Vec<_>>();
        messages.sort_by_key(|message| message.number);
        Ok(messages)
    }

    async fn retrieve(&mut self, number: u32) -> Result<Vec<u8>> {
        self.command_multiline(&format!("RETR {number}")).await
    }

    async fn command(&mut self, command: &str) -> Result<String> {
        self.write_command(command).await?;
        self.read_status().await
    }

    async fn command_multiline(&mut self, command: &str) -> Result<Vec<u8>> {
        self.write_command(command).await?;
        self.read_status().await?;
        self.read_multiline_bytes().await
    }

    async fn write_command(&mut self, command: &str) -> Result<()> {
        let stream = self.stream.get_mut();
        with_pop3_timeout(
            "POP3 write command",
            POP3_COMMAND_TIMEOUT_SECS,
            stream.write_all(format!("{command}\r\n").as_bytes()),
        )
        .await?;
        with_pop3_timeout("POP3 flush", POP3_COMMAND_TIMEOUT_SECS, stream.flush()).await
    }

    async fn read_status(&mut self) -> Result<String> {
        let mut line = String::new();
        with_pop3_timeout(
            "POP3 read status",
            POP3_COMMAND_TIMEOUT_SECS,
            self.stream.read_line(&mut line),
        )
        .await?;
        if line.starts_with("+OK") {
            Ok(line)
        } else if line.starts_with("-ERR") {
            Err(PebbleError::Network(format!(
                "POP3 server error: {}",
                line.trim_end()
            )))
        } else {
            Err(PebbleError::Network(format!(
                "Unexpected POP3 response: {}",
                line.trim_end()
            )))
        }
    }

    async fn read_multiline_bytes(&mut self) -> Result<Vec<u8>> {
        let mut output = Vec::new();
        loop {
            let mut line = Vec::new();
            let read = with_pop3_timeout(
                "POP3 read multiline",
                POP3_COMMAND_TIMEOUT_SECS,
                self.stream.read_until(b'\n', &mut line),
            )
            .await?;
            if read == 0 {
                return Err(PebbleError::Network(
                    "POP3 connection closed during multiline response".to_string(),
                ));
            }

            let stripped = strip_crlf(&line);
            if stripped == b"." {
                break;
            }

            let body_line = if stripped.starts_with(b"..") {
                &stripped[1..]
            } else {
                stripped
            };
            output.extend_from_slice(body_line);
            output.extend_from_slice(b"\r\n");
        }
        Ok(output)
    }
}

async fn connect_tcp(config: &Pop3Config) -> Result<TcpStream> {
    if let Some(proxy) = &config.proxy {
        let stream = with_pop3_display_timeout(
            "POP3 SOCKS5 connect",
            POP3_CONNECT_TIMEOUT_SECS,
            tokio_socks::tcp::Socks5Stream::connect(
                (proxy.host.as_str(), proxy.port),
                (config.host.as_str(), config.port),
            ),
        )
        .await?;
        Ok(stream.into_inner())
    } else {
        with_pop3_timeout(
            "POP3 TCP connect",
            POP3_CONNECT_TIMEOUT_SECS,
            TcpStream::connect((config.host.as_str(), config.port)),
        )
        .await
        .map_err(|e| PebbleError::Network(format!("POP3 TCP connect failed: {e}")))
    }
}

fn build_rustls_connector(accept_invalid_certs: bool) -> Result<tokio_rustls::TlsConnector> {
    use crate::imap::AcceptInvalidCertsVerifier;

    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let mut root_store = rustls::RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let builder = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| PebbleError::Network(format!("POP3 TLS protocol versions: {e}")))?;
    let config = if accept_invalid_certs {
        builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptInvalidCertsVerifier::new(root_store)))
            .with_no_client_auth()
    } else {
        builder
            .with_root_certificates(root_store)
            .with_no_client_auth()
    };
    Ok(tokio_rustls::TlsConnector::from(Arc::new(config)))
}

async fn rustls_connect(
    host: &str,
    tcp: TcpStream,
    accept_invalid_certs: bool,
) -> Result<tokio_rustls::client::TlsStream<TcpStream>> {
    let connector = build_rustls_connector(accept_invalid_certs)?;
    let server_name = rustls::pki_types::ServerName::try_from(host)
        .map_err(|e| PebbleError::Network(format!("Invalid server name '{}': {}", host, e)))?
        .to_owned();
    with_pop3_display_timeout(
        &format!("POP3 TLS handshake (rustls) with {host}"),
        POP3_CONNECT_TIMEOUT_SECS,
        connector.connect(server_name, tcp),
    )
    .await
}

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
        .map_err(|e| PebbleError::Network(format!("POP3 native-tls init: {e}")))?;
    Ok(async_native_tls::TlsConnector::from(connector))
}

async fn native_tls_connect(
    host: &str,
    tcp: TcpStream,
    accept_invalid_certs: bool,
) -> Result<async_native_tls::TlsStream<TcpStream>> {
    let connector = build_native_tls_connector(accept_invalid_certs)?;
    with_pop3_display_timeout(
        &format!("POP3 TLS handshake (native-tls) with {host}"),
        POP3_CONNECT_TIMEOUT_SECS,
        connector.connect(host, tcp),
    )
    .await
}

async fn with_pop3_timeout<T, F>(operation: &str, seconds: u64, future: F) -> Result<T>
where
    F: std::future::Future<Output = std::result::Result<T, std::io::Error>>,
{
    tokio::time::timeout(std::time::Duration::from_secs(seconds), future)
        .await
        .map_err(|_| PebbleError::Network(format!("{operation} timed out after {seconds}s")))?
        .map_err(|e| PebbleError::Network(format!("{operation}: {e}")))
}

async fn with_pop3_display_timeout<T, E, F>(operation: &str, seconds: u64, future: F) -> Result<T>
where
    E: std::fmt::Display,
    F: std::future::Future<Output = std::result::Result<T, E>>,
{
    tokio::time::timeout(std::time::Duration::from_secs(seconds), future)
        .await
        .map_err(|_| PebbleError::Network(format!("{operation} timed out after {seconds}s")))?
        .map_err(|e| PebbleError::Network(format!("{operation}: {e}")))
}

fn strip_crlf(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\r\n")
        .or_else(|| line.strip_suffix(b"\n"))
        .unwrap_or(line)
}

fn parse_uidl_lines(lines: &[u8]) -> Result<Vec<(u32, String)>> {
    let text = std::str::from_utf8(lines)
        .map_err(|e| PebbleError::Network(format!("POP3 UIDL response was not UTF-8: {e}")))?;
    let mut entries = Vec::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let number = parts
            .next()
            .ok_or_else(|| PebbleError::Network("POP3 UIDL response missing number".to_string()))?
            .parse::<u32>()
            .map_err(|e| PebbleError::Network(format!("Invalid POP3 UIDL number: {e}")))?;
        let uid = parts
            .next()
            .ok_or_else(|| PebbleError::Network("POP3 UIDL response missing UID".to_string()))?
            .to_string();
        entries.push((number, uid));
    }
    Ok(entries)
}

fn parse_list_lines(lines: &[u8]) -> Result<HashMap<u32, u64>> {
    let text = std::str::from_utf8(lines)
        .map_err(|e| PebbleError::Network(format!("POP3 LIST response was not UTF-8: {e}")))?;
    let mut entries = HashMap::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let number = parts
            .next()
            .ok_or_else(|| PebbleError::Network("POP3 LIST response missing number".to_string()))?
            .parse::<u32>()
            .map_err(|e| PebbleError::Network(format!("Invalid POP3 LIST number: {e}")))?;
        let size = parts
            .next()
            .ok_or_else(|| PebbleError::Network("POP3 LIST response missing size".to_string()))?
            .parse::<u64>()
            .map_err(|e| PebbleError::Network(format!("Invalid POP3 LIST size: {e}")))?;
        entries.insert(number, size);
    }
    Ok(entries)
}

fn validate_command_arg(label: &str, value: &str) -> Result<()> {
    if value.as_bytes().iter().any(|b| matches!(b, b'\r' | b'\n')) {
        return Err(PebbleError::Validation(format!(
            "{label} must not contain line breaks"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        build_native_tls_connector, build_rustls_connector, parse_list_lines, parse_uidl_lines,
        strip_crlf, validate_command_arg,
    };

    #[test]
    fn parse_uidl_lines_extracts_message_numbers_and_uids() {
        let parsed = parse_uidl_lines(b"1 uid-a\r\n2 uid-b\r\n").unwrap();

        assert_eq!(
            parsed,
            vec![(1, "uid-a".to_string()), (2, "uid-b".to_string())]
        );
    }

    #[test]
    fn parse_list_lines_extracts_sizes_by_message_number() {
        let parsed = parse_list_lines(b"1 120\r\n2 4096\r\n").unwrap();

        assert_eq!(parsed.get(&1), Some(&120));
        assert_eq!(parsed.get(&2), Some(&4096));
    }

    #[test]
    fn strip_crlf_handles_crlf_lf_and_bare_lines() {
        assert_eq!(strip_crlf(b"hello\r\n"), b"hello");
        assert_eq!(strip_crlf(b"hello\n"), b"hello");
        assert_eq!(strip_crlf(b"hello"), b"hello");
    }

    #[test]
    fn pop3_command_args_reject_line_breaks() {
        validate_command_arg("POP3 username", "user@example.com").unwrap();
        assert!(validate_command_arg("POP3 username", "user\r\nSTAT").is_err());
        assert!(validate_command_arg("POP3 password", "pass\nQUIT").is_err());
    }

    #[test]
    fn build_rustls_connector_returns_result() {
        assert!(build_rustls_connector(false).is_ok());
        assert!(build_rustls_connector(true).is_ok());
    }

    #[test]
    fn build_native_tls_connector_returns_result() {
        assert!(build_native_tls_connector(false).is_ok());
        assert!(build_native_tls_connector(true).is_ok());
    }
}
