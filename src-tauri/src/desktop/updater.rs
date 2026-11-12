use crate::state::AppState;
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

const REPO_OWNER: &str = "nexott";
const REPO_NAME: &str = "nullex-client";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

fn parse_semver(v: &str) -> Option<(u64, u64, u64)> {
    let s = v.trim().trim_start_matches('v').trim_start_matches('V');
    let mut parts = s.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts
        .next()
        .unwrap_or("0")
        .split('-')
        .next()?
        .parse()
        .ok()?;
    Some((major, minor, patch))
}

fn is_newer_version(remote: &str, current: &str) -> bool {
    match (parse_semver(remote), parse_semver(current)) {
        (Some(r), Some(c)) => r > c,
        _ => false,
    }
}

pub async fn check_update() -> Result<Option<Value>, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;

    let url = format!("https://api.github.com/repos/{REPO_OWNER}/{REPO_NAME}/releases/latest");
    let resp = client
        .get(&url)
        .header("User-Agent", "Nullex-Client")
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .map_err(|e| format!("Failed to check for updates: {e}"))?;

    if !resp.status().is_success() {
        return Ok(None);
    }

    let release: Value = resp
        .json()
        .await
        .map_err(|e| format!("Invalid release json: {e}"))?;

    let tag = release
        .get("tag_name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();

    if tag.is_empty() {
        return Ok(None);
    }

    if is_newer_version(tag, CURRENT_VERSION) {
        let changelog = release
            .get("body")
            .and_then(Value::as_str)
            .unwrap_or("No changelog provided.");

        Ok(Some(json!({
            "version": tag,
            "changelog": changelog
        })))
    } else {
        Ok(None)
    }
}

pub async fn download_and_install_update(
    state: &Arc<AppState>,
    app_handle: &AppHandle,
    version: &str,
) -> Result<(), String> {
    state.update_cancel.store(false, Ordering::SeqCst);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(|e| e.to_string())?;

    let clean_ver = version.trim();
    let url = if clean_ver.is_empty() || clean_ver == "latest" {
        format!("https://api.github.com/repos/{REPO_OWNER}/{REPO_NAME}/releases/latest")
    } else {
        format!("https://api.github.com/repos/{REPO_OWNER}/{REPO_NAME}/releases/tags/{clean_ver}")
    };

    let resp = client
        .get(&url)
        .header("User-Agent", "Nullex-Client")
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .map_err(|e| format!("Failed to query release info: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("Release not found (HTTP {})", resp.status()));
    }

    let release: Value = resp.json().await.map_err(|e| e.to_string())?;
    let assets = release
        .get("assets")
        .and_then(Value::as_array)
        .ok_or_else(|| "No release assets found".to_string())?;

    // Find installer exe asset
    let asset = assets
        .iter()
        .find(|a| {
            let name = a.get("name").and_then(Value::as_str).unwrap_or("");
            name.ends_with(".exe") && (name.contains("setup") || name.contains("Setup") || name.contains("Nullex"))
        })
        .or_else(|| {
            assets.iter().find(|a| {
                let name = a.get("name").and_then(Value::as_str).unwrap_or("");
                name.ends_with(".exe")
            })
        })
        .ok_or_else(|| "No matching .exe installer found in release".to_string())?;

    let download_url = asset
        .get("browser_download_url")
        .and_then(Value::as_str)
        .ok_or_else(|| "Missing download URL for asset".to_string())?;

    let file_name = asset
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Nullex-Setup.exe");

    let dest_path = std::env::temp_dir().join(file_name);

    // Initial progress
    let _ = app_handle.emit(
        "ipc-update-status",
        json!([{
            "downloading": true,
            "progress": 0
        }]),
    );

    let mut download_resp = client
        .get(download_url)
        .header("User-Agent", "Nullex-Client")
        .send()
        .await
        .map_err(|e| format!("Download request failed: {e}"))?;

    if !download_resp.status().is_success() {
        return Err(format!("Download failed with status: {}", download_resp.status()));
    }

    let total_size = download_resp.content_length().unwrap_or(0);
    let mut downloaded: u64 = 0;

    let mut file = tokio::fs::File::create(&dest_path)
        .await
        .map_err(|e| format!("Failed to create destination file: {e}"))?;

    while let Some(chunk_res) = download_resp.chunk().await.transpose() {
        if state.update_cancel.load(Ordering::SeqCst) {
            drop(file);
            let _ = tokio::fs::remove_file(&dest_path).await;
            let _ = app_handle.emit(
                "ipc-update-status",
                json!([{
                    "downloading": false,
                    "progress": 0
                }]),
            );
            return Err("Update download cancelled".to_string());
        }

        let chunk = chunk_res.map_err(|e| format!("Error downloading chunk: {e}"))?;
        use tokio::io::AsyncWriteExt;
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Failed to write chunk: {e}"))?;

        downloaded += chunk.len() as u64;
        let progress = if total_size > 0 {
            ((downloaded as f64 / total_size as f64) * 100.0).clamp(0.0, 100.0) as u32
        } else {
            50
        };

        let _ = app_handle.emit(
            "ipc-update-status",
            json!([{
                "downloading": true,
                "progress": progress
            }]),
        );
    }

    use tokio::io::AsyncWriteExt;
    let _ = file.flush().await;
    drop(file);

    let _ = app_handle.emit(
        "ipc-update-status",
        json!([{
            "downloading": false,
            "progress": 100
        }]),
    );

    // Launch installer and exit app
    #[cfg(windows)]
    {
        let res = std::process::Command::new(&dest_path).spawn();
        if let Err(e) = res {
            return Err(format!("Failed to launch installer: {e}"));
        }
        app_handle.exit(0);
    }

    Ok(())
}

pub async fn cancel_update(state: &Arc<AppState>) -> Result<(), String> {
    state.update_cancel.store(true, Ordering::SeqCst);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_comparison() {
        assert!(is_newer_version("1.1.1", "1.1.0"));
        assert!(is_newer_version("v1.1.1", "1.1.0"));
        assert!(is_newer_version("1.2.0", "1.1.0"));
        assert!(is_newer_version("2.0.0", "1.1.0"));
        assert!(!is_newer_version("1.1.0", "1.1.0"));
        assert!(!is_newer_version("v1.1.0", "1.1.0"));
        assert!(!is_newer_version("1.0.9", "1.1.0"));
    }
}
