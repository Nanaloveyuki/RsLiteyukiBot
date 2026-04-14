#[derive(Debug, Clone)]
pub enum AdapterError {
    Config(String),
    Http(String),
    WebSocket(String),
    Sse(String),
    Serialize(String),
    Io(String),
}

impl std::fmt::Display for AdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Config(message) => write!(f, "adapter config error: {}", message),
            Self::Http(message) => write!(f, "adapter http error: {}", message),
            Self::WebSocket(message) => write!(f, "adapter websocket error: {}", message),
            Self::Sse(message) => write!(f, "adapter sse error: {}", message),
            Self::Serialize(message) => write!(f, "adapter serialization error: {}", message),
            Self::Io(message) => write!(f, "adapter io error: {}", message),
        }
    }
}

impl std::error::Error for AdapterError {}
