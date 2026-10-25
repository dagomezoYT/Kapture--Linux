//! Recorder core: clock, bounded queues, state machine, session orchestration.
//!
//! This crate must remain free of Windows/Linux API types. Platform backends
//! implement traits defined in sibling crates.

#![forbid(unsafe_code)]

pub mod clock;
pub mod error;
pub mod governor;
pub mod memory;
pub mod policy;
pub mod queue;
pub mod session;
pub mod state_machine;

pub use clock::MasterClock;
pub use error::CoreError;
pub use governor::{evaluate, SaturationStreak};
pub use memory::MemoryTracker;
pub use session::{RecorderBuilder, RecorderSession, SessionOutcome};
pub use state_machine::StateMachine;
