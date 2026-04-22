use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, RwLock};

use crate::PluginManifestLoader;
use serde::Serialize;
use serde_json::Value;

const BUNDLED_ZH_CN: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/i18n/core/zh-CN.json"));
const BUNDLED_EN_US: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/i18n/core/en-US.json"));

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) enum AppLocale {
    #[default]
    ZhCn,
    EnUs,
}

impl AppLocale {
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        let normalized = raw.trim().to_ascii_lowercase().replace('_', "-");
        match normalized.as_str() {
            "zh" | "zh-cn" | "zh-hans" | "zh-hans-cn" | "cn" => Some(Self::ZhCn),
            "en" | "en-us" | "en-gb" => Some(Self::EnUs),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::ZhCn => "zh-CN",
            Self::EnUs => "en-US",
        }
    }

    fn all() -> [Self; 2] {
        [Self::ZhCn, Self::EnUs]
    }

    fn bundled_json(self) -> &'static str {
        match self {
            Self::ZhCn => BUNDLED_ZH_CN,
            Self::EnUs => BUNDLED_EN_US,
        }
    }
}

#[derive(Debug, Default)]
struct I18nCatalog {
    translations: HashMap<AppLocale, HashMap<String, String>>,
}

impl I18nCatalog {
    fn bundled_defaults() -> Self {
        let mut catalog = Self::default();
        let mut warnings = Vec::new();
        for locale in AppLocale::all() {
            catalog.merge_json_str(
                locale,
                locale.bundled_json(),
                format!("bundled core {}", locale.as_str()).as_str(),
                &mut warnings,
            );
        }
        debug_assert!(warnings.is_empty(), "bundled i18n json should stay valid");
        catalog
    }

    fn lookup(&self, locale: AppLocale, key: &str) -> Option<&str> {
        self.translations
            .get(&locale)
            .and_then(|entries| entries.get(key).map(String::as_str))
    }

    #[allow(dead_code)]
    fn resolved_messages_for(&self, locale: AppLocale) -> HashMap<String, String> {
        let mut entries = self
            .translations
            .get(&AppLocale::default())
            .cloned()
            .unwrap_or_default();

        if locale != AppLocale::default()
            && let Some(locale_entries) = self.translations.get(&locale)
        {
            for (key, value) in locale_entries {
                entries.insert(key.clone(), value.clone());
            }
        }

        entries
    }

    fn merge_json_str(
        &mut self,
        locale: AppLocale,
        content: &str,
        source: &str,
        warnings: &mut Vec<String>,
    ) {
        match serde_json::from_str::<Value>(content) {
            Ok(value) => {
                let entries = self.translations.entry(locale).or_default();
                flatten_json_value(None, &value, entries, source, warnings);
            }
            Err(err) => warnings.push(format!("i18n: failed to parse {source}: {err}")),
        }
    }

    fn merge_json_file(&mut self, path: &Path, warnings: &mut Vec<String>) {
        let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
            return;
        };
        let Some(locale) = AppLocale::parse(stem) else {
            return;
        };

        match std::fs::read_to_string(path) {
            Ok(content) => {
                let source = path.display().to_string();
                self.merge_json_str(locale, &content, source.as_str(), warnings);
            }
            Err(err) => warnings.push(format!("i18n: failed to read {}: {}", path.display(), err)),
        }
    }

    fn merge_directory(&mut self, dir: &Path, warnings: &mut Vec<String>) {
        if !dir.is_dir() {
            return;
        }

        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(err) => {
                warnings.push(format!(
                    "i18n: failed to read directory {}: {}",
                    dir.display(),
                    err
                ));
                return;
            }
        };

        for entry in entries {
            let Ok(entry) = entry else {
                continue;
            };
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let is_json = path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("json"));
            if is_json {
                self.merge_json_file(path.as_path(), warnings);
            }
        }
    }
}

