use super::*;

#[test]
// 必要测试
fn describe_optional_path_for_log_formats_missing_path() {
    assert_eq!(describe_optional_path_for_log(None), "<not found>");
}

#[test]
// 必要测试
fn describe_optional_path_for_log_formats_existing_path() {
    let path = PathBuf::from("C:/liteyuki/config.yaml");
    let expected = path.display().to_string();
    assert_eq!(describe_optional_path_for_log(Some(path)), expected);
}
