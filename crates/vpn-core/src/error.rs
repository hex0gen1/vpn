use serde::{Deserialize, Serialize};
use thiserror::Error;
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("already connected")]
    AlreadyConnected,
    #[error("connection already in progress")]
    AlreadyConnecting,
    #[error("not connected")]
    NotConnected,
    #[error("server not found: {0}")]
    ServerNotFound(String),
}
