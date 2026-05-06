use thiserror::Error;

#[derive(Debug, Error)]
pub enum VpnError {
    //Transport
    #[error("Connection timeout")]
    Timeout,
    #[error("DNS resolution failed: {0}")]
    DnsFailed(String),
    #[error("Network unreachable")]
    NetworkUnreachable,
    #[error("Transport I/O error")]
    Io(#[from] std::io::Error),

    //Cryptography
    #[error("Decryption failed: {0}")]
    CryptoError(String),
    #[error("Nonce space exhausted")]
    NonceExhausted,
    #[error("Replay attack detected (replay of nonce)")]
    ReplayDetected,

    //Protocol, framing
    #[error("Invalid frame: {0}")]
    InvalidFrame(String),
    #[error("Protocol version mismatch")]
    VersionMismatch,
    #[error("Frame exceeds max size: bytes({0})")]
    FrameTooLarge(String),

    //Sys/app state
    #[error("TUN interface error: {0}")]
    TunError(String),
    #[error("Backend channel closed")]
    ChannelClosed,
    #[error("Routing command failed: {0}")]
    RoutingError(String),
    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ErrorLevel {
    Recoverable,
    ConnectionFatal,
    AppFatal,
    Warning,
}

impl VpnError {
    pub fn level(&self) -> ErrorLevel {
        match self {
            Self::Timeout | Self::DnsFailed(_) | Self::NetworkUnreachable => {
                ErrorLevel::Recoverable
            }
            Self::CryptoError(_)
            | Self::NonceExhausted
            | Self::TunError(_)
            | Self::ChannelClosed => ErrorLevel::ConnectionFatal,
            Self::InvalidFrame(_)
            | Self::VersionMismatch
            | Self::FrameTooLarge(_)
            | Self::ReplayDetected => ErrorLevel::Warning,
            Self::Io(e) if e.kind() == std::io::ErrorKind::PermissionDenied => ErrorLevel::AppFatal,
            Self::Io(_) => ErrorLevel::Recoverable,
            Self::RoutingError(_) | Self::InvalidConfiguration(_) => ErrorLevel::AppFatal,
        }
    }
    pub fn log_level(&self) -> &'static str {
        match self.level() {
            ErrorLevel::Warning => "DEBUG",
            ErrorLevel::Recoverable => "WARNING",
            ErrorLevel::AppFatal => "FATAL",
            ErrorLevel::ConnectionFatal => "ERROR",
        }
    }
}
