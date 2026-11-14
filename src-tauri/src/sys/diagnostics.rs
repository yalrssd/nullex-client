use crate::service::manager::service_status;
use crate::state::AppState;
use crate::utils::dirs::{log_path, mihomo_core_dir, mihomo_core_path, mihomo_work_dir};
use crate::utils::elevation::is_running_as_admin;
use network_interface::{Addr, NetworkInterface, NetworkInterfaceConfig};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TunStatus {
    pub is_up: bool,
    pub adapter_found: bool,
    pub adapter_name: Option<String>,
    pub has_error: bool,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub suggestion: Option<String>,
}

/// Look for running WinDivert / Zapret processes that can break TUN
pub fn detect_windivert_processes() -> Vec<String> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        };

        let mut detected = Vec::new();
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap.is_null() {
                return detected;
            }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

            if Process32FirstW(snap, &mut entry) != 0 {
                loop {
                    let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(0);
                    let name = OsString::from_wide(&entry.szExeFile[..len])
                        .to_string_lossy()
                        .to_ascii_lowercase();

                    if name == "winws.exe"
                        || name == "zapret.exe"
                        || name == "goodbyedpi.exe"
                        || name.contains("windivert")
                    {
                        detected.push(name);
                    }
                    if Process32NextW(snap, &mut entry) == 0 {
                        break;
                    }
                }
            }
            CloseHandle(snap);
        }
        detected
    }
    #[cfg(not(windows))]
    Vec::new()
}

/// Detect other VPN or proxy client processes
pub fn detect_other_vpn_processes() -> Vec<String> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        };

        let targets = [
            "clash-verge.exe",
            "clash-verge-rev.exe",
            "clash for windows.exe",
            "hiddify.exe",
            "v2rayn.exe",
            "nekoray.exe",
            "wireguard.exe",
            "openvpn.exe",
            "sing-box.exe",
        ];

        let mut detected = Vec::new();
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap.is_null() {
                return detected;
            }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

            if Process32FirstW(snap, &mut entry) != 0 {
                loop {
                    let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(0);
                    let name = OsString::from_wide(&entry.szExeFile[..len])
                        .to_string_lossy()
                        .to_ascii_lowercase();

                    if targets.contains(&name.as_str()) {
                        detected.push(name);
                    }
                    if Process32NextW(snap, &mut entry) == 0 {
                        break;
                    }
                }
            }
            CloseHandle(snap);
        }
        detected
    }
    #[cfg(not(windows))]
    Vec::new()
}

/// Get all running mihomo.exe processes with their PID, full path, and whether they belong to Nullex
pub fn get_mihomo_processes() -> Vec<Value> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        };
        use windows_sys::Win32::System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
        };

        let our_sidecar = mihomo_core_path("mihomo")
            .map(|p| p.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();

        let mut list = Vec::new();
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap.is_null() {
                return list;
            }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

            if Process32FirstW(snap, &mut entry) != 0 {
                loop {
                    let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(0);
                    let name = OsString::from_wide(&entry.szExeFile[..len])
                        .to_string_lossy()
                        .to_ascii_lowercase();

                    if name == "mihomo.exe" {
                        let pid = entry.th32ProcessID;
                        let mut exe_path_str = String::new();
                        let ph = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
                        if !ph.is_null() {
                            let mut buf = [0u16; 1024];
                            let mut sz = buf.len() as u32;
                            if QueryFullProcessImageNameW(ph, 0, buf.as_mut_ptr(), &mut sz) != 0 {
                                exe_path_str = OsString::from_wide(&buf[..sz as usize])
                                    .to_string_lossy()
                                    .to_string();
                            }
                            CloseHandle(ph);
                        }
                        let is_ours = !our_sidecar.is_empty()
                            && exe_path_str.to_ascii_lowercase() == our_sidecar;
                        list.push(json!({
                            "pid": pid,
                            "path": exe_path_str,
                            "isNullex": is_ours
                        }));
                    }
                    if Process32NextW(snap, &mut entry) == 0 {
                        break;
                    }
                }
            }
            CloseHandle(snap);
        }
        list
    }
    #[cfg(not(windows))]
    Vec::new()
}

/// Read the last N lines of today's core log
pub async fn get_recent_core_logs(max_lines: usize) -> Vec<String> {
    let path = log_path();
    if !path.exists() {
        return Vec::new();
    }
    let Ok(content) = tokio::fs::read_to_string(&path).await else {
        return Vec::new();
    };
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(max_lines);
    lines[start..].iter().map(|s| s.to_string()).collect()
}

