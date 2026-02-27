use crate::http::AppState;
use crate::http::device::handle_imu_ws;
use axum::extract::State;
use axum::extract::ws::WebSocketUpgrade;
use axum::response::IntoResponse;
use tracing::warn;

pub async fn imu_upgrade(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| async move {
        if let Err(err) = handle_imu_ws(socket, state).await {
            warn!(error = %err, "imu websocket terminated early");
        }
    })
}
