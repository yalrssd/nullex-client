use crate::config::app::{get_app_config, patch_app_config};
use crate::core::factory::get_runtime_config;
use crate::state::AppState;
use crate::sys::autorun::xml_escape;
use crate::utils::dirs::{
    app_config_path, controled_mihomo_config_path, data_dir, exe_path, mihomo_core_path,
    profile_path, resources_files_dir, task_dir,
};
use crate::utils::elevation::{exec_with_elevation, is_running_as_admin};
use arboard::Clipboard;
use base64::{engine::general_purpose::STANDARD, Engine};
use network_interface::{Addr, NetworkInterface, NetworkInterfaceConfig};
use reqwest::{Client, Proxy};
use serde_json::{json, Map, Value};
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use tokio::process::Command;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const ELEVATE_TASK_NAME: &str = "nullex-run";

const DEFAULT_APP_ICON_DATA_URL: &str = "data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSIzMiIgaGVpZ2h0PSIzMiIgdmlld0JveD0iMCAwIDI0IDI0IiBmaWxsPSJub25lIiBzdHJva2U9IiM5NGEzYjgiIHN0cm9rZS13aWR0aD0iMS44Ij48cmVjdCB4PSIzIiB5PSI0IiB3aWR0aD0iMTgiIGhlaWdodD0iMTYiIHJ4PSIyIi8+PGxpbmUgeDE9IjMiIHkyPSI5IiB4Mj0iMjEiIHkyPSI5Ii8+PC9zdmc+";

fn encode_utf16le_bom(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + text.len() * 2);
    out.push(0xFF);
    out.push(0xFE);
    for unit in text.encode_utf16() {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out
}

pub fn get_file_path_dialog(app_handle: &AppHandle, extensions: &[String]) -> Option<Vec<String>> {
    let mut builder = app_handle.dialog().file();
    if !extensions.is_empty() {
        let ext_refs: Vec<&str> = extensions.iter().map(String::as_str).collect();
        builder = builder.add_filter("Files", &ext_refs);
    }
    builder
        .blocking_pick_files()
        .map(|paths| paths.into_iter().map(|p| p.to_string()).collect())
}

pub async fn read_text_file(file_path: &str) -> Result<String, String> {
    tokio::fs::read_to_string(file_path)
        .await
        .map_err(|e| format!("Failed reading file '{}': {e}", file_path))
}

pub fn open_file_by_id(app_handle: &AppHandle, id: &str) -> Result<(), String> {
    let target = profile_path(id);
    let path_str = target.to_string_lossy().to_string();
    app_handle
        .opener()
        .open_path(path_str, None::<String>)
        .map_err(|e| format!("Failed opening file: {e}"))
}

pub fn open_data_dir(app_handle: &AppHandle) -> Result<(), String> {
    let path_str = data_dir().to_string_lossy().to_string();
    app_handle
        .opener()
        .open_path(path_str, None::<String>)
        .map_err(|e| format!("Failed opening data dir: {e}"))
}

pub async fn open_uwp_tool() -> Result<(), String> {
    let tool = resources_files_dir().join("enableLoopback.exe");
    if !tool.exists() {
        return Err(format!("enableLoopback.exe not found at {}", tool.display()));
    }
    exec_with_elevation(&tool.to_string_lossy(), &[]).await
}

pub async fn setup_firewall() -> Result<(), String> {
    let mihomo_p = mihomo_core_path("mihomo")
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let alpha_p = mihomo_core_path("mihomo-alpha")
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let app_p = exe_path().to_string_lossy().to_string();

    let ps_script = format!(
        r#"
Remove-NetFirewallRule -DisplayName "mihomo" -ErrorAction SilentlyContinue
Remove-NetFirewallRule -DisplayName "mihomo-alpha" -ErrorAction SilentlyContinue
Remove-NetFirewallRule -DisplayName "Nullex" -ErrorAction SilentlyContinue
if (Test-Path "{mihomo_p}") {{
  New-NetFirewallRule -DisplayName "mihomo" -Direction Inbound -Action Allow -Program "{mihomo_p}" -Enabled True -Profile Any -ErrorAction SilentlyContinue | Out-Null
}}
if (Test-Path "{alpha_p}") {{
  New-NetFirewallRule -DisplayName "mihomo-alpha" -Direction Inbound -Action Allow -Program "{alpha_p}" -Enabled True -Profile Any -ErrorAction SilentlyContinue | Out-Null
}}
if (Test-Path "{app_p}") {{
  New-NetFirewallRule -DisplayName "Nullex" -Direction Inbound -Action Allow -Program "{app_p}" -Enabled True -Profile Any -ErrorAction SilentlyContinue | Out-Null
}}
"#
    );

    let tmp_ps = std::env::temp_dir().join("nullex-firewall.ps1");
    let _ = tokio::fs::write(&tmp_ps, ps_script).await;
    let res = exec_with_elevation(
        "powershell.exe",
        &[
            "-NoProfile".into(),
            "-ExecutionPolicy".into(),
            "Bypass".into(),
            "-File".into(),
            tmp_ps.to_string_lossy().to_string(),
        ],
    )
    .await;
    let _ = tokio::fs::remove_file(&tmp_ps).await;
    res
}

