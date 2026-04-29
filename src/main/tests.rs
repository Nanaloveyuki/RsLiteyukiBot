use super::*;

#[test]
// 必要测试
fn builtin_plugin_candidates_cover_runtime_and_dev_layouts() {
    let mut dirs = Vec::new();
    let mut seen = HashSet::new();
    let root = PathBuf::from("C:/liteyuki");
    push_runtime_plugin_dir_candidates(&mut dirs, &mut seen, root.as_path(), true);

    assert!(dirs.contains(&root.join("builtin_plugin")));
    assert!(dirs.contains(&root.join("resources").join("builtin_plugin")));
    assert!(dirs.contains(&root.join("src").join("builtin_plugin")));
}

#[test]
// 必要测试
fn explicit_plugin_paths_support_directories_and_install_roots() {
    let mut dirs = Vec::new();
    let mut seen = HashSet::new();
    let root = PathBuf::from("C:/liteyuki");
    push_explicit_plugin_dir_candidates(&mut dirs, &mut seen, root.as_path());

    assert!(dirs.contains(&root));
    assert!(dirs.contains(&root.join("builtin_plugin")));
    assert!(dirs.contains(&root.join("resources").join("builtin_plugin")));
    assert!(dirs.contains(&root.join("src").join("builtin_plugin")));
}
