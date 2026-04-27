use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use liteyukibot_core::web::runtime::EmbeddedWebRuntime;
use liteyukibot_core::web_ui::APP_SHELL_WINDOW_ICON_ICO;
use liteyukibot_core::{
    LogLevel, RuntimeTarget, emit_console_log, persist_desktop_close_to_tray_preference,
    resolve_desktop_close_behavior,
};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager, Runtime, State, WindowEvent};

const MAIN_WINDOW_LABEL: &str = "main";
const TRAY_ICON_ID: &str = "main-tray";
const TRAY_SHOW_MENU_ID: &str = "tray-show";
const TRAY_QUIT_MENU_ID: &str = "tray-quit";
const RUNTIME_API_BASE_GLOBAL: &str = "__LITEYUKI_RUNTIME_API_BASE__";
const DESKTOP_GLOBAL: &str = "__LITEYUKI_DESKTOP__";
/// JS global injected into the Tauri webview that carries the local auto-login
/// token. The frontend reads this on startup and skips the login page.
const LOCAL_TOKEN_GLOBAL: &str = "__LITEYUKI_LOCAL_TOKEN__";

struct DesktopCloseState {
    allow_app_exit: AtomicBool,
    prompt_pending: AtomicBool,
}

impl DesktopCloseState {
    fn new() -> Self {
        Self {
            allow_app_exit: AtomicBool::new(false),
            prompt_pending: AtomicBool::new(false),
        }
    }
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CloseRequestPayload {
    close_to_tray_default: bool,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let (app_host, server, listener) =
        tauri::async_runtime::block_on(EmbeddedWebRuntime::start(RuntimeTarget::Tauri2))
            .unwrap_or_else(|err| panic!("failed to bootstrap shared HTTP runtime: {err}"))
            .into_parts();
    let server_for_task = server.clone();

    tauri::async_runtime::spawn(async move {
        if let Err(err) = server_for_task.serve(listener).await {
            emit_console_log(
                LogLevel::Warn,
                "tauri.web",
                format!("shared HTTP host stopped unexpectedly: {err}"),
            );
        }
    });

    emit_console_log(
        LogLevel::Info,
        "tauri.web",
        format!(
            "shared HTTP host listening on {} | desktop UI {} | external UI {}",
            server.bind_addr(),
            server.desktop_webui_url(),
            server.external_webui_url_hint(),
        ),
    );
    let close_state = Arc::new(DesktopCloseState::new());
    let close_state_for_tray = close_state.clone();
    let close_state_for_window = close_state.clone();
    let runtime_api_base_script = build_runtime_api_base_init_script(server.desktop_url().as_str());
    let local_token = server.local_token();
    let local_token_script = build_local_token_init_script(local_token.as_str());
    let desktop_close_script = build_desktop_close_init_script();

    tauri::Builder::default()
        .append_invoke_initialization_script(runtime_api_base_script)
        .append_invoke_initialization_script(local_token_script)
        .append_invoke_initialization_script(desktop_close_script)
        .invoke_handler(tauri::generate_handler![
            liteyuki_close_to_background,
            liteyuki_exit_app,
            liteyuki_cancel_close
        ])
        .manage(app_host.clone())
        .manage(server)
        .manage(close_state)
        .setup(move |app| setup_system_tray(app, close_state_for_tray.clone()))
        .on_window_event(move |window, event| {
            handle_main_window_event(window, event, close_state_for_window.as_ref());
        })
        .run(tauri::generate_context!())
        .expect("error while running Liteyuki Tauri shell");

    if let Err(err) = tauri::async_runtime::block_on(app_host.shutdown()) {
        emit_console_log(
            LogLevel::Warn,
            "tauri.web",
            format!("failed to shutdown embedded app host cleanly: {err}"),
        );
    }
}

fn setup_system_tray<R: Runtime>(
    app: &mut App<R>,
    close_state: Arc<DesktopCloseState>,
) -> Result<(), Box<dyn std::error::Error>> {
    let show_item = MenuItem::with_id(app, TRAY_SHOW_MENU_ID, "显示主窗口", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, TRAY_QUIT_MENU_ID, "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_item, &quit_item])?;
    let close_state_for_menu = close_state.clone();

    let mut tray_builder = TrayIconBuilder::with_id(TRAY_ICON_ID)
        .menu(&menu)
        .tooltip("Liteyuki")
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| {
            if event.id() == TRAY_SHOW_MENU_ID {
                show_main_window(app);
            } else if event.id() == TRAY_QUIT_MENU_ID {
                close_state_for_menu
                    .allow_app_exit
                    .store(true, Ordering::Relaxed);
                app.exit(0);
            }
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } | TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                }
            ) {
                show_main_window(tray.app_handle());
            }
        });

    tray_builder = if let Some(icon) = app.default_window_icon().cloned() {
        tray_builder.icon(icon)
    } else {
        match Image::from_bytes(APP_SHELL_WINDOW_ICON_ICO) {
            Ok(icon) => tray_builder.icon(icon),
            Err(err) => {
                emit_console_log(
                    LogLevel::Warn,
                    "tauri.tray",
                    format!("failed to load tray icon from bot.ico: {err}"),
                );
                tray_builder
            }
        }
    };

    tray_builder.build(app)?;
    Ok(())
}