/// Check the current TUN status by inspecting network adapters and recent log errors
pub async fn check_tun_status() -> TunStatus {
    let mut adapter_found = false;
    let mut adapter_name = None;

    if let Ok(interfaces) = NetworkInterface::show() {
        for iface in interfaces {
            let name_low = iface.name.to_ascii_lowercase();
            let is_tun_device = name_low.contains("mihomo")
                || name_low.contains("wintun")
                || name_low.contains("nullex");
            let has_fake_ip = iface.addr.iter().any(|addr| match addr {
                Addr::V4(v4) => v4.ip.to_string().starts_with("198.18."),
                _ => false,
            });

            if is_tun_device || has_fake_ip {
                adapter_found = true;
                adapter_name = Some(iface.name.clone());
                break;
            }
        }
    }

    // Inspect recent log lines for TUN startup errors
    let recent_logs = get_recent_core_logs(60).await;
    let mut last_tun_error = None;
    for line in recent_logs.iter().rev() {
        let low = line.to_ascii_lowercase();
        if low.contains("start tun") || low.contains("configure tun") || low.contains("tun listening error") {
            last_tun_error = Some(line.clone());
            break;
        }
    }

    // Check for WinDivert conflict
    let windivert_procs = detect_windivert_processes();
    let has_windivert = !windivert_procs.is_empty();

    // Check wintun.dll presence
    let wintun_sidecar = mihomo_core_dir().join("wintun.dll");
    let wintun_work = mihomo_work_dir().join("wintun.dll");
    let wintun_exists = wintun_sidecar.exists() || wintun_work.exists();

    if !adapter_found {
        if let Some(err_line) = last_tun_error {
            let low = err_line.to_ascii_lowercase();
            if low.contains("access is denied") {
                return TunStatus {
                    is_up: false,
                    adapter_found: false,
                    adapter_name: None,
                    has_error: true,
                    error_code: Some("access_denied".into()),
                    error_message: Some(err_line),
                    suggestion: Some(
                        "TUN mode requires Administrator rights or another security software is blocking network adapter configuration. Try running as Administrator.".into(),
                    ),
                };
            }
            if !wintun_exists {
                return TunStatus {
                    is_up: false,
                    adapter_found: false,
                    adapter_name: None,
                    has_error: true,
                    error_code: Some("wintun_missing".into()),
                    error_message: Some("wintun.dll not found".into()),
                    suggestion: Some(
                        "wintun.dll driver is missing from application resources.".into(),
                    ),
                };
            }
            return TunStatus {
                is_up: false,
                adapter_found: false,
                adapter_name: None,
                has_error: true,
                error_code: Some("tun_start_error".into()),
                error_message: Some(err_line),
                suggestion: Some("Failed to initialize TUN interface in core.".into()),
            };
        }

        if has_windivert {
            return TunStatus {
                is_up: false,
                adapter_found: false,
                adapter_name: None,
                has_error: true,
                error_code: Some("windivert_conflict".into()),
                error_message: Some(format!(
                    "Conflicting packet filter process running: {}",
                    windivert_procs.join(", ")
                )),
                suggestion: Some(
                    "WinDivert / Zapret (winws.exe) is running. It intercepts packets at the kernel driver level and often breaks virtual TUN adapters. Close Zapret/GoodbyeDPI to use TUN mode.".into(),
                ),
            };
        }

        return TunStatus {
            is_up: false,
            adapter_found: false,
            adapter_name: None,
            has_error: true,
            error_code: Some("adapter_missing".into()),
            error_message: Some("TUN network adapter 'mihomo' was not found.".into()),
            suggestion: Some(
                "The TUN virtual adapter has not been created yet or failed to start.".into(),
            ),
        };
    }

    TunStatus {
        is_up: true,
        adapter_found: true,
        adapter_name,
        has_error: false,
        error_code: None,
        error_message: None,
        suggestion: None,
    }
}

