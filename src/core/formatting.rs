use std::fmt::Write;

use super::runtime::BotEvent;

pub trait EventTextFormatter: Send + Sync {
    fn format_event(&self, event: &BotEvent) -> String;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultEventTextFormatter;

impl EventTextFormatter for DefaultEventTextFormatter {
    fn format_event(&self, event: &BotEvent) -> String {
        format_event_text(event)
    }
}

pub fn format_event_text(event: &BotEvent) -> String {
    let payload = event.payload.to_string();
    let mut text = String::with_capacity(32 + event.topic.len() + payload.len());
    let _ = write!(
        &mut text,
        "event id={} topic={} payload={}",
        event.id, event.topic, payload
    );
    text
}

pub fn format_event_with(formatter: &dyn EventTextFormatter, event: &BotEvent) -> String {
    formatter.format_event(event)
}
