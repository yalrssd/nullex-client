pub mod config;
pub mod core;
pub mod desktop;
pub mod ipc;
pub mod service;
pub mod state;
pub mod sys;
pub mod utils;

use crate::config::app::get_app_config;
use crate::config::mihomo::get_controled_mihomo_config;
use crate::config::profile::{add_profile_item, get_profile_config, set_profile_config};
use crate::config::template::{default_app_config, default_controled_mihomo_config, default_profile_config};
use crate::core::manager::{start_core, stop_core};
use crate::core::profile_updater::init_profile_updater;
use crate::desktop::floating::show_floating_window;
use crate::desktop::shortcuts::init_shortcuts;
use crate::desktop::theme::init_custom_theme;
use crate::desktop::tray::create_tray;
use crate::service::manager::init_key_manager;
use crate::state::AppState;
use crate::sys::misc::{check_elevate_task, create_elevate_task_sync};
use crate::sys::sysproxy::disable_sys_proxy_sync;
use crate::utils::dirs::{
    app_config_path, controled_mihomo_config_path, data_dir, log_dir, mihomo_test_dir,
    mihomo_work_dir, profile_config_path, profiles_dir, resources_files_dir, rules_dir,
    set_resource_dir, themes_dir,
};
use crate::utils::elevation::is_running_as_admin;
use crate::utils::geo::{link_geo_files, remove_unused_metadb, GEO_FILES};
use crate::utils::i18n::set_language;
use crate::utils::yaml::stringify_json_to_yaml;
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use url::Url;

/// On Windows: subclass the main window HWND so we can intercept
/// WM_QUERYENDSESSION / WM_ENDSESSION and set is_quitting = true
/// *before* Tauri emits CloseRequested — this prevents Nullex from
/// blocking system shutdown / restart.
#[cfg(windows)]
fn install_shutdown_hook(hwnd_raw: isize, state: Arc<AppState>) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowLongPtrW, GetWindowLongPtrW, CallWindowProcW,
        GWLP_WNDPROC, WM_QUERYENDSESSION, WM_ENDSESSION,
    };

    static STATE_PTR: std::sync::OnceLock<Arc<AppState>> = std::sync::OnceLock::new();
    let _ = STATE_PTR.set(state);

    static ORIG_PROC: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

    unsafe extern "system" fn subclass_proc(
        hwnd: windows_sys::Win32::Foundation::HWND,
        msg: u32,
        wparam: windows_sys::Win32::Foundation::WPARAM,
        lparam: windows_sys::Win32::Foundation::LPARAM,
    ) -> windows_sys::Win32::Foundation::LRESULT {
        if msg == WM_QUERYENDSESSION || msg == WM_ENDSESSION {
            if let Some(st) = STATE_PTR.get() {
                st.is_quitting.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            if msg == WM_QUERYENDSESSION {
                return 1; // TRUE = allow session to end
            }
        }
        let orig = ORIG_PROC.load(std::sync::atomic::Ordering::SeqCst);
        if orig != 0 {
            CallWindowProcW(
                Some(std::mem::transmute(orig)),
                hwnd, msg, wparam, lparam,
            )
        } else {
            0
        }
    }

    unsafe {
        let hwnd = hwnd_raw as windows_sys::Win32::Foundation::HWND;
        let orig = GetWindowLongPtrW(hwnd, GWLP_WNDPROC);
        ORIG_PROC.store(orig, std::sync::atomic::Ordering::SeqCst);
        SetWindowLongPtrW(hwnd, GWLP_WNDPROC, subclass_proc as *const () as isize);
    }
}

fn init_dirs_and_base_files() {
    for d in [
        data_dir(),
        profiles_dir(),
        log_dir(),
        rules_dir(),
        themes_dir(),
        mihomo_work_dir(),
        mihomo_test_dir(),
    ] {
        let _ = std::fs::create_dir_all(d);
    }

    if !app_config_path().exists() {
        if let Ok(yaml) = stringify_json_to_yaml(&default_app_config()) {
            let _ = std::fs::write(app_config_path(), yaml);
        }
    }
    if !controled_mihomo_config_path().exists() {
        if let Ok(yaml) = stringify_json_to_yaml(&default_controled_mihomo_config()) {
            let _ = std::fs::write(controled_mihomo_config_path(), yaml);
        }
    }
    if !profile_config_path().exists() {
        if let Ok(yaml) = stringify_json_to_yaml(&default_profile_config()) {
            let _ = std::fs::write(profile_config_path(), yaml);
        }
    }

    let files_src = resources_files_dir();
    let work_dir = mihomo_work_dir();
    for geo in GEO_FILES {
        let src = files_src.join(geo);
        let dst = work_dir.join(geo);
        if src.exists() && !dst.exists() {
            let _ = std::fs::copy(&src, &dst);
        }
    }
}

