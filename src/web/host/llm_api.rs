#[path = "llm_api/chat_exec.rs"]
mod chat_exec;
#[path = "llm_api/frontend_chat.rs"]
mod frontend_chat;
#[path = "llm_api/manager.rs"]
mod manager;
#[path = "llm_api/manager_actions.rs"]
mod manager_actions;
#[path = "llm_api/prompt_profiles.rs"]
mod prompt_profiles;
#[path = "llm_api/provider_catalog.rs"]
mod provider_catalog;
#[path = "llm_api/provider_serialization.rs"]
mod provider_serialization;
#[path = "llm_api/provider_state.rs"]
mod provider_state;
#[path = "llm_api/request_builders.rs"]
mod request_builders;
#[path = "llm_api/transport.rs"]
mod transport;
#[path = "llm_api/types.rs"]
mod types;

use self::chat_exec::*;
use self::frontend_chat::*;
use self::manager::*;
use self::manager_actions::*;
use self::prompt_profiles::*;
use self::provider_catalog::*;
use self::provider_serialization::*;
use self::provider_state::*;
use self::request_builders::*;
use self::transport::*;
use self::types::*;

pub(super) use super::parse_json_body;

use super::{WebHostService, napcat_err, napcat_ok, napcat_response};

pub(super) fn route_llm_api(
    service: &WebHostService,
    method: &str,
    api_path: &str,
    request: &[u8],
    is_head: bool,
) -> Option<Vec<u8>> {
    if api_path == "/LLM/GetSettings" {
        let body = match llm_settings_payload() {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/LLM/Chat" {
        if !method.eq_ignore_ascii_case("POST") {
            let body = napcat_err(-1, "LLM/Chat only accepts POST");
            return Some(napcat_response(body, is_head));
        }

        let body = match llm_chat_payload(service, request) {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/LLM/GetManagerState" {
        let body = match llm_manager_state_payload() {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/LLM/SaveManagerState" {
        if let Some(response) = reject_non_post_method(method, "LLM/SaveManagerState", is_head) {
            return Some(response);
        }

        let body = match save_llm_manager_state(request) {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/LLM/UpdateEnabled" {
        if let Some(response) = reject_non_post_method(method, "LLM/UpdateEnabled", is_head) {
            return Some(response);
        }

        let body = match update_llm_enabled_state(request) {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/LLM/FetchModels" {
        if let Some(response) = reject_non_post_method(method, "LLM/FetchModels", is_head) {
            return Some(response);
        }

        let body = match llm_fetch_models_payload(request) {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/LLM/TestModels" {
        if let Some(response) = reject_non_post_method(method, "LLM/TestModels", is_head) {
            return Some(response);
        }

        let body = match llm_test_models_payload(request) {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/LLM/PreviewRequest" {
        if let Some(response) = reject_non_post_method(method, "LLM/PreviewRequest", is_head) {
            return Some(response);
        }

        let body = match llm_preview_request_payload(request) {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/LLM/PromptProfiles" {
        if !method.eq_ignore_ascii_case("GET") {
            let body = napcat_err(-1, "LLM/PromptProfiles only accepts GET");
            return Some(napcat_response(body, is_head));
        }

        let body = match llm_prompt_profiles_payload() {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/LLM/PromptProfiles/Save" {
        if let Some(response) = reject_non_post_method(method, "LLM/PromptProfiles/Save", is_head) {
            return Some(response);
        }

        let body = match save_llm_prompt_profile(request) {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/LLM/PromptProfiles/Delete" {
        if let Some(response) = reject_non_post_method(method, "LLM/PromptProfiles/Delete", is_head)
        {
            return Some(response);
        }

        let body = match delete_llm_prompt_profile(request) {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/LLM/PromptProfiles/Use" {
        if let Some(response) = reject_non_post_method(method, "LLM/PromptProfiles/Use", is_head) {
            return Some(response);
        }

        let body = match use_llm_prompt_profile(request) {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/LLM/PromptProfiles/Preview" {
        if let Some(response) =
            reject_non_post_method(method, "LLM/PromptProfiles/Preview", is_head)
        {
            return Some(response);
        }

        let body = match preview_llm_prompt_profile(request) {
            Ok(payload) => napcat_ok(&payload),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    None
}

fn reject_non_post_method(method: &str, route_name: &str, is_head: bool) -> Option<Vec<u8>> {
    if method.eq_ignore_ascii_case("POST") {
        return None;
    }

    let body = napcat_err(-1, format!("{route_name} only accepts POST").as_str());
    Some(napcat_response(body, is_head))
}

#[cfg(test)]
#[path = "llm_api/tests.rs"]
mod tests;
