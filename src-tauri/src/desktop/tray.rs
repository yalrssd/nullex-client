use crate::config::app::{get_app_config, patch_app_config};
use crate::config::mihomo::{get_controled_mihomo_config, patch_controled_mihomo_config};
use crate::core::manager::stop_core;
use crate::core::mihomo_api::mihomo_hot_reload_config;
use crate::desktop::shortcuts::trigger_main_window;
use crate::state::AppState;
use crate::sys::misc::copy_env;
use crate::sys::sysproxy::{disable_sys_proxy, trigger_sys_proxy};
use crate::utils::dirs::app_icons_dir;
use crate::utils::i18n::tr;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};

const TRAY_ID: &str = "main-tray";
static LAST_TOOLTIP_MS: AtomicI64 = AtomicI64::new(0);

fn load_tray_image(app_handle: &AppHandle, active: bool) -> Option<Image<'static>> {
    let dir = app_icons_dir();
    let candidates: &[&str] = if active {
        &["icon.png", "icon.ico"]
    } else {
        &["icon_off.png", "icon_off.ico", "icon.png", "icon.ico"]
    };
    for name in candidates {
        let p = dir.join(name);
        if p.exists() {
            if let Ok(img) = Image::from_path(&p) {
                return Some(img.to_owned());
            }
        }
    }
    app_handle
        .default_window_icon()
        .map(|i| Image::new_owned(i.rgba().to_vec(), i.width(), i.height()))
}

fn format_speed(bytes_per_sec: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes_per_sec as f64;
    if b >= GB {
        format!("{:.2} GB/s", b / GB)
    } else if b >= MB {
        format!("{:.1} MB/s", b / MB)
    } else if b >= KB {
        format!("{:.0} KB/s", b / KB)
    } else {
        format!("{bytes_per_sec} B/s")
    }
}

