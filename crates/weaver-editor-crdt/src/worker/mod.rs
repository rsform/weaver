//! Worker implementation for off-main-thread CRDT operations.
//!
//! Currently WASM-specific using gloo-worker, but the core state machine
//! could be abstracted to work with any async channel pair.

mod reactor;

pub use reactor::{WorkerInput, WorkerOutput};

#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub use reactor::EditorReactor;
