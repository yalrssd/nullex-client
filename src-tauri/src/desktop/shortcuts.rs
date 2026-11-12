use crate::config::app::{get_app_config, patch_app_config};
use crate::config::mihomo::{get_controled_mihomo_config, patch_controled_mihomo_config};
use crate::core::manager::stop_core;
use crate::desktop::floating::trigger_floating_window;
use crate::state::AppState;
use crate::sys::sysproxy::{disable_sys_proxy, trigger_sys_proxy};
use serde_json::{json, Value};
use std::str::FromStr;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

const SHORTCUT_KEYS: [&str; 9] = [
    "showWindowShortcut",
    "showFloatingWindowShortcut",
    "triggerSysProxyShortcut",
    "triggerTunShortcut",
    "ruleModeShortcut",
    "globalModeShortcut",
    "directModeShortcut",
    "quitWithoutCoreShortcut",
    "restartAppShortcut",
];

fn normalize_electron_shortcut(raw: &str) -> String {
    raw.trim()
        .replace("CommandOrControl", "CmdOrCtrl")
        .replace("Command", "Super")
        .replace("Control", "Ctrl")
        .replace("Return", "Enter")
        .replace("Escape", "Esc")
        .replace(' ', "")
}

pub fn trigger_main_window(app_handle: &AppHandle) {
    let Some(win) = app_handle.get_webview_window("main") else {
        return;
    };
    let is_vis = win.is_visible().unwrap_or(false);
    let is_min = win.is_minimized().unwrap_or(false);
    let is_foc = win.is_focused().unwrap_or(false);
    if !is_vis || is_min || !is_foc {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
    } else {
        let _ = win.hide();
    }
}

async fn execute_shortcut_action(state: &Arc<AppState>, app_handle: &AppHandle, action: &str) {
    match action {
        "showWindowShortcut" => trigger_main_window(app_handle),
        "showFloatingWindowShortcut" => {
            let _ = trigger_floating_window(state, app_handle).await;
        }
        "triggerSysProxyShortcut" => {
            if let Ok(cfg) = get_app_config(state, false).await {
                let current = cfg
                    .get("sysProxy")
                    .and_then(|s| s.get("enable"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let next = !current;
                let _ = patch_app_config(state, json!({ "sysProxy": { "enable": next } })).await;
                let only_active = cfg
                    .get("onlyActiveDevice")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let _ = trigger_sys_proxy(state, next, only_active).await;
                let _ = app_handle.emit("ipc-appConfigUpdated", json!([]));
                crate::desktop::tray::request_tray_menu_update(state, app_handle);
            }
        }
        "triggerTunShortcut" => {
            if let Ok(mcfg) = get_controled_mihomo_config(state, false).await {
                let current = mcfg
                    .get("tun")
                    .and_then(|t| t.get("enable"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let _ = patch_controled_mihomo_config(
                    state,
                    Some(app_handle),
                    json!({ "tun": { "enable": !current } }),
                )
                .await;
                let _ = app_handle.emit("ipc-controledMihomoConfigUpdated", json!([]));
            }
        }
        "ruleModeShortcut" => {
            let _ = patch_controled_mihomo_config(
                state,
                Some(app_handle),
                json!({ "mode": "rule" }),
            )
            .await;
            let _ = app_handle.emit("ipc-controledMihomoConfigUpdated", json!([]));
        }
        "globalModeShortcut" => {
            let _ = patch_controled_mihomo_config(
                state,
                Some(app_handle),
                json!({ "mode": "global" }),
            )
            .await;
            let _ = app_handle.emit("ipc-controledMihomoConfigUpdated", json!([]));
        }
        "directModeShortcut" => {
            let _ = patch_controled_mihomo_config(
                state,
                Some(app_handle),
                json!({ "mode": "direct" }),
            )
            .await;
            let _ = app_handle.emit("ipc-controledMihomoConfigUpdated", json!([]));
        }
        "quitWithoutCoreShortcut" => {
            state.not_quit_dialog.store(true, Ordering::SeqCst);
            state.is_quitting.store(true, Ordering::SeqCst);
            app_handle.exit(0);
        }
        "restartAppShortcut" => {
            state.not_quit_dialog.store(true, Ordering::SeqCst);
            state.is_quitting.store(true, Ordering::SeqCst);
            let _ = stop_core(state, Some(app_handle)).await;
            let _ = disable_sys_proxy(state, false).await;
            app_handle.restart();
        }
        _ => {}
    }
}

pub async fn init_shortcuts(state: &Arc<AppState>, app_handle: &AppHandle) {
    let Ok(cfg) = get_app_config(state, false).await else {
        return;
    };
    for key in SHORTCUT_KEYS {
        if let Some(s) = cfg.get(key).and_then(Value::as_str) {
            if !s.trim().is_empty() {
                let _ = register_shortcut(state, app_handle, "", s, key).await;
            }
        }
    }
}

pub async fn register_shortcut(
    state: &Arc<AppState>,
    app_handle: &AppHandle,
    old_shortcut: &str,
    new_shortcut: &str,
    action: &str,
) -> Result<bool, String> {
    let gs = app_handle.global_shortcut();

    if !old_shortcut.trim().is_empty() {
        let norm_old = normalize_electron_shortcut(old_shortcut);
        if let Ok(parsed) = Shortcut::from_str(&norm_old) {
            let _ = gs.unregister(parsed);
        }
        state.shortcuts.lock().await.remove(action);
    }

    if new_shortcut.trim().is_empty() {
        return Ok(true);
    }

    let norm_new = normalize_electron_shortcut(new_shortcut);
    let parsed_new = Shortcut::from_str(&norm_new)
        .map_err(|e| format!("Invalid shortcut '{new_shortcut}': {e}"))?;

    let st_clone = state.clone();
    let app_clone = app_handle.clone();
    let action_owned = action.to_string();

    match gs.on_shortcut(parsed_new, move |_app, _scut, event| {
        if event.state == ShortcutState::Pressed {
            let st_inner = st_clone.clone();
            let app_inner = app_clone.clone();
            let act = action_owned.clone();
            tauri::async_runtime::spawn(async move {
                execute_shortcut_action(&st_inner, &app_inner, &act).await;
            });
        }
    }) {
        Ok(()) => {
            state
                .shortcuts
                .lock()
                .await
                .insert(action.to_string(), norm_new);
            Ok(true)
        }
        Err(_) => Ok(false),
    }
}