/// Run complete TUN diagnostics report
pub async fn run_tun_diagnostics(_state: &Arc<AppState>) -> Value {
    let is_admin = is_running_as_admin();
    let s_status = service_status().await.unwrap_or_else(|e| format!("error: {e}"));

    let wintun_sidecar = mihomo_core_dir().join("wintun.dll");
    let wintun_work = mihomo_work_dir().join("wintun.dll");

    let mihomo_procs = get_mihomo_processes();
    let windivert_procs = detect_windivert_processes();
    let other_vpns = detect_other_vpn_processes();

    // Inspect network adapters
    let mut adapters_info = Vec::new();
    let mut tun_adapter_found = false;
    if let Ok(interfaces) = NetworkInterface::show() {
        for iface in interfaces {
            let name_low = iface.name.to_ascii_lowercase();
            let is_tun = name_low.contains("mihomo")
                || name_low.contains("wintun")
                || name_low.contains("nullex")
                || iface.addr.iter().any(|a| match a {
                    Addr::V4(v4) => v4.ip.to_string().starts_with("198.18."),
                    _ => false,
                });
            if is_tun {
                tun_adapter_found = true;
            }
            let addrs: Vec<String> = iface
                .addr
                .iter()
                .map(|a| match a {
                    Addr::V4(v4) => v4.ip.to_string(),
                    Addr::V6(v6) => v6.ip.to_string(),
                })
                .collect();
            adapters_info.push(json!({
                "name": iface.name,
                "isTun": is_tun,
                "addresses": addrs
            }));
        }
    }

    // DNS test via Mihomo API
    let start_dns = Instant::now();
    let _dns_result = crate::core::mihomo_api::mihomo_config().await;
    let dns_rtt_ms = start_dns.elapsed().as_millis() as u64;

    // Recent logs
    let recent_logs = get_recent_core_logs(40).await;
    let tun_logs: Vec<String> = recent_logs
        .into_iter()
        .filter(|l| {
            let low = l.to_ascii_lowercase();
            low.contains("tun") || low.contains("dns") || low.contains("error") || low.contains("fatal")
        })
        .collect();

    // Human-readable summary for clipboard copy
    let mut summary = String::new();
    summary.push_str("=== NULLEX CLIENT TUN DIAGNOSTICS ===\n");
    summary.push_str(&format!("Timestamp: {}\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S")));
    summary.push_str(&format!("Administrator Privileges: {}\n", if is_admin { "YES (Elevated)" } else { "NO (Standard User)" }));
    summary.push_str(&format!("Sparkle Service: {}\n", s_status));
    summary.push_str(&format!("wintun.dll (sidecar): {}\n", if wintun_sidecar.exists() { "Present" } else { "MISSING" }));
    summary.push_str(&format!("wintun.dll (work dir): {}\n", if wintun_work.exists() { "Present" } else { "Not copied" }));
    summary.push_str(&format!("TUN Virtual Adapter: {}\n", if tun_adapter_found { "ACTIVE" } else { "NOT FOUND" }));
    summary.push_str(&format!("Running mihomo.exe count: {}\n", mihomo_procs.len()));
    for p in &mihomo_procs {
        summary.push_str(&format!("  - PID {}: {} (Nullex: {})\n", p["pid"], p["path"], p["isNullex"]));
    }
    if !windivert_procs.is_empty() {
        summary.push_str(&format!("WARNING: WinDivert / Zapret detected: {}\n", windivert_procs.join(", ")));
        summary.push_str("  -> WinDivert kernel filters frequently block or break TUN adapters!\n");
    } else {
        summary.push_str("WinDivert / Zapret: Not detected\n");
    }
    if !other_vpns.is_empty() {
        summary.push_str(&format!("Other VPN clients detected: {}\n", other_vpns.join(", ")));
    }
    summary.push_str(&format!("Core API response time: {} ms\n", dns_rtt_ms));
    summary.push_str("\nRecent TUN/Core logs:\n");
    for line in tun_logs.iter().rev().take(10) {
        summary.push_str(&format!("  {}\n", line));
    }
    summary.push_str("=====================================\n");

    json!({
        "isAdmin": is_admin,
        "serviceStatus": s_status,
        "tunAdapterFound": tun_adapter_found,
        "wintunSidecarExists": wintun_sidecar.exists(),
        "wintunWorkExists": wintun_work.exists(),
        "mihomoProcesses": mihomo_procs,
        "windivertDetected": !windivert_procs.is_empty(),
        "windivertProcesses": windivert_procs,
        "otherVpns": other_vpns,
        "adapters": adapters_info,
        "dnsRttMs": dns_rtt_ms,
        "tunLogs": tun_logs,
        "summary": summary
    })
}
