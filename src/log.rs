use std::fmt;
use tracing::{
    field::{Field, Visit},
    Event, Subscriber,
};
use tracing_subscriber::{layer::Context, prelude::*, registry::LookupSpan, Layer};

// This looks for message lines that being with the ']' character to output those to stdout, all
// other messages are output to stderr
struct OutputSplitter {}

// Implement a visitor to extract fields from the event
struct FieldVisitor {
    message: Option<String>,
}

impl Visit for FieldVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{:?}", value));
        }
    }
}

impl<S> Layer<S> for OutputSplitter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        // try to extract the message field from the event
        let mut visitor = FieldVisitor { message: None };
        event.record(&mut visitor);

        // if we have a message, check to see if it starts with a ']' character, strip that off and
        // output the rest to stdout, otherwise output to stderr
        if let Some(message) = visitor.message {
            if message.starts_with(']') {
                let message = message.trim_start_matches(']').trim();
                eprintln!("{message}");
            } else if message.starts_with(' ') {
                let level = *event.metadata().level();
                let message = message.trim();
                eprintln!("{level}: {message}");
            }
        }
    }
}

/// Async tracing logger wrapper that filters and feeds log messages over an mpsc channel for
/// integration into the TUI gui.
pub struct Log;

impl Log {
    /// Starts the logger and returns the task handle and receiver for the log messages.
    pub fn init() {
        tracing_subscriber::registry()
            .with(OutputSplitter {})
            .init();
    }
}
