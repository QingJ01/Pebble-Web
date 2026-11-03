use axum::{
    extract::{
        ws::{CloseFrame, Message, Utf8Bytes, WebSocket, WebSocketUpgrade},
        State,
    },
    response::Response,
};
use tokio::sync::broadcast;

use crate::auth::validate_token;
use crate::state::AppStateRef;

pub async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppStateRef>) -> Response {
    let jwt_secret = state.config.jwt_secret.clone();
    let rx = state.ws_broadcast.subscribe();
    ws.on_upgrade(move |socket| handle_socket(socket, rx, jwt_secret))
}

async fn handle_socket(
    mut socket: WebSocket,
    mut rx: broadcast::Receiver<String>,
    jwt_secret: String,
) {
    // First message must be the auth token
    let authenticated =
        match tokio::time::timeout(std::time::Duration::from_secs(10), socket.recv()).await {
            Ok(Some(Ok(Message::Text(token)))) => {
                validate_token(token.as_str(), &jwt_secret).is_ok()
            }
            _ => false,
        };

    if !authenticated {
        let _ = socket
            .send(Message::Text(Utf8Bytes::from(
                r#"{"type":"error","message":"unauthorized"}"#.to_string(),
            )))
            .await;
        let _ = socket
            .send(Message::Close(Some(CloseFrame {
                code: 4001,
                reason: Utf8Bytes::from("unauthorized"),
            })))
            .await;
        return;
    }

    let _ = socket
        .send(Message::Text(Utf8Bytes::from(
            r#"{"type":"authenticated"}"#.to_string(),
        )))
        .await;

    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Ok(text) => {
                        let bytes = Utf8Bytes::from(text);
                        if socket.send(Message::Text(bytes)).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
}