pub async fn check_elevate_task() -> bool {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("schtasks.exe");
        cmd.args(["/Query", "/TN", ELEVATE_TASK_NAME])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW);
        cmd.status().await.map(|s| s.success()).unwrap_or(false)
    }
    #[cfg(not(windows))]
    false
}

pub async fn delete_elevate_task() -> Result<(), String> {
    #[cfg(windows)]
    {
        if is_running_as_admin() {
            let mut cmd = Command::new("schtasks.exe");
            cmd.args(["/Delete", "/TN", ELEVATE_TASK_NAME, "/F"])
                .creation_flags(CREATE_NO_WINDOW);
            if let Ok(out) = cmd.output().await {
                if out.status.success() {
                    return Ok(());
                }
            }
        }
        exec_with_elevation(
            "schtasks.exe",
            &[
                "/Delete".into(),
                "/TN".into(),
                ELEVATE_TASK_NAME.into(),
                "/F".into(),
            ],
        )
        .await
    }
    #[cfg(not(windows))]
    Ok(())
}

pub fn create_elevate_task_sync(force: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        let dir = task_dir();
        let runner_dst = dir.join("nullex-run.exe");
        let runner_src = resources_files_dir().join("nullex-run.exe");
        if runner_src.exists() && (force || !runner_dst.exists()) {
            std::fs::copy(&runner_src, &runner_dst)
                .map_err(|e| format!("Failed copying nullex-run.exe: {e}"))?;
        }

        let cmd_path = if runner_dst.exists() {
            runner_dst
        } else {
            exe_path()
        };

        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <Principals>
    <Principal id="Author">
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>Parallel</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>false</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>3</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{}</Command>
    </Exec>
  </Actions>
</Task>"#,
            xml_escape(&cmd_path.to_string_lossy())
        );

        let xml_path = dir.join(format!("{ELEVATE_TASK_NAME}.xml"));
        std::fs::write(&xml_path, encode_utf16le_bom(&xml))
            .map_err(|e| format!("Failed writing elevate task XML: {e}"))?;

        let out = std::process::Command::new("schtasks.exe")
            .args([
                "/Create",
                "/TN",
                ELEVATE_TASK_NAME,
                "/XML",
                &xml_path.to_string_lossy(),
                "/F",
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("Failed running schtasks: {e}"))?;

        if !out.status.success() {
            let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
            return Err(if msg.is_empty() {
                "Failed creating elevate task".to_string()
            } else {
                msg
            });
        }
    }
    let _ = force;
    Ok(())
}

pub fn get_interfaces() -> Value {
    let Ok(ifaces) = NetworkInterface::show() else {
        return json!({});
    };
    let mut map = Map::new();
    for iface in ifaces {
        let mut addrs = Vec::new();
        for addr in iface.addr {
            match addr {
                Addr::V4(v4) => {
                    addrs.push(json!({
                        "address": v4.ip.to_string(),
                        "netmask": v4.netmask.map(|m| m.to_string()).unwrap_or_default(),
                        "family": "IPv4",
                        "mac": iface.mac_addr.clone().unwrap_or_default(),
                        "internal": v4.ip.is_loopback()
                    }));
                }
                Addr::V6(v6) => {
                    addrs.push(json!({
                        "address": v6.ip.to_string(),
                        "netmask": v6.netmask.map(|m| m.to_string()).unwrap_or_default(),
                        "family": "IPv6",
                        "mac": iface.mac_addr.clone().unwrap_or_default(),
                        "internal": v6.ip.is_loopback()
                    }));
                }
            }
        }
        if !addrs.is_empty() {
            map.insert(iface.name, Value::Array(addrs));
        }
    }
    Value::Object(map)
}

pub async fn copy_env(state: &Arc<AppState>, env_type: &str) -> Result<(), String> {
    let app_cfg = get_app_config(state, false).await.unwrap_or_default();
    let rt = get_runtime_config(state).await.unwrap_or_default();
    let mixed_port = rt
        .get("mixed-port")
        .and_then(Value::as_u64)
        .filter(|&p| p > 0)
        .unwrap_or(7897);
    let host = app_cfg
        .get("sysProxy")
        .and_then(|s| s.get("host"))
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("127.0.0.1");

    let addr = format!("http://{host}:{mixed_port}");
    let text = match env_type {
        "bash" => format!("export https_proxy={addr} http_proxy={addr} all_proxy={addr}"),
        "cmd" => format!("set http_proxy={addr}\r\nset https_proxy={addr}"),
        "powershell" => format!("$env:HTTP_PROXY=\"{addr}\"; $env:HTTPS_PROXY=\"{addr}\""),
        "fish" => format!("set -x http_proxy {addr}; set -x https_proxy {addr}; set -x all_proxy {addr}"),
        _ => addr,
    };

    let mut clipboard = Clipboard::new().map_err(|e| format!("Clipboard error: {e}"))?;
    clipboard
        .set_text(text)
        .map_err(|e| format!("Failed writing clipboard: {e}"))
}

