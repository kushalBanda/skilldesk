//! Structured error returned across the IPC boundary (ADR 0007).
//!
//! Serialized as `{"kind": "io", "message": "..."}` so the UI can branch on
//! `kind` and still show a readable `message`. `details` is omitted when absent.

use serde::Serialize;
use std::fmt;
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ErrorKind {
    Io,
    Parse,
    NotFound,
    InvalidInput,
    Permission,
    /// A write was refused because the target is not ours to replace.
    /// Always carries `details.conflicts`.
    TargetConflict,
    Cancelled,
    /// No connection, a timeout, or a marketplace host answered with an error (ADR 0016).
    Network,
    Database,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct TargetConflictDetail {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct ErrorDetails {
    pub conflicts: Vec<TargetConflictDetail>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS, thiserror::Error)]
#[ts(export)]
pub struct AppError {
    pub kind: ErrorKind,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub details: Option<ErrorDetails>,
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl AppError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            details: None,
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Internal, message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_error_serializes_snake_case_kind_and_omits_empty_details() {
        let err = AppError::new(ErrorKind::TargetConflict, "refused");
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["kind"], "target_conflict");
        assert_eq!(json["message"], "refused");
        assert!(json.get("details").is_none());
    }
}
