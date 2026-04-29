#[path = "request_builders/anthropic.rs"]
mod anthropic;
#[path = "request_builders/gemini.rs"]
mod gemini;
#[path = "request_builders/openrouter.rs"]
mod openrouter;
#[path = "request_builders/responses.rs"]
mod responses;
#[path = "request_builders/shared.rs"]
mod shared;

pub(super) use self::anthropic::{build_anthropic_request, extract_anthropic_text};
pub(super) use self::gemini::{build_gemini_request, extract_gemini_text};
// 外部调用
pub(super) use self::openrouter::build_chat_completions_request_payload;
#[allow(unused_imports)]
pub(super) use self::openrouter::{build_chat_completions_messages, build_openrouter_request};
pub(super) use self::responses::build_openai_compatible_request_payload;
pub(super) use self::responses::build_responses_input;
pub(super) use self::shared::{compose_chat_fallback_prompt, effective_messages, truncate_inline};
