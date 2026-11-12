use crate::config::app::get_app_config;
use crate::core::factory::get_runtime_config;
use crate::state::AppState;
use crate::sys::pac_server::{start_pac_server, stop_pac_server};
use crate::utils::dirs::service_path;
use serde_json::Value;
use std::process::Stdio;
use std::sync::Arc;
use tokio::process::Command;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const DEFAULT_BYPASS_WINDOWS: &[&str] = &[
    "localhost",
    "127.*",
    "10.*",
    "172.16.*",
    "172.17.*",
    "172.18.*",
    "172.19.*",
    "172.20.*",
    "172.21.*",
    "172.22.*",
    "172.23.*",
    "172.24.*",
    "172.25.*",
    "172.26.*",
    "172.27.*",
    "172.28.*",
    "172.29.*",
    "172.30.*",
    "172.31.*",
    "192.168.*",
    "<local>",
];

async fn run_sparkle_sysproxy(args: &[String]) -> Result<(), String> {
    let exe = service_path();
    if !exe.exists() {
        return Err(format!("Service binary not found: {}", exe.display()));
    }
    let mut cmd = Command::new(&exe);
    cmd.args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let out = cmd
        .output()
        .await
        .map_err(|e| format!("Failed executing sysproxy command: {e}"))?;

    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        return Err(if !err.is_empty() { err } else { stdout });
    }
    Ok(())
}

pub async fn trigger_sys_proxy(
    state: &Arc<AppState>,
    enable: bool,
    only_active_device: bool,
) -> Result<(), String> {
    if !enable {
        return disable_sys_proxy(state, only_active_device).await;
    }

    let app_cfg = get_app_config(state, false).await?;
    let sys_proxy = app_cfg.get("sysProxy").cloned().unwrap_or(Value::Null);
    let mode = sys_proxy
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("manual");

    if mode == "auto" {
        let pac_port = start_pac_server(state).await?;
        let pac_url = format!("http://127.0.0.1:{pac_port}/pac");
        let mut args = vec!["sysproxy".into(), "pac".into(), "--url".into(), pac_url];
        if only_active_device {
            args.push("--only-active-device".into());
        }
        return run_sparkle_sysproxy(&args).await;
    }

    let host = sys_proxy
        .get("host")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("127.0.0.1");

    let mixed_port = if let Some(p) = *state.last_mixed_port.lock().await {
        p
    } else {
        get_runtime_config(state)
            .await
            .ok()
            .and_then(|r| r.get("mixed-port").and_then(Value::as_u64))
            .filter(|&p| p > 0)
            .unwrap_or(7897) as u16
    };

    if mixed_port == 0 {
        return disable_sys_proxy(state, only_active_device).await;
    }

    let bypass_list: Vec<String> = sys_proxy
        .get("bypass")
        .and_then(Value::as_array)
        .filter(|arr| !arr.is_empty())
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_else(|| DEFAULT_BYPASS_WINDOWS.iter().map(|s| (*s).to_string()).collect());

    let bypass = bypass_list.join(";");
    let server = format!("{host}:{mixed_port}");

    let mut args = vec![
        "sysproxy".into(),
        "proxy".into(),
        "--server".into(),
        server,
        "--bypass".into(),
        bypass,
    ];
    if only_active_device {
        args.push("--only-active-device".into());
    }

    run_sparkle_sysproxy(&args).await
}

pub async fn disable_sys_proxy(state: &Arc<AppState>, only_active_device: bool) -> Result<(), String> {
    stop_pac_server(state).await;
    let mut args = vec!["sysproxy".into(), "disable".into()];
    if only_active_device {
        args.push("--only-active-device".into());
    }
    run_sparkle_sysproxy(&args).await
}

pub fn disable_sys_proxy_sync(only_active_device: bool) {
    let exe = service_path();
    if !exe.exists() {
        return;
    }
    let mut cmd = std::process::Command::new(&exe);
    cmd.args(["sysproxy", "disable"]);
    if only_active_device {
        cmd.arg("--only-active-device");
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let _ = cmd.output();
}
