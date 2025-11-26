//! Ring buffer log capture for bug reports.
//!
//! Provides a tracing Layer that captures recent log messages to a fixed-size
//! ring buffer. Logs can be retrieved on-demand for bug reports.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt::Write as FmtWrite;

use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

/// Maximum number of log entries to keep.
const MAX_ENTRIES: usize = 100;

/// Module prefixes to capture in the ring buffer.
const CAPTURED_PREFIXES: &[&str] = &["weaver_", "markdown_weaver"];

thread_local! {
    static LOG_BUFFER: RefCell<VecDeque<String>> = RefCell::new(VecDeque::with_capacity(MAX_ENTRIES));
}

/// Minimum level to buffer from our modules.
const BUFFER_MIN_LEVEL: Level = Level::DEBUG;

/// A tracing Layer that captures log messages to a ring buffer.
/// Console output is handled by WASMLayer in the subscriber stack.
pub struct LogCaptureLayer;

impl<S: Subscriber> Layer<S> for LogCaptureLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let metadata = event.metadata();
        let level = metadata.level();
        let target = metadata.target();

        // Only buffer debug+ logs from our modules
        let is_our_module = CAPTURED_PREFIXES.iter().any(|prefix| target.starts_with(prefix));
        if !is_our_module || *level > BUFFER_MIN_LEVEL {
            return;
        }

        // Format the log entry
        let mut message = String::new();
        let mut visitor = MessageVisitor(&mut message);
        event.record(&mut visitor);

        let formatted = format!("[{}] {}: {}", level_str(level), target, message);

        LOG_BUFFER.with(|buf| {
            let mut buf = buf.borrow_mut();
            if buf.len() >= MAX_ENTRIES {
                buf.pop_front();
            }
            buf.push_back(formatted);
        });
    }
}

/// Visitor that extracts the message field from a tracing event.
struct MessageVisitor<'a>(&'a mut String);

impl Visit for MessageVisitor<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.0, "{:?}", value);
        } else {
            if !self.0.is_empty() {
                self.0.push_str(", ");
            }
            let _ = write!(self.0, "{}={:?}", field.name(), value);
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.0.push_str(value);
        } else {
            if !self.0.is_empty() {
                self.0.push_str(", ");
            }
            let _ = write!(self.0, "{}={}", field.name(), value);
        }
    }
}

fn level_str(level: &Level) -> &'static str {
    match *level {
        Level::ERROR => "ERROR",
        Level::WARN => "WARN",
        Level::INFO => "INFO",
        Level::DEBUG => "DEBUG",
        Level::TRACE => "TRACE",
    }
}

/// Get all captured log entries as a single string.
pub fn get_logs() -> String {
    LOG_BUFFER.with(|buf| {
        let buf = buf.borrow();
        buf.iter().cloned().collect::<Vec<_>>().join("\n")
    })
}

/// Clear the log buffer.
#[allow(dead_code)]
pub fn clear_logs() {
    LOG_BUFFER.with(|buf| buf.borrow_mut().clear());
}
