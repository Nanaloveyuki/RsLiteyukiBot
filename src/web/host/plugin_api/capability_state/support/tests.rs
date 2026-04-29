use super::*;

#[test]
// 必要测试
fn cron_capability_support_marks_disabled_jobs_as_disabled() {
    let support = build_cron_capability_support(true, false, false, true);
    assert_eq!(support.status, "disabled");
    assert!(!support.active);
    assert!(!support.executable);
}

#[test]
// 必要测试
fn cron_capability_support_marks_enabled_non_executable_jobs_as_registered_only() {
    let support = build_cron_capability_support(true, true, false, true);
    assert_eq!(support.status, "registered_only");
    assert!(support.active);
    assert!(!support.executable);
}