async fn cleanup_old_logs(max_days: u64) {
    if max_days == 0 {
        return;
    }
    let cutoff = Duration::from_secs(max_days.saturating_mul(86400));
    let Ok(mut entries) = tokio::fs::read_dir(log_dir()).await else {
        return;
    };
    let now = SystemTime::now();
    while let Ok(Some(entry)) = entries.next_entry().await {
        if let Ok(meta) = entry.metadata().await {
            if meta.is_file() {
                if let Ok(modified) = meta.modified() {
                    if now.duration_since(modified).unwrap_or_default() > cutoff {
                        let _ = tokio::fs::remove_file(entry.path()).await;
                    }
                }
            }
        }
    }
}

fn handle_deep_link(state: Arc<AppState>, app_handle: AppHandle, raw_url: String) {
    let Ok(parsed) = Url::parse(raw_url.trim()) else {
        return;
    };
    let scheme = parsed.scheme().to_ascii_lowercase();
    if scheme != "clash" && scheme != "mihomo" && scheme != "nullex" {
        return;
    }

    let mut sub_url: Option<String> = None;
    let mut sub_name: Option<String> = None;
    for (k, v) in parsed.query_pairs() {
        if k == "url" {
            sub_url = Some(v.into_owned());
        } else if k == "name" {
            sub_name = Some(v.into_owned());
        }
    }
    let Some(target_url) = sub_url.filter(|s| !s.trim().is_empty()) else {
        return;
    };

    tauri::async_runtime::spawn(async move {
        if let Some(win) = app_handle.get_webview_window("main") {
            let _ = win.unminimize();
            let _ = win.show();
            let _ = win.set_focus();
        }
        let (tx, rx) = tokio::sync::oneshot::channel::<bool>();
        *state.install_confirm_tx.lock().await = Some(tx);
        let _ = app_handle.emit(
            "ipc-show-profile-install-confirm",
            json!([{ "url": target_url, "name": sub_name }]),
        );

        if let Ok(true) = rx.await {
            let mut item = json!({
                "type": "remote",
                "url": target_url,
            });
            if let Some(n) = sub_name {
                if !n.trim().is_empty() {
                    item["name"] = json!(n);
                }
            }
            if let Err(e) = add_profile_item(&state, Some(&app_handle), item).await {
                let _ = app_handle.emit("ipc-showError", json!(["Import Profile", e]));
            }
        }
    });
}

async fn bootstrap_app(state: Arc<AppState>, app_handle: AppHandle) {
    let app_cfg = get_app_config(&state, true)
        .await
        .unwrap_or_else(|_| default_app_config());
    let _ = get_controled_mihomo_config(&state, true).await;
    let _ = get_profile_config(&state, true).await;

    if let Some(lang) = app_cfg.get("language").and_then(Value::as_str) {
        set_language(lang);
    }

    let max_days = app_cfg
        .get("maxLogDays")
        .and_then(Value::as_u64)
        .unwrap_or(7);
    cleanup_old_logs(max_days).await;

    remove_unused_metadb(&mihomo_work_dir()).await;
    link_geo_files(&mihomo_test_dir()).await;
    let _ = init_key_manager(&state).await;

    // First-run Windows elevation check matching Electron init flow
    if cfg!(windows) && !cfg!(debug_assertions) {
        let cli_no_admin = std::env::args().any(|a| a == "--noadmin" || a == "--smoke-test");
        let elevation_declined = app_cfg
            .get("elevationDeclined")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let perm_mode = app_cfg
            .get("corePermissionMode")
            .and_then(Value::as_str)
            .unwrap_or("");
        if is_running_as_admin() && !check_elevate_task().await {
            let _ = create_elevate_task_sync(false);
        } else if !cli_no_admin
            && !is_running_as_admin()
            && !elevation_declined
            && perm_mode != "service"
            && !check_elevate_task().await
            && !crate::utils::dirs::is_portable()
        {
            state.needs_first_run_admin.store(true, Ordering::SeqCst);
            let _ = app_handle.emit("ipc-needs-admin-setup", json!([]));
        }
    }

    let _ = create_tray(&state, &app_handle).await;
    init_shortcuts(&state, &app_handle).await;
    init_custom_theme(&state, &app_handle).await;

    let silent_start = app_cfg
        .get("silentStart")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    if !silent_start {
        if let Some(win) = app_handle.get_webview_window("main") {
            let _ = win.show();
            let _ = win.set_focus();
        }
    }

    if app_cfg
        .get("showFloatingWindow")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        let _ = show_floating_window(&state, &app_handle).await;
    }

    let _ = start_core(&state, Some(&app_handle)).await;
    init_profile_updater(&state, Some(&app_handle)).await;

    // Normalize blank profile config structure if needed
    if let Ok(pcfg) = get_profile_config(&state, false).await {
        if pcfg.get("items").and_then(Value::as_array).is_none() {
            let _ = set_profile_config(&state, Some(&app_handle), default_profile_config()).await;
        }
    }
}

