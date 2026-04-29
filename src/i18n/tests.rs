use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_plugin_dir(name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    path.push(format!("rsliteyuki-i18n-plugin-{name}-{nanos}"));
    path
}

#[test]
// 必要测试
fn locale_parser_supports_chinese_and_english_aliases() {
    assert_eq!(AppLocale::parse("zh-CN"), Some(AppLocale::ZhCn));
    assert_eq!(AppLocale::parse("zh_hans"), Some(AppLocale::ZhCn));
    assert_eq!(AppLocale::parse("en"), Some(AppLocale::EnUs));
    assert_eq!(AppLocale::parse("EN_us"), Some(AppLocale::EnUs));
    assert_eq!(AppLocale::parse("ja-JP"), None);
}

#[test]
// 必要测试
fn reload_catalog_merges_plugin_language_packs() {
    let plugin_dir = temp_plugin_dir("merge");
    std::fs::create_dir_all(plugin_dir.join("i18n"))
        .expect("plugin i18n dir should be created for test");
    std::fs::write(
        plugin_dir.join("plugin.json"),
        r#"{
  "id": "demo-i18n",
  "name": "Demo I18n Plugin",
  "runtime": {
    "kind": "python",
    "entrypoint": "demo:bootstrap"
  }
}"#,
    )
    .expect("plugin manifest should be written");
    std::fs::write(
        plugin_dir.join("i18n").join("en-US.json"),
        r#"{
  "plugin": {
    "demo": {
      "title": "Demo plugin"
    }
  }
}"#,
    )
    .expect("plugin i18n file should be written");

    let warnings = reload_catalog([plugin_dir.as_path()]);
    assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    assert_eq!(
        lookup_with_locale(AppLocale::EnUs, "plugin.demo.title"),
        "Demo plugin"
    );
    assert_eq!(
        lookup_with_locale(AppLocale::EnUs, "command.spec.help.summary"),
        "Show commands available in the current scope"
    );

    let _ = std::fs::remove_dir_all(&plugin_dir);
    let _ = reload_catalog(std::iter::empty::<&Path>());
}

#[test]
// 必要测试
fn explicit_i18n_paths_support_directories_and_common_install_layouts() {
    let mut dirs = Vec::new();
    let mut seen = HashSet::new();
    let root = PathBuf::from("C:/liteyuki");
    push_explicit_i18n_dir_candidates(&mut dirs, &mut seen, root.as_path());

    assert!(dirs.contains(&root));
    assert!(dirs.contains(&root.join("core")));
    assert!(dirs.contains(&root.join("i18n").join("core")));
    assert!(dirs.contains(&root.join("resources").join("i18n").join("core")));
}

#[test]
// 必要测试
fn runtime_roots_cover_repo_and_installer_i18n_locations() {
    let mut dirs = Vec::new();
    let mut seen = HashSet::new();
    let root = PathBuf::from("C:/liteyuki");
    push_runtime_root_i18n_dir_candidates(&mut dirs, &mut seen, root.as_path());

    assert!(dirs.contains(&root.join("i18n").join("core")));
    assert!(dirs.contains(&root.join("resources").join("i18n").join("core")));
}

#[test]
// 必要测试
fn snapshot_for_uses_requested_locale_with_default_fallback() {
    let snapshot = snapshot_for(AppLocale::EnUs);

    assert_eq!(snapshot.locale, "en-US");
    assert_eq!(snapshot.fallback_locale, "zh-CN");
    assert_eq!(
        snapshot
            .messages
            .get("command.spec.help.summary")
            .map(String::as_str),
        Some("Show commands available in the current scope")
    );
    assert_eq!(
        snapshot
            .messages
            .get("web.nav.overview")
            .map(String::as_str),
        Some("Overview")
    );
    assert_eq!(
        snapshot
            .messages
            .get("web.runtime.status.running")
            .map(String::as_str),
        Some("Running")
    );
    assert!(
        snapshot.messages.contains_key("tui.brand"),
        "resolved snapshot should include default catalog entries"
    );
}