static CURRENT_LOCALE: LazyLock<RwLock<AppLocale>> =
    LazyLock::new(|| RwLock::new(AppLocale::default()));
static CATALOG: LazyLock<RwLock<I18nCatalog>> =
    LazyLock::new(|| RwLock::new(I18nCatalog::bundled_defaults()));

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize)]
pub(crate) struct I18nSnapshot {
    pub locale: String,
    pub fallback_locale: String,
    pub available_locales: Vec<String>,
    pub messages: HashMap<String, String>,
}

pub(crate) fn current_locale() -> AppLocale {
    *CURRENT_LOCALE
        .read()
        .expect("locale lock should not be poisoned")
}

pub(crate) fn set_current_locale(locale: AppLocale) {
    *CURRENT_LOCALE
        .write()
        .expect("locale lock should not be poisoned") = locale;
}

pub(crate) fn reload_catalog<I, P>(plugin_dirs: I) -> Vec<String>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let mut catalog = I18nCatalog::bundled_defaults();
    let mut warnings = Vec::new();
    for dir in runtime_core_i18n_dirs() {
        catalog.merge_directory(dir.as_path(), &mut warnings);
    }

    let plugin_dirs = plugin_dirs
        .into_iter()
        .map(|dir| dir.as_ref().to_path_buf())
        .collect::<Vec<_>>();
    match PluginManifestLoader::discover_in_dirs(plugin_dirs.iter()) {
        Ok(manifests) => {
            let mut seen_dirs = HashSet::new();
            for manifest in manifests {
                let Some(parent) = manifest.path.parent() else {
                    continue;
                };
                let i18n_dir = parent.join("i18n");
                if seen_dirs.insert(i18n_dir.clone()) {
                    catalog.merge_directory(i18n_dir.as_path(), &mut warnings);
                }
            }
        }
        Err(err) => warnings.push(format!("i18n: plugin manifest discovery failed: {err}")),
    }

    *CATALOG
        .write()
        .expect("i18n catalog lock should not be poisoned") = catalog;
    warnings
}

pub(crate) fn tr(key: &str) -> String {
    tr_for(current_locale(), key)
}

pub(crate) fn trf(key: &str, args: &[(&str, &str)]) -> String {
    trf_for(current_locale(), key, args)
}

#[allow(dead_code)]
pub(crate) fn current_snapshot() -> I18nSnapshot {
    snapshot_for(current_locale())
}

pub(crate) fn tr_for(locale: AppLocale, key: &str) -> String {
    lookup_with_locale(locale, key)
}

pub(crate) fn trf_for(locale: AppLocale, key: &str, args: &[(&str, &str)]) -> String {
    let mut text = tr_for(locale, key);
    for (name, value) in args {
        text = text.replace(format!("{{{name}}}").as_str(), value);
    }
    text
}

fn lookup_with_locale(locale: AppLocale, key: &str) -> String {
    let catalog = CATALOG
        .read()
        .expect("i18n catalog lock should not be poisoned");
    catalog
        .lookup(locale, key)
        .or_else(|| {
            if locale == AppLocale::default() {
                None
            } else {
                catalog.lookup(AppLocale::default(), key)
            }
        })
        .unwrap_or(key)
        .to_string()
}

#[allow(dead_code)]
fn snapshot_for(locale: AppLocale) -> I18nSnapshot {
    let catalog = CATALOG
        .read()
        .expect("i18n catalog lock should not be poisoned");

    I18nSnapshot {
        locale: locale.as_str().to_string(),
        fallback_locale: AppLocale::default().as_str().to_string(),
        available_locales: AppLocale::all()
            .into_iter()
            .map(|entry| entry.as_str().to_string())
            .collect(),
        messages: catalog.resolved_messages_for(locale),
    }
}

