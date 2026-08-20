use thiserror::Error;

/// Errors from a [`crate::LessonStore`].
#[derive(Debug, Error)]
pub enum LessonStoreError {
    /// The lesson key already exists; the caller should `strengthen` instead.
    #[error("lesson already exists for key {0:?}; use strengthen")]
    AlreadyExists(crate::types::LessonKey),
    /// (De)serialization failure (durable backend).
    #[error("serialization error: {0}")]
    Serialization(String),
    /// Underlying storage failure (durable backend).
    #[error("storage error: {0}")]
    Storage(String),
}
