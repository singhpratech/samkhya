use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serde(String),

    #[error("invalid Puffin file: {0}")]
    InvalidPuffin(String),

    #[error("invalid sketch payload: {0}")]
    InvalidSketch(String),

    #[error("feedback store error: {0}")]
    Feedback(String),

    #[error("LpBound violation: estimated {estimate} exceeds ceiling {ceiling}")]
    LpBoundExceeded { estimate: f64, ceiling: f64 },
}

impl From<bincode::Error> for Error {
    fn from(e: bincode::Error) -> Self {
        Error::Serde(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
