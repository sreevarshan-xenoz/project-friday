//! Typed error definitions for friday-os-helper.

use thiserror::Error;

/// High-level errors produced by helper operations.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum HelperError {
    #[error("authorization denied")]
    AuthDenied,

    #[error("authorizer error: {0}")]
    AuthError(String),

    #[error("another job is running")]
    AlreadyRunning,

    #[error("invalid package name \"{0}\"")]
    InvalidPackageName(String),

    #[error("package \"{0}\" is protected and cannot be removed")]
    ProtectedPackage(String),

    #[error("package \"{0}\" not found in repositories")]
    PackageNotFound(String),

    #[error("job \"{0}\" not found")]
    JobNotFound(String),

    #[error("execution error: {0}")]
    Execution(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        assert_eq!(HelperError::AuthDenied.to_string(), "authorization denied");
        assert_eq!(
            HelperError::AlreadyRunning.to_string(),
            "another job is running"
        );
        assert_eq!(
            HelperError::InvalidPackageName("bad/name".into()).to_string(),
            "invalid package name \"bad/name\""
        );
        assert_eq!(
            HelperError::ProtectedPackage("base".into()).to_string(),
            "package \"base\" is protected and cannot be removed"
        );
        assert_eq!(
            HelperError::PackageNotFound("missing".into()).to_string(),
            "package \"missing\" not found in repositories"
        );
        assert_eq!(
            HelperError::JobNotFound("job-1".into()).to_string(),
            "job \"job-1\" not found"
        );
    }
}
