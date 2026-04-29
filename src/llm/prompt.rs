use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

const DEFAULT_PROMPT_PROFILE: &str = "default";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LlmPromptProfile {
    pub name: String,
    #[serde(default)]
    pub soul: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LlmPromptStore {
    #[serde(default = "default_profile_name")]
    pub active_profile: String,
    #[serde(default)]
    pub profiles: Vec<LlmPromptProfile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmPromptPreview {
    pub system_prompt: String,
    pub composed_user_prompt: String,
    pub combined_prompt: String,
}

impl Default for LlmPromptStore {
    fn default() -> Self {
        Self::new()
    }
}

impl LlmPromptStore {
    pub fn new() -> Self {
        Self {
            active_profile: DEFAULT_PROMPT_PROFILE.to_string(),
            profiles: vec![LlmPromptProfile {
                name: DEFAULT_PROMPT_PROFILE.to_string(),
                soul: String::new(),
            }],
        }
    }

    pub fn load_or_default_from_path(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::new());
        }
        Self::load_from_path(path)
    }

    pub fn load_from_path(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|err| format!("failed to read prompt store {}: {err}", path.display()))?;
        let store = serde_json::from_str::<LlmPromptStore>(&content)
            .map_err(|err| format!("invalid prompt store json {}: {err}", path.display()))?;
        Ok(store.normalized())
    }

    pub fn save_to_path(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|err| {
                format!(
                    "failed to create prompt store directory {}: {err}",
                    parent.display()
                )
            })?;
        }
        let content = serde_json::to_string_pretty(&self.normalized())
            .map_err(|err| format!("failed to serialize prompt store: {err}"))?;
        std::fs::write(path, content)
            .map_err(|err| format!("failed to write prompt store {}: {err}", path.display()))?;
        Ok(())
    }

    pub fn set_active_profile(&mut self, profile_name: &str) -> Result<(), String> {
        let profile_name = normalize_profile_name(profile_name)
            .ok_or_else(|| "profile name should not be empty".to_string())?;
        if !self
            .profiles
            .iter()
            .any(|profile| profile.name == profile_name)
        {
            return Err(format!("prompt profile not found: {profile_name}"));
        }
        self.active_profile = profile_name;
        Ok(())
    }

    pub fn upsert_profile(&mut self, profile_name: &str, soul: &str) -> Result<(), String> {
        let profile_name = normalize_profile_name(profile_name)
            .ok_or_else(|| "profile name should not be empty".to_string())?;
        let soul = soul.trim().to_string();
        if let Some(profile) = self
            .profiles
            .iter_mut()
            .find(|profile| profile.name == profile_name)
        {
            profile.soul = soul;
        } else {
            self.profiles.push(LlmPromptProfile {
                name: profile_name,
                soul,
            });
        }
        self.ensure_store_invariants();
        Ok(())
    }

    pub fn remove_profile(&mut self, profile_name: &str) -> Result<(), String> {
        let profile_name = normalize_profile_name(profile_name)
            .ok_or_else(|| "profile name should not be empty".to_string())?;
        if profile_name == DEFAULT_PROMPT_PROFILE {
            return Err("default profile cannot be removed".to_string());
        }

        let before = self.profiles.len();
        self.profiles.retain(|profile| profile.name != profile_name);
        if self.profiles.len() == before {
            return Err(format!("prompt profile not found: {profile_name}"));
        }

        if self.active_profile == profile_name {
            self.active_profile = DEFAULT_PROMPT_PROFILE.to_string();
        }
        self.ensure_store_invariants();
        Ok(())
    }

    pub fn profile_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .normalized()
            .profiles
            .into_iter()
            .map(|profile| profile.name)
            .collect();
        names.sort();
        names
    }

    pub fn active_profile(&self) -> LlmPromptProfile {
        let store = self.normalized();
        store
            .profiles
            .iter()
            .find(|profile| profile.name == store.active_profile)
            .cloned()
            .or_else(|| store.profiles.first().cloned())
            .expect("normalized prompt store should contain at least one profile")
    }

    pub fn normalized(&self) -> Self {
        let mut store = self.clone();
        store.ensure_store_invariants();
        store
    }

    fn ensure_store_invariants(&mut self) {
        let mut seen = HashSet::new();
        let mut normalized_profiles = Vec::new();

        for profile in self.profiles.drain(..) {
            let Some(name) = normalize_profile_name(profile.name.as_str()) else {
                continue;
            };
            if !seen.insert(name.clone()) {
                continue;
            }
            normalized_profiles.push(LlmPromptProfile {
                name,
                soul: profile.soul.trim().to_string(),
            });
        }

        if !seen.contains(DEFAULT_PROMPT_PROFILE) {
            normalized_profiles.push(LlmPromptProfile {
                name: DEFAULT_PROMPT_PROFILE.to_string(),
                soul: String::new(),
            });
            seen.insert(DEFAULT_PROMPT_PROFILE.to_string());
        }

        let active_profile = normalize_profile_name(self.active_profile.as_str())
            .unwrap_or_else(|| DEFAULT_PROMPT_PROFILE.to_string());
        if seen.contains(active_profile.as_str()) {
            self.active_profile = active_profile;
        } else {
            self.active_profile = DEFAULT_PROMPT_PROFILE.to_string();
        }

        self.profiles = normalized_profiles;
    }
}

pub fn compose_user_prompt(user_prompt: &str, soul: &str) -> String {
    let user_prompt = user_prompt.trim();
    let soul = soul.trim();

    match (soul.is_empty(), user_prompt.is_empty()) {
        (true, true) => String::new(),
        (true, false) => user_prompt.to_string(),
        (false, true) => soul.to_string(),
        (false, false) => format!("{soul}\n\n{user_prompt}"),
    }
}

pub fn build_prompt_preview(
    system_prompt: Option<&str>,
    user_prompt: &str,
    soul: &str,
) -> LlmPromptPreview {
    let system_prompt = system_prompt.unwrap_or_default().trim().to_string();
    let composed_user_prompt = compose_user_prompt(user_prompt, soul);
    let combined_prompt = if system_prompt.is_empty() {
        format!("USER:\n{}", composed_user_prompt)
    } else {
        format!(
            "SYSTEM:\n{}\n\nUSER:\n{}",
            system_prompt, composed_user_prompt
        )
    };

    LlmPromptPreview {
        system_prompt,
        composed_user_prompt,
        combined_prompt,
    }
}

fn default_profile_name() -> String {
    DEFAULT_PROMPT_PROFILE.to_string()
}

fn normalize_profile_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
