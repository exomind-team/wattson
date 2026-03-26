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

pub mod data;
pub mod error;
pub mod protocol;
pub mod serial;

pub use data::{PsuSnapshot, DeviceProfile};
pub use error::{WattsonError, Result};
pub use serial::{PsuMonitor, PsuHandle, Mode};
