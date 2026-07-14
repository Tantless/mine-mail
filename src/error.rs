use thiserror::Error;

pub type Result<T> = std::result::Result<T, MailError>;

#[derive(Debug, Error)]
pub enum MailError {
    #[error("configuration error: {0}")]
    Config(String),

    #[error("validation error: {0}")]
    Validation(String),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("IMAP error: {0}")]
    Imap(String),

    #[error("SMTP error: {0}")]
    Smtp(String),

    #[error("message format error: {0}")]
    Mime(String),

    #[error("{operation} timed out")]
    Timeout { operation: &'static str },

    #[error("{entity} was not found: {id}")]
    NotFound { entity: &'static str, id: String },
}