pub fn run() {
    let app_state = Arc::new(AppState::default());

    let state_setup = app_state.clone();
    let state_single = app_state.clone();
    let state_exit = app_state.clone();

    tauri::Builder::default()
        .manage(app_state)
        .plugin(tauri_plugin_single_instance::init(move |app, argv, _cwd| {
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.unminimize();
                let _ = win.show();
                let _ = win.set_focus();
            }
            for arg in argv {
                if arg.starts_with("clash://")
                    || arg.starts_with("mihomo://")
                    || arg.starts_with("nullex://")
                {
                    handle_deep_link(state_single.clone(), app.clone(), arg);
                }
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            ipc::ipc_invoke,
            ipc::ipc_send
        ])
        .setup(move |app| {
            if let Ok(res_dir) = app.path().resource_dir() {
                set_resource_dir(res_dir);
            }
            init_dirs_and_base_files();

            if let Some(main_win) = app.get_webview_window("main") {
                let win_clone = main_win.clone();
                let ah_ev = app.handle().clone();
                let st_ev = state_setup.clone();

                // Install Windows shutdown hook so WM_QUERYENDSESSION
                // sets is_quitting = true before CloseRequested fires,
                // preventing Nullex from blocking system shutdown/restart.
                #[cfg(windows)]
                {
                    if let Ok(hwnd) = main_win.hwnd() {
                        install_shutdown_hook(hwnd.0 as isize, state_setup.clone());
                    }
                }
                main_win.on_window_event(move |event| match event {
                    WindowEvent::CloseRequested { api, .. } => {
                        if !st_ev.is_quitting.load(Ordering::SeqCst) {
                            api.prevent_close();
                            let _ = win_clone.hide();
                        }
                    }
                    WindowEvent::Resized(_) => {
                        if win_clone.is_maximized().unwrap_or(false) {
                            let _ = ah_ev.emit("ipc-window-maximized", json!([]));
                        } else {
                            let _ = ah_ev.emit("ipc-window-unmaximized", json!([]));
                        }
                    }
                    _ => {}
                });
            }

            let ah_boot = app.handle().clone();
            let st_boot = state_setup.clone();
            tauri::async_runtime::spawn(async move {
                bootstrap_app(st_boot, ah_boot).await;
            });

            // Check startup deep-link arguments
            for arg in std::env::args().skip(1) {
                if arg.starts_with("clash://")
                    || arg.starts_with("mihomo://")
                    || arg.starts_with("nullex://")
                {
                    handle_deep_link(state_setup.clone(), app.handle().clone(), arg);
                }
            }

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Nullex Tauri application")
        .run(move |app_handle, event| {
            if let tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit = event {
                if !state_exit.is_quitting.swap(true, Ordering::SeqCst) {
                    let st = state_exit.clone();
                    let ah = app_handle.clone();
                    tauri::async_runtime::block_on(async move {
                        let _ = stop_core(&st, Some(&ah)).await;
                    });
                    disable_sys_proxy_sync(false);
                }
            }
        });
}
