use crate::config::app::get_app_config;
use crate::core::factory::get_runtime_config;
use crate::state::AppState;
use crate::utils::dirs::themes_dir;
use reqwest::{Client, Proxy};
use serde_json::{json, Value};
use std::io::{Cursor, Read};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use zip::ZipArchive;

const THEMES_ZIP_URL: &str =
    "https://github.com/mihomo-party-org/theme-hub/releases/download/latest/themes.zip";

pub async fn resolve_themes() -> Result<Vec<Value>, String> {
    let dir = themes_dir();
    if !dir.exists() {
        let _ = tokio::fs::create_dir_all(&dir).await;
    }
    let mut list = vec![json!({ "key": "default.css", "label": "Default" })];
    let mut entries = tokio::fs::read_dir(&dir)
        .await
        .map_err(|e| format!("Failed reading themes dir: {e}"))?;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".css") && name != "default.css" {
            let label = name.trim_end_matches(".css").to_string();
            list.push(json!({ "key": name, "label": label }));
        }
    }
    Ok(list)
}

pub async fn fetch_themes(state: &Arc<AppState>) -> Result<(), String> {
    let rt = get_runtime_config(state).await.unwrap_or_default();
    let port = rt.get("mixed-port").and_then(Value::as_u64).unwrap_or(0);
    let mut builder = Client::builder().timeout(Duration::from_secs(30));
    if port > 0 && port <= u16::MAX as u64 {
        if let Ok(proxy) = Proxy::all(format!("http://127.0.0.1:{port}")) {
            builder = builder.proxy(proxy);
        }
    }
    let client = builder
        .build()
        .map_err(|e| format!("Failed building theme HTTP client: {e}"))?;

    let zip_bytes = client
        .get(THEMES_ZIP_URL)
        .send()
        .await
        .map_err(|e| format!("Failed fetching themes: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Theme server error: {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("Failed reading theme bytes: {e}"))?;

    let dir = themes_dir();
    let _ = tokio::fs::create_dir_all(&dir).await;

    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let mut archive = ZipArchive::new(Cursor::new(zip_bytes))
            .map_err(|e| format!("Invalid theme zip: {e}"))?;
        for i in 0..archive.len() {
            let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
            if file.is_dir() {
                continue;
            }
            let Some(name) = Path::new(file.name())
                .file_name()
                .and_then(|s| s.to_str())
                .map(str::to_string)
            else {
                continue;
            };
            if name.ends_with(".css") {
                let mut buf = String::new();
                if file.read_to_string(&mut buf).is_ok() {
                    let _ = std::fs::write(dir.join(name), buf);
                }
            }
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

pub async fn import_themes(files: Vec<String>) -> Result<(), String> {
    let dir = themes_dir();
    let _ = tokio::fs::create_dir_all(&dir).await;
    for f in files {
        let src = Path::new(&f);
        if !src.exists() {
            continue;
        }
        if f.to_ascii_lowercase().ends_with(".zip") {
            let bytes = tokio::fs::read(src)
                .await
                .map_err(|e| format!("Failed reading theme zip: {e}"))?;
            let dir_clone = dir.clone();
            tokio::task::spawn_blocking(move || -> Result<(), String> {
                let mut archive = ZipArchive::new(Cursor::new(bytes))
                    .map_err(|e| format!("Invalid theme zip: {e}"))?;
                for i in 0..archive.len() {
                    let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
                    if let Some(name) = Path::new(entry.name())
                        .file_name()
                        .and_then(|s| s.to_str())
                        .map(str::to_string)
                    {
                        if name.ends_with(".css") {
                            let mut buf = String::new();
                            if entry.read_to_string(&mut buf).is_ok() {
                                let _ = std::fs::write(dir_clone.join(name), buf);
                            }
                        }
                    }
                }
                Ok(())
            })
            .await
            .map_err(|e| e.to_string())??;
        } else if f.to_ascii_lowercase().ends_with(".css") {
            if let Some(name) = src.file_name() {
                let _ = tokio::fs::copy(src, dir.join(name)).await;
            }
        }
    }
    Ok(())
}

pub async fn read_theme(theme: &str) -> Result<String, String> {
    if theme.is_empty() || theme == "default.css" {
        return Ok(String::new());
    }
    let path = themes_dir().join(theme);
    if !path.exists() {
        return Ok(String::new());
    }
    tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| format!("Failed reading theme CSS: {e}"))
}

pub async fn write_theme(theme: &str, css: &str) -> Result<(), String> {
    let dir = themes_dir();
    let _ = tokio::fs::create_dir_all(&dir).await;
    tokio::fs::write(dir.join(theme), css)
        .await
        .map_err(|e| format!("Failed writing theme CSS: {e}"))
}

pub async fn apply_theme(
    state: &Arc<AppState>,
    app_handle: &AppHandle,
    theme: &str,
) -> Result<(), String> {
    let css = read_theme(theme).await.unwrap_or_default();
    *state.current_css.write().await = css.clone();
    let _ = app_handle.emit("nullex-apply-theme-css", json!({ "css": css }));
    Ok(())
}

pub async fn init_custom_theme(state: &Arc<AppState>, app_handle: &AppHandle) {
    if let Ok(cfg) = get_app_config(state, false).await {
        if let Some(theme) = cfg.get("customTheme").and_then(Value::as_str) {
            if !theme.trim().is_empty() && theme != "default.css" {
                let _ = apply_theme(state, app_handle, theme).await;
            }
        }
    }
}
