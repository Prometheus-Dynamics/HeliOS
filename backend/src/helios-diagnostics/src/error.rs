use derive_more::{Display, From};

#[derive(Debug, Display, From)]
pub enum Error {
    Io(std::io::Error),
    Json(serde_json::Error),
    Glob(glob::PatternError),
    Utf8(std::string::FromUtf8Error),
    Msg(String),
}

pub type Result<T> = core::result::Result<T, Error>;

impl Error {
    pub fn msg<S: Into<String>>(s: S) -> Self {
        Self::Msg(s.into())
    }
}