fn runtime_core_i18n_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut seen = HashSet::new();

    if let Ok(raw) = std::env::var("LY_I18N_PATH") {
        for path in std::env::split_paths(&raw) {
            push_explicit_i18n_dir_candidates(&mut dirs, &mut seen, path.as_path());
        }
    }

    if let Ok(current_dir) = std::env::current_dir() {
        push_runtime_root_i18n_dir_candidates(&mut dirs, &mut seen, current_dir.as_path());
    }

    if let Ok(exe_path) = std::env::current_exe()
        && let Some(parent) = exe_path.parent()
    {
        push_runtime_root_i18n_dir_candidates(&mut dirs, &mut seen, parent);
    }

    dirs
}

fn push_explicit_i18n_dir_candidates(
    dirs: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
    path: &Path,
) {
    push_unique_path(dirs, seen, path.to_path_buf());
    push_unique_path(dirs, seen, path.join("core"));
    push_unique_path(dirs, seen, path.join("i18n").join("core"));
    push_unique_path(dirs, seen, path.join("resources").join("i18n").join("core"));
}

fn push_runtime_root_i18n_dir_candidates(
    dirs: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
    root: &Path,
) {
    push_unique_path(dirs, seen, root.join("i18n").join("core"));
    push_unique_path(dirs, seen, root.join("resources").join("i18n").join("core"));
}

fn push_unique_path(dirs: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>, path: PathBuf) {
    if seen.insert(path.clone()) {
        dirs.push(path);
    }
}

fn flatten_json_value(
    prefix: Option<&str>,
    value: &Value,
    entries: &mut HashMap<String, String>,
    source: &str,
    warnings: &mut Vec<String>,
) {
    match value {
        Value::Object(map) => {
            for (key, nested) in map {
                let next_key = match prefix {
                    Some(prefix) if !prefix.is_empty() => format!("{prefix}.{key}"),
                    _ => key.clone(),
                };
                flatten_json_value(Some(next_key.as_str()), nested, entries, source, warnings);
            }
        }
        Value::String(text) => {
            if let Some(key) = prefix {
                entries.insert(key.to_string(), text.clone());
            } else {
                warnings.push(format!(
                    "i18n: root string is not allowed in {source}, expected an object"
                ));
            }
        }
        Value::Null => {}
        _ => {
            if let Some(key) = prefix {
                warnings.push(format!(
                    "i18n: key '{key}' in {source} should resolve to a string or object"
                ));
            } else {
                warnings.push(format!(
                    "i18n: root value in {source} should be an object of translation keys"
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
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
    fn locale_parser_supports_chinese_and_english_aliases() {
        assert_eq!(AppLocale::parse("zh-CN"), Some(AppLocale::ZhCn));
        assert_eq!(AppLocale::parse("zh_hans"), Some(AppLocale::ZhCn));
        assert_eq!(AppLocale::parse("en"), Some(AppLocale::EnUs));
        assert_eq!(AppLocale::parse("EN_us"), Some(AppLocale::EnUs));
        assert_eq!(AppLocale::parse("ja-JP"), None);
    }

    #[test]
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
    fn runtime_roots_cover_repo_and_installer_i18n_locations() {
        let mut dirs = Vec::new();
        let mut seen = HashSet::new();
        let root = PathBuf::from("C:/liteyuki");
        push_runtime_root_i18n_dir_candidates(&mut dirs, &mut seen, root.as_path());

        assert!(dirs.contains(&root.join("i18n").join("core")));
        assert!(dirs.contains(&root.join("resources").join("i18n").join("core")));
    }

    #[test]
    fn snapshot_for_uses_requested_locale_with_default_fallback() {
        let snapshot = snapshot_for(AppLocale::EnUs);

        assert_eq!(snapshot.locale, "en-US");
        assert_eq!(snapshot.fallback_locale, "zh-CN");
        assert_eq!(
            snapshot.messages.get("command.spec.help.summary").map(String::as_str),
            Some("Show commands available in the current scope")
        );
        assert_eq!(
            snapshot.messages.get("web.nav.overview").map(String::as_str),
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
}
