pub mod bot;
pub mod formatting;
pub mod lifecycle;
pub mod platform;
pub mod process_manager;
pub mod runtime;

pub use bot::{BotBootstrapContext, LiteyukiBot, LiteyukiBotBuilder, LiteyukiBotError};
pub use formatting::{
    DefaultEventTextFormatter, EventTextFormatter, format_event_text, format_event_with,
};
pub use lifecycle::{
    HookFailure, HookFilter, LifecycleContext, LifecycleExecutionError, LifecycleFailurePolicy,
    LifecyclePhase, Lifespan, RuntimeCapabilities, RuntimeFlavor,
};
pub use platform::RuntimeTarget;
pub use process_manager::{
    ManagedProcessRunner, ManagedProcessSpec, ProcessManager, ProcessManagerError, RestartPolicy,
};
pub use runtime::{BotEvent, BotHandle, BotRuntime, BotRuntimeConfig};
