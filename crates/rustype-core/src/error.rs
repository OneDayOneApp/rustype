use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("provider returned HTTP {status}: {body}")]
    Status { status: u16, body: String },
    #[error("invalid response: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("configuration error: {0}")]
    Config(String),
    #[error("api error: {0}")]
    Api(String),
    #[error("audio error: {0}")]
    Audio(String),
}

pub type Result<T> = std::result::Result<T, Error>;
