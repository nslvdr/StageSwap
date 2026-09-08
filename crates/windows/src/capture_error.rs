//! Capture failures carry policy separately from human-readable diagnostics.
use stageswap_core::{ScreenFailureKind, WebcamFailureKind};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureError<K> {
    pub kind: K,
    pub message: String,
}

impl<K> CaptureError<K> {
    pub fn new(kind: K, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl<K> std::fmt::Display for CaptureError<K> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl<K: std::fmt::Debug> std::error::Error for CaptureError<K> {}

pub type WebcamError = CaptureError<WebcamFailureKind>;
pub type ScreenError = CaptureError<ScreenFailureKind>;

impl From<String> for WebcamError {
    fn from(message: String) -> Self {
        Self::new(WebcamFailureKind::DriverFailure, message)
    }
}

impl From<&str> for WebcamError {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
}

impl From<String> for ScreenError {
    fn from(message: String) -> Self {
        Self::new(ScreenFailureKind::CaptureFailure, message)
    }
}

impl From<&str> for ScreenError {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
}
