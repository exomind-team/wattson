use thiserror::Error;

#[derive(Error, Debug)]
pub enum WattsonError {
    #[error("Serial port error: {0}")]
    Serial(#[from] serialport::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Protocol error: {message}")]
    Protocol { message: String },

    #[error("Device not connected")]
    NotConnected,

    #[error("Timeout waiting for data")]
    Timeout,
}

pub type Result<T> = std::result::Result<T, WattsonError>;