fn handle_main_window_event<R: Runtime>(
    window: &tauri::Window<R>,
    event: &WindowEvent,
    close_state: &DesktopCloseState,
) {
    if window.label() != MAIN_WINDOW_LABEL || close_state.allow_app_exit.load(Ordering::Relaxed) {
        return;
    }

    if let WindowEvent::CloseRequested { api, .. } = event {
        api.prevent_close();
        let behavior = resolve_desktop_close_behavior();
        if behavior.configured {
            if behavior.close_to_tray {
                hide_main_window(window);
            } else {
                close_state.allow_app_exit.store(true, Ordering::Relaxed);
                window.app_handle().exit(0);
            }
            return;
        }

        request_close_decision(window, close_state, behavior.close_to_tray);
    }
}

fn request_close_decision<R: Runtime>(
    window: &tauri::Window<R>,
    close_state: &DesktopCloseState,
    close_to_tray_default: bool,
) {
    if close_state.prompt_pending.swap(true, Ordering::Relaxed) {
        show_main_window(window.app_handle());
        return;
    }

    show_main_window(window.app_handle());
    let payload = CloseRequestPayload {
        close_to_tray_default,
    };
    let payload_json = serde_json::to_string(&payload)
        .expect("close request payload should serialize into a JS object literal");
    let Some(webview_window) = window.app_handle().get_webview_window(MAIN_WINDOW_LABEL) else {
        close_state.prompt_pending.store(false, Ordering::Relaxed);
        emit_console_log(
            LogLevel::Warn,
            "tauri.window",
            format!("failed to request close confirmation: window '{MAIN_WINDOW_LABEL}' not found"),
        );
        return;
    };
    if let Err(err) = webview_window.eval(format!(
        "window.{DESKTOP_GLOBAL} && window.{DESKTOP_GLOBAL}.requestClose({payload_json});"
    )) {
        close_state.prompt_pending.store(false, Ordering::Relaxed);
        emit_console_log(
            LogLevel::Warn,
            "tauri.window",
            format!("failed to request close confirmation from frontend: {err}"),
        );
    }
}

fn hide_main_window<R: Runtime>(window: &tauri::Window<R>) {
    if let Err(err) = window.hide() {
        emit_console_log(
            LogLevel::Warn,
            "tauri.window",
            format!("failed to hide main window on close request: {err}"),
        );
    }
}

#[tauri::command]
fn liteyuki_close_to_background(
    window: tauri::Window,
    state: State<'_, Arc<DesktopCloseState>>,
    always_remember: bool,
) -> Result<(), String> {
    if always_remember && let Err(err) = persist_desktop_close_to_tray_preference(true) {
        state.prompt_pending.store(false, Ordering::Relaxed);
        return Err(err);
    }
    state.prompt_pending.store(false, Ordering::Relaxed);
    hide_main_window(&window);
    Ok(())
}

#[tauri::command]
fn liteyuki_exit_app(
    app: AppHandle,
    state: State<'_, Arc<DesktopCloseState>>,
    always_remember: bool,
) -> Result<(), String> {
    if always_remember && let Err(err) = persist_desktop_close_to_tray_preference(false) {
        state.prompt_pending.store(false, Ordering::Relaxed);
        return Err(err);
    }
    state.prompt_pending.store(false, Ordering::Relaxed);
    state.allow_app_exit.store(true, Ordering::Relaxed);
    app.exit(0);
    Ok(())
}

#[tauri::command]
fn liteyuki_cancel_close(state: State<'_, Arc<DesktopCloseState>>) {
    state.prompt_pending.store(false, Ordering::Relaxed);
}

fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        emit_console_log(
            LogLevel::Warn,
            "tauri.window",
            format!("failed to show main window: window '{MAIN_WINDOW_LABEL}' not found"),
        );
        return;
    };

    if let Err(err) = window.unminimize() {
        emit_console_log(
            LogLevel::Warn,
            "tauri.window",
            format!("failed to restore main window from minimized state: {err}"),
        );
    }
    if let Err(err) = window.show() {
        emit_console_log(
            LogLevel::Warn,
            "tauri.window",
            format!("failed to show main window: {err}"),
        );
        return;
    }
    if let Err(err) = window.set_focus() {
        emit_console_log(
            LogLevel::Warn,
            "tauri.window",
            format!("failed to focus main window: {err}"),
        );
    }
}

fn build_runtime_api_base_init_script(runtime_api_base: &str) -> String {
    let runtime_api_base = serde_json::to_string(runtime_api_base)
        .expect("runtime api base should serialize into a JS string literal");
    format!("window.{RUNTIME_API_BASE_GLOBAL} = {runtime_api_base};")
}

fn build_local_token_init_script(token: &str) -> String {
    let token_json = serde_json::to_string(token)
        .expect("local token should serialize into a JS string literal");
    format!("window.{LOCAL_TOKEN_GLOBAL} = {token_json};")
}

