use super::{LlmEventSink, LlmStreamEvent};

pub(super) struct EventDispatcher<'a> {
    sink: Option<&'a mut dyn LlmEventSink>,
}

impl<'a> EventDispatcher<'a> {
    pub(super) fn new(sink: Option<&'a mut dyn LlmEventSink>) -> Self {
        Self { sink }
    }

    pub(super) fn emit(&mut self, event: LlmStreamEvent) {
        if let Some(sink) = self.sink.as_mut() {
            (*sink).on_event(event);
        }
    }

    pub(super) fn has_sink(&self) -> bool {
        self.sink.is_some()
    }
}
