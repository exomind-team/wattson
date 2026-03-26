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

pub mod api;
pub mod chart;
pub mod config;
pub mod data;
pub mod error;
pub mod gui;
pub mod gui_settings;
pub mod history;
pub mod protocol;
pub mod runtime;
pub mod serial;
pub mod tui;

pub use config::Config;
pub use data::{CostData, DeviceProfile, PsuSnapshot};
pub use error::{Result, WattsonError};
pub use serial::{Mode, PsuHandle, PsuMonitor};
