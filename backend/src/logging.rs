//! Logging setup and an in-memory buffer of recent log events for the settings page.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

use crate::metadata::tags;

/// Severity names shown in the UI, lowest first.
pub const SEVERITIES: [&str; 6] = ["Verbose", "Debug", "Information", "Warning", "Error", "Fatal"];

pub fn severity_rank(name: &str) -> Option<usize> {
    SEVERITIES.iter().position(|s| s.eq_ignore_ascii_case(name))
}

fn severity_of(level: &Level) -> &'static str {
    match *level {
        Level::TRACE => "Verbose",
        Level::DEBUG => "Debug",
        Level::INFO => "Information",
        Level::WARN => "Warning",
        Level::ERROR => "Error",
    }
}

#[derive(Debug, Clone)]
pub struct BufferedLogEvent {
    pub timestamp: DateTime<Utc>,
    pub severity: &'static str,
    pub channel: String,
    pub message: String,
    pub exception: Option<String>,
}

pub struct LogBuffer {
    capacity: usize,
    events: Mutex<VecDeque<BufferedLogEvent>>,
}

impl LogBuffer {
    pub fn new(capacity: usize) -> Self {
        Self { capacity: capacity.max(100), events: Mutex::new(VecDeque::new()) }
    }

    pub fn push(&self, event: BufferedLogEvent) {
        let mut events = self.events.lock();
        events.push_back(event);
        while events.len() > self.capacity {
            events.pop_front();
        }
    }

    /// Newest first, optionally filtered by channel (case-insensitive) and minimum severity.
    pub fn get(&self, channel: Option<&str>, minimum: Option<usize>, limit: usize) -> Vec<BufferedLogEvent> {
        let channel = channel.map(str::trim).filter(|c| !c.is_empty());
        let events = self.events.lock();
        events
            .iter()
            .rev()
            .filter(|e| channel.is_none_or(|c| e.channel.eq_ignore_ascii_case(c)))
            .filter(|e| minimum.is_none_or(|m| severity_rank(e.severity).is_some_and(|r| r >= m)))
            .take(limit.clamp(1, 1000))
            .cloned()
            .collect()
    }

    pub fn channels(&self) -> Vec<String> {
        let events = self.events.lock();
        let mut channels: Vec<String> = Vec::new();
        for e in events.iter() {
            if !e.channel.trim().is_empty() && !channels.iter().any(|c| c.eq_ignore_ascii_case(&e.channel)) {
                channels.push(e.channel.clone());
            }
        }
        channels.sort_by(|a, b| tags::cmp_ignore_case(a, b));
        channels
    }
}

#[derive(Default)]
struct MessageVisitor {
    message: String,
    fields: String,
    error: Option<String>,
}

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        match field.name() {
            "message" => {
                let _ = write!(self.message, "{value:?}");
            }
            "error" => self.error = Some(format!("{value:?}")),
            name => {
                let _ = write!(self.fields, " {name}={value:?}");
            }
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "message" => self.message.push_str(value),
            "error" => self.error = Some(value.to_string()),
            name => {
                let _ = write!(self.fields, " {name}={value}");
            }
        }
    }
}

struct BufferLayer {
    buffer: Arc<LogBuffer>,
}

impl<S: Subscriber> Layer<S> for BufferLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);
        let target = meta.target();
        self.buffer.push(BufferedLogEvent {
            timestamp: Utc::now(),
            severity: severity_of(meta.level()),
            channel: if target.is_empty() { "General".to_string() } else { target.to_string() },
            message: format!("{}{}", visitor.message, visitor.fields),
            exception: visitor.error,
        });
    }
}

/// Default filter: info for this app, warnings for dependencies. Override with
/// `FINDAMODEL_LOG` (EnvFilter syntax), e.g. `findamodel=debug,findamodel.Models=debug`.
fn default_filter() -> EnvFilter {
    EnvFilter::try_from_env("FINDAMODEL_LOG")
        .unwrap_or_else(|_| EnvFilter::new("warn,wgpu_hal=error,wgpu_core=error,naga=error,findamodel=info"))
}

/// Installs the global subscriber (console + buffer). Safe to call more than once; later
/// calls are ignored.
pub fn init(buffer: Arc<LogBuffer>) {
    let console = tracing_subscriber::fmt::layer().with_target(true).with_filter(default_filter());
    let captured = BufferLayer { buffer }.with_filter(default_filter());
    let _ = tracing_subscriber::registry().with(console).with(captured).try_init();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(severity: &'static str, channel: &str, message: &str) -> BufferedLogEvent {
        BufferedLogEvent {
            timestamp: Utc::now(),
            severity,
            channel: channel.into(),
            message: message.into(),
            exception: None,
        }
    }

    #[test]
    fn buffer_filters_by_channel_and_severity_newest_first() {
        let buffer = LogBuffer::new(100);
        buffer.push(event("Information", "findamodel.Models", "one"));
        buffer.push(event("Warning", "findamodel.Hull", "two"));
        buffer.push(event("Error", "findamodel.models", "three"));

        let all = buffer.get(None, None, 10);
        assert_eq!(all.iter().map(|e| e.message.as_str()).collect::<Vec<_>>(), vec!["three", "two", "one"]);
        let models = buffer.get(Some("FINDAMODEL.MODELS"), None, 10);
        assert_eq!(models.len(), 2);
        let warnings = buffer.get(None, severity_rank("warning"), 10);
        assert_eq!(warnings.len(), 2);
        assert_eq!(buffer.channels(), vec!["findamodel.Hull", "findamodel.Models"]);
    }

    #[test]
    fn buffer_is_bounded() {
        let buffer = LogBuffer::new(100);
        for i in 0..250 {
            buffer.push(event("Information", "c", &i.to_string()));
        }
        let entries = buffer.get(None, None, 1000);
        assert_eq!(entries.len(), 100);
        assert_eq!(entries[0].message, "249");
    }
}
