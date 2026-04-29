use super::*;

pub(crate) fn load_app_config_with_warnings(emit_stderr: bool) -> (AppConfigDoc, Vec<String>) {
    let Some(path) = resolve_app_config_path() else {
        return (AppConfigDoc::default(), Vec::new());
    };

    match load_app_config_from_path(&path) {
        Ok(doc) => {
            let warnings = validate_app_config(&doc);
            if emit_stderr {
                for warning in &warnings {
                    eprintln!(
                        "{}",
                        localized_doc_textf(
                            &doc,
                            "config.stderr.warning",
                            &[
                                ("source", path.display().to_string().as_str()),
                                ("warning", warning.as_str()),
                            ],
                        )
                    );
                }
            }
            (doc, warnings)
        }
        Err(err) => {
            let message = trf(
                "config.stderr.load_failed",
                &[
                    ("path", path.display().to_string().as_str()),
                    ("err", err.to_string().as_str()),
                ],
            );
            if emit_stderr {
                eprintln!("{message}");
            }
            (AppConfigDoc::default(), vec![message])
        }
    }
}

pub(crate) fn resolve_app_config_path() -> Option<PathBuf> {
    resolve_existing_app_config_path()
}

// 外部调用
#[allow(dead_code)]
pub fn resolve_desktop_close_behavior() -> DesktopCloseBehavior {
    let (doc, _) = load_app_config_with_warnings(false);
    desktop_close_behavior_from_doc(&doc)
}

// 外部调用
#[allow(dead_code)]
pub fn persist_desktop_close_to_tray_preference(
    close_to_tray: bool,
) -> Result<DesktopCloseBehavior, String> {
    ensure_default_config_files().map_err(|err| err.to_string())?;
    let path = resolve_app_config_path().unwrap_or_else(resolve_default_app_config_path);
    crate::config_edit::persist_desktop_close_to_tray(path.as_path(), close_to_tray)?;
    Ok(DesktopCloseBehavior {
        close_to_tray,
        configured: true,
    })
}

fn desktop_close_behavior_from_doc(doc: &AppConfigDoc) -> DesktopCloseBehavior {
    match doc
        .desktop
        .as_ref()
        .and_then(|section| section.close_to_tray)
    {
        Some(close_to_tray) => DesktopCloseBehavior {
            close_to_tray,
            configured: true,
        },
        None => DesktopCloseBehavior {
            close_to_tray: true,
            configured: false,
        },
    }
}

pub(crate) fn ensure_default_config_files() -> Result<(), Box<dyn std::error::Error>> {
    if let Ok(path) = std::env::var("LY_CONFIG_PATH") {
        let path = PathBuf::from(path);
        write_default_config_if_missing(path.as_path())?;
        return Ok(());
    }

    if let Some(user_path) = resolve_existing_user_app_config_path() {
        replace_default_user_app_config_with_legacy(user_path.as_path())
            .map_err(|err| -> Box<dyn std::error::Error> { err.into() })?;
        return Ok(());
    }

    migrate_legacy_app_config_to_user_dir()
        .map_err(|err| -> Box<dyn std::error::Error> { err.into() })?;

    if resolve_existing_user_app_config_path().is_none() {
        let path = resolve_default_app_config_path();
        write_default_config_if_missing(path.as_path())?;
    }

    Ok(())
}

fn replace_default_user_app_config_with_legacy(user_path: &Path) -> Result<(), String> {
    let Some(legacy_path) = resolve_existing_legacy_app_config_path() else {
        return Ok(());
    };
    if legacy_path == user_path {
        return Ok(());
    }

    let current = std::fs::read_to_string(user_path).map_err(|err| {
        format!(
            "failed to read user app config {}: {err}",
            user_path.display()
        )
    })?;
    let template = default_config_template(user_path);
    if normalize_template_text(current.as_str()) != normalize_template_text(template.as_str()) {
        return Ok(());
    }

    replace_config_file(legacy_path.as_path(), user_path)
}

fn normalize_template_text(raw: &str) -> &str {
    raw.trim()
}

pub(crate) fn write_default_config_if_missing(
    path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }

    let template = default_config_template(path);
    std::fs::write(path, template)?;
    eprintln!(
        "{}",
        trf(
            "config.stderr.created_default",
            &[("path", path.display().to_string().as_str())],
        )
    );
    Ok(())
}

fn default_config_template(path: &Path) -> String {
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());
    match ext.as_deref() {
        Some("toml") => super::DEFAULT_TOML_CONFIG_TEMPLATE.to_string(),
        _ => super::DEFAULT_YAML_CONFIG_TEMPLATE.to_string(),
    }
}

pub(crate) fn load_app_config_from_path(
    path: &Path,
) -> Result<AppConfigDoc, Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(path)?;
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());

    match ext.as_deref() {
        Some("yaml") | Some("yml") => Ok(serde_yaml::from_str::<AppConfigDoc>(&content)?),
        Some("toml") => Ok(toml::from_str::<AppConfigDoc>(&content)?),
        _ => Err(format!("unsupported config extension for {}", path.display()).into()),
    }
}
