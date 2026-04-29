use liteyukibot_core::{LlmPromptStore, compose_user_prompt};

#[test]
fn compose_user_prompt_combines_soul_and_user_input() {
    let composed = compose_user_prompt("hello", "you are a coding assistant");
    assert_eq!(composed, "you are a coding assistant\n\nhello");
}

#[test]
fn prompt_store_ensures_default_profile_exists() {
    let store = LlmPromptStore {
        active_profile: "missing".to_string(),
        profiles: vec![],
    }
    .normalized();
    assert_eq!(store.active_profile, "default");
    assert!(
        store
            .profiles
            .iter()
            .any(|profile| profile.name == "default")
    );
}

#[test]
fn prompt_store_upsert_and_switch_active_profile() {
    let mut store = LlmPromptStore::new();
    store
        .upsert_profile("roleplay", "answer in a concise style")
        .expect("upsert should succeed");
    store
        .set_active_profile("roleplay")
        .expect("set active profile should succeed");
    assert_eq!(store.active_profile, "roleplay");
    assert_eq!(store.active_profile().soul, "answer in a concise style");
}
