use std::sync::{Arc, RwLock};

use crate::observability::Logger;

use super::{Matcher, MatcherReport, Rule, SessionEvent};

const MODULE_SESSION_ROUTER: &str = "session.router";

#[derive(Debug, Clone, Default)]
pub struct SessionDispatchReport {
    pub matched: usize,
    pub handled: usize,
    pub blocked: bool,
    pub errors: Vec<String>,
}

#[derive(Clone, Default)]
pub struct SessionRouter {
    matchers: Arc<RwLock<Vec<Matcher>>>,
    logger: Option<Logger>,
}

impl SessionRouter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_logger(logger: Logger) -> Self {
        Self {
            logger: Some(logger),
            ..Self::default()
        }
    }

    pub fn set_logger(&mut self, logger: Logger) {
        self.logger = Some(logger);
    }

    pub fn add_matcher(&self, matcher: Matcher) {
        let mut lock = self
            .matchers
            .write()
            .expect("session matcher lock should not be poisoned");
        let index = lock
            .iter()
            .position(|item| item.priority() < matcher.priority())
            .unwrap_or(lock.len());
        lock.insert(index, matcher);
    }

    pub fn on_message<F, Fut>(
        &self,
        name: impl Into<String>,
        rule: Rule,
        priority: i32,
        block: bool,
        handler: F,
    ) where
        F: Fn(Arc<SessionEvent>) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<(), String>> + Send + 'static,
    {
        let mut matcher = Matcher::new(name, rule, priority, block);
        matcher.add_handler(handler);
        self.add_matcher(matcher);
    }

    pub fn on_keywords<F, Fut>(
        &self,
        name: impl Into<String>,
        keywords: impl IntoIterator<Item = impl Into<String>>,
        priority: i32,
        block: bool,
        handler: F,
    ) where
        F: Fn(Arc<SessionEvent>) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<(), String>> + Send + 'static,
    {
        self.on_message(name, Rule::keywords(keywords), priority, block, handler);
    }

    pub fn matcher_count(&self) -> usize {
        self.matchers
            .read()
            .expect("session matcher lock should not be poisoned")
            .len()
    }

    pub async fn dispatch(&self, event: SessionEvent) -> SessionDispatchReport {
        let event = Arc::new(event);
        let matchers = self
            .matchers
            .read()
            .expect("session matcher lock should not be poisoned")
            .clone();

        let mut report = SessionDispatchReport::default();
        for matcher in matchers {
            let matcher_report: MatcherReport = matcher.run(event.clone()).await;
            if !matcher_report.matched {
                continue;
            }

            report.matched += 1;
            report.handled += matcher_report.handled;
            if !matcher_report.errors.is_empty() {
                for err in &matcher_report.errors {
                    if let Some(logger) = &self.logger {
                        logger.warn_in(
                            MODULE_SESSION_ROUTER,
                            format!(
                                "matcher='{}' handler failed for topic='{}': {}",
                                matcher.name(),
                                event.topic,
                                err
                            ),
                        );
                    }
                }
                report.errors.extend(matcher_report.errors);
            }

            if matcher_report.blocked {
                report.blocked = true;
                break;
            }
        }

        report
    }
}
