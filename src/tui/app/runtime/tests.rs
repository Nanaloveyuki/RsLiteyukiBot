use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_resume_path(name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    path.push(format!("rsliteyuki-runtime-{name}-{nanos}.json"));
    path
}

fn test_app() -> AppState {
    AppState::new(
        RuntimeTarget::Cli,
        "test".to_string(),
        Vec::new(),
        TuiConfig {
            resume_store_path: temp_resume_path("llm-multiline"),
            resume_max_sessions: 8,
            resume_max_size_mib: 4,
        },
    )
}

#[test]
// 必要测试
fn llm_multiline_output_is_split_into_multiple_logs() {
    let mut app = test_app();
    push_llm_response_logs(&mut app, "line-1\nline-2\nline-3".to_string());

    let tail: Vec<(UiLevel, String)> = app
        .logs
        .iter()
        .rev()
        .take(3)
        .map(|log| (log.level, log.message.clone()))
        .collect();
    assert_eq!(
        tail.into_iter().rev().collect::<Vec<_>>(),
        vec![
            (UiLevel::Llm, "line-1".to_string()),
            (UiLevel::Llm, "| line-2".to_string()),
            (UiLevel::Llm, "| line-3".to_string()),
        ]
    );
}

#[test]
// 必要测试
fn llm_multiline_output_preserves_blank_lines() {
    let mut app = test_app();
    push_llm_response_logs(&mut app, "first\n\nthird".to_string());

    let tail: Vec<String> = app
        .logs
        .iter()
        .rev()
        .take(3)
        .map(|log| log.message.clone())
        .collect();
    assert_eq!(
        tail.into_iter().rev().collect::<Vec<_>>(),
        vec![
            "first".to_string(),
            "|  ".to_string(),
            "| third".to_string(),
        ]
    );
}

#[test]
// 必要测试
fn llm_zero_width_output_shows_placeholder() {
    let mut app = test_app();
    push_llm_response_logs(&mut app, "\u{200B}\u{200D}\u{FEFF}".to_string());

    let tail: Vec<String> = app
        .logs
        .iter()
        .rev()
        .take(2)
        .map(|log| log.message.clone())
        .collect();
    assert_eq!(
        tail.into_iter().rev().collect::<Vec<_>>(),
        vec![" ".to_string(), tr("llm.output.empty"),]
    );
}
