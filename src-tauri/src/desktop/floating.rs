use crate::config::app::{get_app_config, patch_app_config};
use crate::state::AppState;
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

pub async fn show_floating_window(state: &Arc<AppState>, app_handle: &AppHandle) -> Result<(), String> {
    if let Some(win) = app_handle.get_webview_window("floating") {
        let _ = win.show();
        let _ = win.set_focus();
        return Ok(());
    }

    let app_cfg = get_app_config(state, false).await.unwrap_or_default();
    let x = app_cfg.get("floatingWindowX").and_then(Value::as_f64);
    let y = app_cfg.get("floatingWindowY").and_then(Value::as_f64);

    let mut builder = WebviewWindowBuilder::new(
        app_handle,
        "floating",
        WebviewUrl::App("floating.html".into()),
    )
    .title("Nullex Floating")
    .inner_size(120.0, 42.0)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .resizable(false)
    .skip_taskbar(true);

    if let (Some(px), Some(py)) = (x, y) {
        builder = builder.position(px, py);
    }

    let win = builder
        .build()
        .map_err(|e| format!("Failed creating floating window: {e}"))?;

    let st = state.clone();
    win.on_window_event(move |ev| {
        if let WindowEvent::Moved(pos) = ev {
            let st_inner = st.clone();
            let px = pos.x;
            let py = pos.y;
            tauri::async_runtime::spawn(async move {
                let _ = patch_app_config(
                    &st_inner,
                    json!({ "floatingWindowX": px, "floatingWindowY": py }),
                )
                .await;
            });
        }
    });

    let _ = win.show();
    let _ = patch_app_config(state, json!({ "showFloatingWindow": true })).await;
    Ok(())
}

pub async fn close_floating_window(state: &Arc<AppState>, app_handle: &AppHandle) -> Result<(), String> {
    if let Some(win) = app_handle.get_webview_window("floating") {
        let _ = win.close();
    }
    let _ = patch_app_config(state, json!({ "showFloatingWindow": false })).await;
    Ok(())
}

pub async fn trigger_floating_window(state: &Arc<AppState>, app_handle: &AppHandle) -> Result<(), String> {
    if app_handle.get_webview_window("floating").is_some() {
        close_floating_window(state, app_handle).await
    } else {
        show_floating_window(state, app_handle).await
    }
}

pub fn show_context_menu(app_handle: &AppHandle) {
    if let Some(win) = app_handle.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}
