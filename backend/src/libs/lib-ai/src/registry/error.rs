use crate::registry::ParamType;

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    BackendNotFound { name: String },
    BackendFailedToCreate { reason: String },
    MissingParameter { key: String, value: ParamType },
    IncorrectDataType { message: String },
}

impl core::fmt::Display for Error {
    fn fmt(&self, fmt: &mut core::fmt::Formatter<'_>) -> core::result::Result<(), core::fmt::Error> {
        write!(fmt, "{self:?}")
    }
}

impl std::error::Error for Error {}
