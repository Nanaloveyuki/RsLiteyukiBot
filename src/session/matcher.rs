use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use super::{Rule, SessionEvent};

type HandlerFuture = Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'static>>;
type EventHandler = Arc<dyn Fn(Arc<SessionEvent>) -> HandlerFuture + Send + Sync + 'static>;

#[derive(Debug, Clone, Default)]
pub struct MatcherReport {
    pub matched: bool,
    pub blocked: bool,
    pub handled: usize,
    pub errors: Vec<String>,
}

#[derive(Clone)]
pub struct Matcher {
    name: Arc<str>,
    rule: Rule,
    priority: i32,
    block: bool,
    handlers: Vec<EventHandler>,
}

impl Matcher {
    pub fn new(name: impl Into<String>, rule: Rule, priority: i32, block: bool) -> Self {
        Self {
            name: Arc::from(name.into()),
            rule,
            priority,
            block,
            handlers: Vec::new(),
        }
    }

    pub fn name(&self) -> &str {
        self.name.as_ref()
    }

    pub fn priority(&self) -> i32 {
        self.priority
    }

    pub fn is_blocking(&self) -> bool {
        self.block
    }

    pub fn add_handler<F, Fut>(&mut self, handler: F)
    where
        F: Fn(Arc<SessionEvent>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), String>> + Send + 'static,
    {
        self.handlers
            .push(Arc::new(move |event| Box::pin(handler(event))));
    }

    pub async fn run(&self, event: Arc<SessionEvent>) -> MatcherReport {
        if !self.rule.matches(event.clone()).await {
            return MatcherReport::default();
        }

        let mut report = MatcherReport {
            matched: true,
            blocked: self.block,
            handled: 0,
            errors: Vec::new(),
        };
        for handler in &self.handlers {
            match handler(event.clone()).await {
                Ok(()) => {
                    report.handled += 1;
                }
                Err(err) => report.errors.push(err),
            }
        }
        report
    }
}
