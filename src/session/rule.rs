use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use super::SessionEvent;

type RuleFuture = Pin<Box<dyn Future<Output = bool> + Send + 'static>>;
type RuleHandler = Arc<dyn Fn(Arc<SessionEvent>) -> RuleFuture + Send + Sync + 'static>;

#[derive(Clone)]
pub struct Rule {
    name: Arc<str>,
    handler: RuleHandler,
}

impl Rule {
    pub fn new<F, Fut>(name: impl Into<String>, handler: F) -> Self
    where
        F: Fn(Arc<SessionEvent>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = bool> + Send + 'static,
    {
        Self {
            name: Arc::from(name.into()),
            handler: Arc::new(move |event| Box::pin(handler(event))),
        }
    }

    pub fn always() -> Self {
        Self::new("always", |_event| async move { true })
    }

    pub fn name(&self) -> &str {
        self.name.as_ref()
    }

    pub async fn matches(&self, event: Arc<SessionEvent>) -> bool {
        (self.handler)(event).await
    }

    pub fn and(self, other: Rule) -> Rule {
        let lhs = self.clone();
        let rhs = other.clone();
        Rule::new(format!("{}&&{}", self.name(), other.name()), move |event| {
            let lhs = lhs.clone();
            let rhs = rhs.clone();
            async move { lhs.matches(event.clone()).await && rhs.matches(event).await }
        })
    }

    pub fn or(self, other: Rule) -> Rule {
        let lhs = self.clone();
        let rhs = other.clone();
        Rule::new(format!("{}||{}", self.name(), other.name()), move |event| {
            let lhs = lhs.clone();
            let rhs = rhs.clone();
            async move { lhs.matches(event.clone()).await || rhs.matches(event).await }
        })
    }

    pub fn keywords(words: impl IntoIterator<Item = impl Into<String>>) -> Rule {
        let keywords: Vec<String> = words.into_iter().map(Into::into).collect();
        Rule::new("keywords", move |event| {
            let keywords = keywords.clone();
            async move { keywords.iter().any(|keyword| event.message.contains(keyword)) }
        })
    }
}