fn build_desktop_close_init_script() -> String {
    format!(
        r#"
(function () {{
  const listeners = [];
  const pending = [];
  const invoke = (command, payload) => window.__TAURI_INTERNALS__.invoke(command, payload);
  window.{DESKTOP_GLOBAL} = {{
    onCloseRequested(listener) {{
      listeners.push(listener);
      while (pending.length > 0) listener(pending.shift());
      return () => {{
        const index = listeners.indexOf(listener);
        if (index >= 0) listeners.splice(index, 1);
      }};
    }},
    requestClose(payload) {{
      if (listeners.length === 0) {{
        pending.push(payload);
        return;
      }}
      for (const listener of listeners.slice()) listener(payload);
    }},
    closeToBackground(alwaysRemember) {{
      return invoke('liteyuki_close_to_background', {{ alwaysRemember }});
    }},
    exitApp(alwaysRemember) {{
      return invoke('liteyuki_exit_app', {{ alwaysRemember }});
    }},
    cancelClose() {{
      return invoke('liteyuki_cancel_close');
    }}
  }};
}})();
"#
    )
}

#[cfg(test)]
mod tests {
    use crate::build_runtime_api_base_init_script;
    use serde::Deserialize;
    use std::collections::HashMap;
    use std::fs;
    use std::path::{Path, PathBuf};

    #[derive(Debug, Deserialize)]
    struct PackageJson {
        scripts: HashMap<String, String>,
    }

    #[derive(Debug, Deserialize)]
    struct TauriConfig {
        build: TauriBuildConfig,
        bundle: TauriBundleConfig,
    }

    #[derive(Debug, Deserialize)]
    struct TauriBuildConfig {
        #[serde(rename = "beforeDevCommand")]
        before_dev_command: String,
        #[serde(rename = "beforeBuildCommand")]
        before_build_command: String,
        #[serde(rename = "devUrl")]
        dev_url: String,
        #[serde(rename = "frontendDist")]
        frontend_dist: String,
    }

    #[derive(Debug, Deserialize)]
    struct TauriBundleConfig {
        icon: Vec<String>,
    }

    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("src-tauri manifest should have repo root parent")
            .to_path_buf()
    }

    fn load_package_json() -> PackageJson {
        let package_json_path = repo_root().join("package.json");
        let package_json = fs::read_to_string(&package_json_path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", package_json_path.display()));

        serde_json::from_str(&package_json).unwrap_or_else(|err| {
            panic!(
                "failed to parse {} as package.json: {err}",
                package_json_path.display()
            )
        })
    }

    fn load_tauri_config() -> TauriConfig {
        let tauri_config_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json");
        let tauri_config = fs::read_to_string(&tauri_config_path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", tauri_config_path.display()));

        serde_json::from_str(&tauri_config).unwrap_or_else(|err| {
            panic!(
                "failed to parse {} as tauri config: {err}",
                tauri_config_path.display()
            )
        })
    }

    fn extract_pnpm_script(command: &str) -> Option<&str> {
        command.strip_prefix("pnpm ")
    }

    #[test]
    fn tauri_config_commands_stay_aligned_with_package_scripts() {
        let package_json = load_package_json();
        let tauri_config = load_tauri_config();

        assert_eq!(tauri_config.build.before_dev_command, "pnpm dev");
        assert_eq!(tauri_config.build.before_build_command, "pnpm build");
        assert_eq!(tauri_config.build.dev_url, "http://127.0.0.1:1420");
        assert_eq!(
            Path::new(&tauri_config.build.frontend_dist),
            Path::new("../frontend/dist")
        );

        for command in [
            tauri_config.build.before_dev_command.as_str(),
            tauri_config.build.before_build_command.as_str(),
        ] {
            let script_name = extract_pnpm_script(command)
                .unwrap_or_else(|| panic!("expected '{command}' to invoke a pnpm script"));
            let script = package_json.scripts.get(script_name).unwrap_or_else(|| {
                panic!("package.json is missing script '{script_name}' used by tauri.conf.json")
            });
            assert!(
                !script.trim().is_empty(),
                "package.json script '{script_name}' should not be empty"
            );
        }

        assert_eq!(
            package_json.scripts.get("cargo:dev").map(String::as_str),
            Some("tauri dev")
        );
        assert_eq!(
            package_json.scripts.get("cargo:build").map(String::as_str),
            Some("tauri build")
        );
    }

    #[test]
    fn tauri_bundle_icon_paths_exist() {
        let tauri_config = load_tauri_config();
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));

        assert!(
            !tauri_config.bundle.icon.is_empty(),
            "tauri bundle icon list should not be empty"
        );

        for icon_path in tauri_config.bundle.icon {
            let resolved = manifest_dir.join(&icon_path);
            assert!(
                resolved.is_file(),
                "tauri bundle icon path '{}' should exist at {}",
                icon_path,
                resolved.display()
            );
        }
    }

    #[test]
    fn runtime_api_base_init_script_assigns_window_global() {
        let script = build_runtime_api_base_init_script("http://127.0.0.1:14500");

        assert_eq!(
            script,
            "window.__LITEYUKI_RUNTIME_API_BASE__ = \"http://127.0.0.1:14500\";"
        );
    }
}
