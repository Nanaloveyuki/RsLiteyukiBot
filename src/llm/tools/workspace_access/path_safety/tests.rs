use super::sanitize_relative_path;

#[test]
// 必要测试
fn sanitize_relative_path_rejects_parent_dirs() {
    let error = sanitize_relative_path("../secret").expect_err("path should be rejected");
    assert!(error.contains("parent-directory"));
}
