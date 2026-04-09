use chrono::NaiveDate;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Database error: {0}")]
    DatabaseError(#[from] rusqlite::Error),

    #[error("Excel error: {0}")]
    ExcelError(String),

    #[error("Domain error: {0}")]
    DomainError(String),

    #[error("Chronological violation: new date {new_date} <= existing max {existing_max}")]
    ChronologicalViolation {
        new_date: NaiveDate,
        existing_max: NaiveDate,
    },

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Internal error: {0}")]
    InternalError(String),
}

pub type AppResult<T> = Result<T, AppError>;
