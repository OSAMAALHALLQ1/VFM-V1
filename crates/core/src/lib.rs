//! Platform-agnostic player logic. No FFI, no UI.

#![forbid(unsafe_code)]

pub mod brand;
pub mod cli;
pub mod error;
pub mod model;
pub mod platform;
pub mod playlist;
pub mod queue;
pub mod scanner;
pub mod session;
pub mod settings;
pub mod state;
pub mod store;
pub mod telemetry;
pub mod time;