pub async fn reset_app_config(state: &Arc<AppState>, app_handle: &AppHandle) -> Result<(), String> {
    let _ = tokio::fs::remove_file(app_config_path()).await;
    let _ = tokio::fs::remove_file(controled_mihomo_config_path()).await;
    *state.app_config.write().await = None;
    *state.mihomo_config.write().await = None;
    let _ = get_app_config(state, true).await;
    let _ = app_handle.emit("ipc-appConfigUpdated", json!([]));
    let _ = app_handle.emit("ipc-controledMihomoConfigUpdated", json!([]));
    Ok(())
}

pub async fn decline_elevation(state: &Arc<AppState>) -> Result<(), String> {
    patch_app_config(state, json!({ "elevationDeclined": true })).await
}

pub async fn get_app_name(app_path: &str) -> String {
    Path::new(app_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}

pub async fn get_icon_data_url(_app_path: &str) -> String {
    DEFAULT_APP_ICON_DATA_URL.to_string()
}

pub async fn get_image_data_url(state: &Arc<AppState>, url: &str) -> Result<String, String> {
    if url.starts_with("data:") {
        return Ok(url.to_string());
    }
    let rt = get_runtime_config(state).await.unwrap_or_default();
    let port = rt.get("mixed-port").and_then(Value::as_u64).unwrap_or(0);
    let mut builder = Client::builder()
        .timeout(Duration::from_secs(15))
        .danger_accept_invalid_certs(true);
    if port > 0 && port <= u16::MAX as u64 {
        if let Ok(proxy) = Proxy::all(format!("http://127.0.0.1:{port}")) {
            builder = builder.proxy(proxy);
        }
    }
    let client = builder
        .build()
        .map_err(|e| format!("Failed creating image client: {e}"))?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Image request failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Image HTTP error: {e}"))?;
    let mime = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("image/png")
        .to_string();
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("Failed reading image bytes: {e}"))?;
    Ok(format!("data:{mime};base64,{}", STANDARD.encode(&bytes)))
}

pub async fn restart_as_admin(
    app_handle: &AppHandle,
    state: &Arc<AppState>,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SHELLEXECUTEINFOW, SEE_MASK_NOCLOSEPROCESS};

        // 1. Stop core and system proxy before elevating
        crate::core::manager::stop_core(state, Some(app_handle)).await?;
        crate::sys::sysproxy::disable_sys_proxy_sync(false);

        let exe = exe_path();
        let pid = std::process::id();

        // Build the --wait-pid argument as a wide string
        let verb: Vec<u16> = "runas\0".encode_utf16().collect();
        let file: Vec<u16> = exe
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0u16))
            .collect();
        let params_str = format!("--wait-pid={pid}\0");
        let params: Vec<u16> = params_str.encode_utf16().collect();

        let mut sei = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS,
            hwnd: std::ptr::null_mut(),
            lpVerb: verb.as_ptr(),
            lpFile: file.as_ptr(),
            lpParameters: params.as_ptr(),
            lpDirectory: std::ptr::null(),
            nShow: 1, // SW_NORMAL
            hInstApp: std::ptr::null_mut(),
            lpIDList: std::ptr::null_mut(),
            lpClass: std::ptr::null(),
            hkeyClass: std::ptr::null_mut(),
            dwHotKey: 0,
            Anonymous: unsafe { std::mem::zeroed() },
            hProcess: std::ptr::null_mut(),
        };

        let ok = unsafe { ShellExecuteExW(&mut sei) };
        if ok == 0 {
            let err = unsafe { windows_sys::Win32::Foundation::GetLastError() };
            // ERROR_CANCELLED = 1223 → user cancelled UAC → do NOT exit
            if err == 1223 {
                return Err("UAC elevation was cancelled by user".to_string());
            }
            return Err(format!("ShellExecuteExW failed: error code {err}"));
        }

        // Close the new process handle — it'll wait for us via --wait-pid
        if !sei.hProcess.is_null() {
            unsafe { CloseHandle(sei.hProcess) };
        }

        // 2. Exit cleanly via app_handle so RunEvent::Exit fires
        app_handle.exit(0);
        Ok(())
    }
    #[cfg(not(windows))]
    Err("Not supported".to_string())
}
