use crate::state::AppState;
use axum::{
    extract::ws::{WebSocket, WebSocketUpgrade},
    extract::State,
};
use futures_util::{SinkExt, StreamExt};

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
                    // Ignore explicit ping text; heartbeat stream still keeps connection alive.
                }
            }
            axum::extract::ws::Message::Close(_) => break,
            _ => {}
        }
    }

    send_task.abort();
}
