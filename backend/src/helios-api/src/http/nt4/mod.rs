use axum::{Router, routing::post};

#[cfg(test)]
mod tests;
mod topics;
mod transport;
mod types;
mod value;

pub(crate) use topics::{__path_list_topics, list_topics};
pub(crate) use value::{__path_read_value, read_value};

pub fn router() -> Router<super::AppState> {
    Router::new().route("/topics", post(list_topics)).route("/value", post(read_value))
}
