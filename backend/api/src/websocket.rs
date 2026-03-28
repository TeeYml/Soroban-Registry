use axum::{
    extract::ws::{WebSocketUpgrade, WebSocket},
    extract::State,
};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::broadcast;
use crate::state::{AppState, RealtimeEvent};

pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl axum::response::IntoResponse {
    ws.on_upgrade(|socket| handle_connection(socket, state))
}

async fn handle_connection(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();
    let mut broadcaster = state.event_broadcaster.subscribe();

    // Spawn a task to forward broadcast messages to this client
    let send_task = tokio::spawn(async move {
        while let Ok(event) = broadcaster.recv().await {
            if let Ok(json) = serde_json::to_string(&event) {
                let msg = axum::extract::ws::Message::Text(json);
                if sender.send(msg).await.is_err() {
                    break;
                }
            }
        }
    });

    // Handle incoming messages (mainly ping/pong for keeping connection alive)
    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            axum::extract::ws::Message::Text(text) => {
                if text == "ping" {
                    let _ = receiver
                        .get_mut()
                        .send(axum::extract::ws::Message::Text("pong".to_string()))
                        .await;
                }
            }
            axum::extract::ws::Message::Close(_) => break,
            _ => {}
        }
    }

    send_task.abort();
}