fn build_tray_menu_sync(
    app_handle: &AppHandle,
    app_cfg: &Value,
    mihomo_cfg: &Value,
) -> Result<(Menu<tauri::Wry>, bool), String> {

    let proxy_mode = app_cfg
        .get("proxyMode")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let tun_enable = mihomo_cfg
        .get("tun")
        .and_then(|t| t.get("enable"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mode = mihomo_cfg
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("rule");

    let active = proxy_mode || tun_enable;

    let show_item = MenuItem::with_id(app_handle, "show", tr("tray.showWindow"), true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let sep1 = PredefinedMenuItem::separator(app_handle).map_err(|e| e.to_string())?;

    let sys_proxy_item = CheckMenuItem::with_id(
        app_handle,
        "sys_proxy",
        tr("tray.sysProxy"),
        true,
        proxy_mode,
        None::<&str>,
    )
    .map_err(|e| e.to_string())?;
    let tun_item = CheckMenuItem::with_id(
        app_handle,
        "tun",
        tr("tray.tun"),
        true,
        tun_enable,
        None::<&str>,
    )
    .map_err(|e| e.to_string())?;
    let sep2 = PredefinedMenuItem::separator(app_handle).map_err(|e| e.to_string())?;

    let mode_rule = CheckMenuItem::with_id(
        app_handle,
        "mode_rule",
        tr("tray.ruleMode"),
        true,
        mode == "rule",
        None::<&str>,
    )
    .map_err(|e| e.to_string())?;
    let mode_global = CheckMenuItem::with_id(
        app_handle,
        "mode_global",
        tr("tray.globalMode"),
        true,
        mode == "global",
        None::<&str>,
    )
    .map_err(|e| e.to_string())?;
    let mode_direct = CheckMenuItem::with_id(
        app_handle,
        "mode_direct",
        tr("tray.directMode"),
        true,
        mode == "direct",
        None::<&str>,
    )
    .map_err(|e| e.to_string())?;
    let sep3 = PredefinedMenuItem::separator(app_handle).map_err(|e| e.to_string())?;

    let copy_env_item = MenuItem::with_id(app_handle, "copy_env", "Copy Env", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let quit_item = MenuItem::with_id(app_handle, "quit", tr("tray.quit"), true, None::<&str>)
        .map_err(|e| e.to_string())?;

    let menu = Menu::with_items(
        app_handle,
        &[
            &show_item,
            &sep1,
            &sys_proxy_item,
            &tun_item,
            &sep2,
            &mode_rule,
            &mode_global,
            &mode_direct,
            &sep3,
            &copy_env_item,
            &quit_item,
        ],
    )
    .map_err(|e| e.to_string())?;

    Ok((menu, active))
}

async fn handle_menu_action(state: &Arc<AppState>, app_handle: &AppHandle, id: &str) {
    match id {
        "show" => {
            if let Some(win) = app_handle.get_webview_window("main") {
                let _ = win.unminimize();
                let _ = win.show();
                let _ = win.set_focus();
            }
        }
        "sys_proxy" => {
            if let Ok(cfg) = get_app_config(state, false).await {
                let current = cfg
                    .get("proxyMode")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let next = !current;
                let write_sys = cfg
                    .get("sysProxy")
                    .and_then(|s| s.get("enable"))
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
                let only_active = cfg
                    .get("onlyActiveDevice")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                if next {
                    let _ = patch_app_config(
                        state,
                        json!({ "mainSwitchMode": "sysproxy", "proxyMode": true }),
                    )
                    .await;
                    let _ = mihomo_hot_reload_config(state, Some(app_handle)).await;
                    if write_sys {
                        let _ = trigger_sys_proxy(state, true, only_active).await;
                    }
                } else {
                    if write_sys {
                        let _ = trigger_sys_proxy(state, false, only_active).await;
                    }
                    let _ = patch_app_config(state, json!({ "proxyMode": false })).await;
                    let _ = mihomo_hot_reload_config(state, Some(app_handle)).await;
                }
                let _ = app_handle.emit("ipc-appConfigUpdated", json!([]));
                request_tray_menu_update(state, app_handle);
            }
        }
        "tun" => {
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
                let _ = mihomo_hot_reload_config(state, Some(app_handle)).await;
                let _ = app_handle.emit("ipc-controledMihomoConfigUpdated", json!([]));
                request_tray_menu_update(state, app_handle);
            }
        }
        "mode_rule" | "mode_global" | "mode_direct" => {
            let mode = id.trim_start_matches("mode_");
            let _ = patch_controled_mihomo_config(
                state,
                Some(app_handle),
                json!({ "mode": mode }),
            )
            .await;
            let _ = app_handle.emit("ipc-controledMihomoConfigUpdated", json!([]));
            request_tray_menu_update(state, app_handle);
        }
        "copy_env" => {
            let _ = copy_env(state, "powershell").await;
        }
        "quit" => {
            state.not_quit_dialog.store(true, Ordering::SeqCst);
            state.is_quitting.store(true, Ordering::SeqCst);
            let _ = stop_core(state, Some(app_handle)).await;
            let _ = disable_sys_proxy(state, false).await;
            app_handle.exit(0);
        }
        _ => {}
    }
}

fn build_tray_sync(
    state: &Arc<AppState>,
    app_handle: &AppHandle,
    app_cfg: &Value,
    mihomo_cfg: &Value,
) -> Result<tauri::tray::TrayIcon, String> {
    let (menu, active) = build_tray_menu_sync(app_handle, app_cfg, mihomo_cfg)?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip("Nullex")
        .show_menu_on_left_click(false);

    if let Some(img) = load_tray_image(app_handle, active) {
        builder = builder.icon(img);
    }

    let st_menu = state.clone();
    let app_menu = app_handle.clone();
    builder = builder.on_menu_event(move |_app, event| {
        let st = st_menu.clone();
        let ah = app_menu.clone();
        let id = event.id().as_ref().to_string();
        tauri::async_runtime::spawn(async move {
            handle_menu_action(&st, &ah, &id).await;
        });
    });

    builder = builder.on_tray_icon_event(move |tray, event| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = event
        {
            trigger_main_window(tray.app_handle());
        }
    });

    builder
        .build(app_handle)
        .map_err(|e| format!("Failed creating tray icon: {e}"))
}

pub async fn create_tray(state: &Arc<AppState>, app_handle: &AppHandle) -> Result<(), String> {
    let app_cfg = get_app_config(state, false).await.unwrap_or_default();
    if app_cfg.get("useTrayIcon").and_then(Value::as_bool) == Some(false) {
        return Ok(());
    }

    if let Some(existing) = app_handle.tray_by_id(TRAY_ID) {
        let _ = existing.set_visible(true);
        request_tray_menu_update(state, app_handle);
        return Ok(());
    }

    let mihomo_cfg = get_controled_mihomo_config(state, false)
        .await
        .unwrap_or_default();
    let tray = build_tray_sync(state, app_handle, &app_cfg, &mihomo_cfg)?;
    *state.tray_icon.lock().await = Some(tray);
    Ok(())
}

pub async fn update_tray_icon(state: &Arc<AppState>, app_handle: &AppHandle) -> Result<(), String> {
    let app_cfg = get_app_config(state, false).await.unwrap_or_default();
    let mihomo_cfg = get_controled_mihomo_config(state, false)
        .await
        .unwrap_or_default();
    update_tray_sync(app_handle, &app_cfg, &mihomo_cfg)
}

fn update_tray_sync(
    app_handle: &AppHandle,
    app_cfg: &Value,
    mihomo_cfg: &Value,
) -> Result<(), String> {
    let Some(tray) = app_handle.tray_by_id(TRAY_ID) else {
        return Ok(());
    };
    let (menu, active) = build_tray_menu_sync(app_handle, app_cfg, mihomo_cfg)?;
    let _ = tray.set_menu(Some(menu));
    if let Some(img) = load_tray_image(app_handle, active) {
        let _ = tray.set_icon(Some(img));
    }
    Ok(())
}

pub fn request_tray_menu_update(state: &Arc<AppState>, app_handle: &AppHandle) {
    let st = state.clone();
    let ah = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        let _ = update_tray_icon(&st, &ah).await;
    });
}

pub async fn close_tray_icon(app_handle: &AppHandle) -> Result<(), String> {
    if let Some(tray) = app_handle.tray_by_id(TRAY_ID) {
        let _ = tray.set_visible(false);
    }
    Ok(())
}

pub async fn show_tray_icon(state: &Arc<AppState>, app_handle: &AppHandle) -> Result<(), String> {
    if let Some(tray) = app_handle.tray_by_id(TRAY_ID) {
        let _ = tray.set_visible(true);
        update_tray_icon(state, app_handle).await
    } else {
        create_tray(state, app_handle).await
    }
}

pub fn update_tray_traffic_tooltip(
    _state: &Arc<AppState>,
    app_handle: &AppHandle,
    up: u64,
    down: u64,
) {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let prev = LAST_TOOLTIP_MS.load(Ordering::Relaxed);
    if now_ms - prev < 1000 {
        return;
    }
    LAST_TOOLTIP_MS.store(now_ms, Ordering::Relaxed);
    if let Some(tray) = app_handle.tray_by_id(TRAY_ID) {
        let tip = format!(
            "Nullex\n↑ {}\n↓ {}",
            format_speed(up),
            format_speed(down)
        );
        let _ = tray.set_tooltip(Some(&tip));
    }
}
