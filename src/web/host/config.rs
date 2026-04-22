use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct ThemeConfigDoc {
    dark: HashMap<String, String>,
    light: HashMap<String, String>,
    #[serde(rename = "fontMode")]
    font_mode: String,
}

impl Default for ThemeConfigDoc {
    fn default() -> Self {
        Self {
            dark: HashMap::new(),
            light: HashMap::new(),
            font_mode: "aacute".to_string(),
        }
    }
}

pub(super) fn state_path(path: &str) -> PathBuf {
    PathBuf::from(path)
}

fn read_json_file<T>(path: &Path) -> T
where
    T: DeserializeOwned + Default,
{
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<T>(&raw).ok())
        .unwrap_or_default()
}

fn write_json_file<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
    }
    let body = serde_json::to_string_pretty(value)
        .map_err(|err| format!("failed to serialize {}: {err}", path.display()))?;
    fs::write(path, body).map_err(|err| format!("failed to write {}: {err}", path.display()))
}

pub(super) fn load_webui_server_config(port: u16) -> NapCatWebUIConfig {
    let mut config = read_json_file::<NapCatWebUIConfig>(state_path(WEBUI_SERVER_CONFIG_FILE).as_path());
    if config.host.trim().is_empty() {
        config.host = "0.0.0.0".to_string();
    }
    if config.port == 0 {
        config.port = port;
    }
    if config.access_control_mode.trim().is_empty() {
        config.access_control_mode = "none".to_string();
    }
    config
}

pub(super) fn save_webui_server_config(config: &NapCatWebUIConfig) -> Result<(), String> {
    write_json_file(state_path(WEBUI_SERVER_CONFIG_FILE).as_path(), config)
}

pub(super) fn load_theme_config() -> ThemeConfigDoc {
    read_json_file(state_path(THEME_CONFIG_FILE).as_path())
}

pub(super) fn save_theme_config(config: &ThemeConfigDoc) -> Result<(), String> {
    write_json_file(state_path(THEME_CONFIG_FILE).as_path(), config)
}

pub(super) fn load_onebot_config() -> OneBotConfig {
    read_json_file(state_path(ONEBOT_CONFIG_FILE).as_path())
}

pub(super) fn save_onebot_config(config: &OneBotConfig) -> Result<(), String> {
    write_json_file(state_path(ONEBOT_CONFIG_FILE).as_path(), config)
}

pub(super) fn load_napcat_config(use_uin_config: bool) -> NapCatConfig {
    let path = if use_uin_config {
        state_path(NAPCAT_UIN_CONFIG_FILE)
    } else {
        state_path(NAPCAT_CONFIG_FILE)
    };
    read_json_file(path.as_path())
}

pub(super) fn save_napcat_config(
    use_uin_config: bool,
    config: &NapCatConfig,
) -> Result<(), String> {
    let path = if use_uin_config {
        state_path(NAPCAT_UIN_CONFIG_FILE)
    } else {
        state_path(NAPCAT_CONFIG_FILE)
    };
    write_json_file(path.as_path(), config)
}

pub(super) fn render_theme_css(config: &ThemeConfigDoc) -> String {
    let mut css = String::new();
    css.push_str("@font-face{font-family:'JetBrains Mono';src:url('/webui/fonts/JetBrainsMono.ttf') format('truetype');font-display:swap;}");
    css.push_str("@font-face{font-family:'JetBrains Mono';src:url('/webui/fonts/JetBrainsMono-Italic.ttf') format('truetype');font-style:italic;font-display:swap;}");

    match config.font_mode.as_str() {
        "aacute" => {
            css.push_str("@font-face{font-family:'Aa偷吃可爱长大的';src:url('/webui/fonts/AaCute.woff') format('woff');font-display:swap;}");
            css.push_str(":root{--font-family-base:'Aa偷吃可爱长大的',var(--font-family-fallbacks);--font-family-mono:'Aa偷吃可爱长大的',var(--font-family-fallbacks);}");
        }
        "custom" => {
            if state_path(CUSTOM_FONT_FILE).is_file() {
                css.push_str("@font-face{font-family:'CustomFont';src:url('/webui/fonts/CustomFont.woff') format('woff');font-display:swap;}");
                css.push_str(":root{--font-family-base:'CustomFont',var(--font-family-fallbacks);--font-family-mono:'CustomFont',var(--font-family-fallbacks);}");
            }
        }
        _ => {
            css.push_str(":root{--font-family-base:var(--font-family-fallbacks);--font-family-mono:'JetBrains Mono',ui-monospace,SFMono-Regular,Menlo,Monaco,Consolas,'Liberation Mono','Courier New',monospace;}");
        }
    }

    css.push_str(":root,.light,[data-theme=\"light\"]{");
    for (key, value) in &config.light {
        css.push_str(key);
        css.push(':');
        css.push_str(value);
        css.push(';');
    }
    css.push('}');
    css.push_str(".dark,[data-theme=\"dark\"]{");
    for (key, value) in &config.dark {
        css.push_str(key);
        css.push(':');
        css.push_str(value);
        css.push(';');
    }
    css.push('}');
    css
}

pub(super) fn built_in_public_font(path: &str) -> Option<WebHostAsset> {
    let font_name = path.strip_prefix("/webui/fonts/")?;
    if font_name.eq_ignore_ascii_case("CustomFont.woff") {
        return read_asset_file(state_path(CUSTOM_FONT_FILE));
    }

    read_asset_file(state_path(PUBLIC_FONT_DIR).join(font_name))
}
