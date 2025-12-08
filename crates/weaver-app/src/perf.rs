//! Performance timing utilities for instrumentation.
//!
//! Provides a cross-platform wrapper around Performance.now() for WASM
//! and a no-op fallback for native builds.

/// Get the current high-resolution timestamp in milliseconds.
///
/// On WASM, this uses `Performance.now()` from the Web Performance API.
/// On native builds, returns 0.0 (instrumentation is primarily for browser profiling).
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub fn now() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or(0.0)
}

#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
pub fn now() -> f64 {
    0.0
}

/// Measure the execution time of a closure and log it.
///
/// Returns the closure's result and logs the elapsed time via tracing.
#[allow(dead_code)]
pub fn measure<T, F: FnOnce() -> T>(label: &str, f: F) -> T {
    let start = now();
    let result = f();
    let elapsed = now() - start;
    tracing::debug!(elapsed_ms = elapsed, "{}", label);
    result
}

/// A guard that logs elapsed time when dropped.
///
/// Useful for timing blocks of code without closures.
#[allow(dead_code)]
pub struct TimingGuard {
    label: &'static str,
    start: f64,
}

impl TimingGuard {
    pub fn new(label: &'static str) -> Self {
        Self {
            label,
            start: now(),
        }
    }
}

impl Drop for TimingGuard {
    fn drop(&mut self) {
        let elapsed = now() - self.start;
        tracing::debug!(elapsed_ms = elapsed, "{}", self.label);
    }
}
