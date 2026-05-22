//! # Wattson
//!
//! Universal digital PSU monitoring library — read real-time power consumption
//! from your computer via serial protocols.
//!
//! ## Quick Start
//!
//! ```no_run
//! use wattson::{PsuMonitor, Mode};
//!
//! let monitor = PsuMonitor::new("COM4", Mode::Passive);
//! let handle = monitor.start().unwrap();
//!
//! let snapshot = handle.latest();
//! println!("AC Input: {:.1}W", snapshot.power.ac_input_w);
//! ```

// Core modules (always compiled)
pub mod data;
pub mod error;
pub mod protocol;
pub mod serial;

// Optional modules
#[cfg(feature = "server")]
pub mod api;
#[cfg(feature = "chart")]
pub mod chart;
#[cfg(feature = "config")]
pub mod config;
#[cfg(feature = "gui")]
pub mod gui;
#[cfg(feature = "gui")]
pub mod gui_settings;
#[cfg(feature = "history")]
pub mod history;
#[cfg(feature = "runtime")]
pub mod runtime;
#[cfg(feature = "tui")]
pub mod tui;

// Core re-exports (always available)
pub use data::{CostData, DeviceProfile, PsuSnapshot};
pub use error::{Result, WattsonError};
pub use protocol::FanMode;
pub use serial::{Mode, PsuHandle, PsuMonitor};

// Test utility re-export
#[cfg(test)]
pub use serial::test_handle_with_recorder;
