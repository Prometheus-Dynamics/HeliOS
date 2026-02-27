use actix_web_actors::ws;

use crate::app::WsSession;

/// Convenience wrapper around `WebsocketContext`.
///
/// WebSocket command and loop handlers receive a `WsContext` so they can
/// easily send enveloped responses using the handler's command path.
#[derive(Copy, Clone)]
pub struct WsContext {
    path: &'static str,
    ctx: *mut ws::WebsocketContext<WsSession>,
}

impl WsContext {
    pub fn new(path: &'static str, ctx: &mut ws::WebsocketContext<WsSession>) -> Self {
        Self { path, ctx }
    }

    pub fn push<T: serde::Serialize>(&mut self, payload: T) {
        let ts = chrono::Utc::now().to_rfc3339();
        let val = serde_json::json!({
            "cmd": self.path,
            "payload": payload,
            "sent-at": ts,
        });
        unsafe { &mut *self.ctx }.text(val.to_string());
    }
}

impl std::ops::Deref for WsContext {
    type Target = ws::WebsocketContext<WsSession>;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.ctx }
    }
}

impl std::ops::DerefMut for WsContext {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.ctx }
    }
}
