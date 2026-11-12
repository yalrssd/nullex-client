use crate::core::factory::get_runtime_config;
use crate::state::AppState;
use crate::utils::dirs::downloaded_core_dir;
use reqwest::{Client, Proxy};
use serde_json::Value;
use std::io::{Cursor, Read};
use std::sync::Arc;
use std::time::Duration;
use zip::ZipArchive;

const ALPHA_RELEASE_URL: &str =
    "https://github.com/MetaCubeX/mihomo/releases/download/Prerelease-Alpha";

fn alpha_asset_name() -> Option<&'static str> {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    return Some("mihomo-windows-amd64-v1");
    #[cfg(all(target_os = "windows", target_arch = "x86"))]
    return Some("mihomo-windows-386");
    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    return Some("mihomo-windows-arm64");
    #[cfg(not(target_os = "windows"))]
    None
}

pub async fn download_alpha_core(state: &Arc<AppState>) -> Result<(), String> {
    let asset = alpha_asset_name().ok_or_else(|| "Unsupported platform for alpha core".to_string())?;
    let runtime = get_runtime_config(state).await.unwrap_or(Value::Null);
    let mixed_port = runtime
        .get("mixed-port")
        .and_then(Value::as_u64)
        .unwrap_or(0);

    let mut builder = Client::builder().timeout(Duration::from_secs(60));
    if mixed_port > 0 && mixed_port <= u16::MAX as u64 {
        if let Ok(proxy) = Proxy::all(format!("http://127.0.0.1:{mixed_port}")) {
            builder = builder.proxy(proxy);
        }
    }
    let client = builder
        .build()
        .map_err(|e| format!("Failed creating HTTP client: {e}"))?;

    let version = client
        .get(format!("{ALPHA_RELEASE_URL}/version.txt"))
        .send()
        .await
        .map_err(|e| format!("Failed fetching alpha version: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Alpha version request failed: {e}"))?
        .text()
        .await
        .map_err(|e| format!("Failed reading alpha version: {e}"))?;

    let version = version.trim();
    let archive_url = format!("{ALPHA_RELEASE_URL}/{asset}-{version}.zip");
    let archive_bytes = client
        .get(&archive_url)
        .send()
        .await
        .map_err(|e| format!("Failed downloading alpha archive: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Alpha download HTTP error: {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("Failed reading alpha archive bytes: {e}"))?;

    let expected_exe = format!("{asset}.exe");
    let binary = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, String> {
        let mut archive = ZipArchive::new(Cursor::new(archive_bytes))
            .map_err(|e| format!("Invalid zip archive: {e}"))?;
        let mut entry = archive
            .by_name(&expected_exe)
            .map_err(|_| format!("{expected_exe}: ENOENT"))?;
        let mut out = Vec::new();
        entry
            .read_to_end(&mut out)
            .map_err(|e| format!("Failed reading zip entry: {e}"))?;
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())??;

    let target_dir = downloaded_core_dir();
    tokio::fs::create_dir_all(&target_dir)
        .await
        .map_err(|e| format!("Failed creating core dir: {e}"))?;
    let target = target_dir.join("mihomo-alpha.exe");
    let tmp = target_dir.join("mihomo-alpha.exe.tmp");
    tokio::fs::write(&tmp, &binary)
        .await
        .map_err(|e| format!("Failed writing temp alpha core: {e}"))?;
    if target.exists() {
        let _ = tokio::fs::remove_file(&target).await;
    }
    tokio::fs::rename(&tmp, &target)
        .await
        .map_err(|e| format!("Failed installing alpha core: {e}"))?;

    Ok(())
}
