use crate::config::app::get_app_config;
use crate::config::profile::get_profile_config;
use crate::core::factory::generate_profile;
use crate::core::mihomo_api::{
    apply_log_level, mihomo_version, start_mihomo_streams, stop_mihomo_streams,
};
use crate::service::manager::{service_api_post, test_service_connection};
use crate::state::{AppState, CoreErrorPayload, CoreStatePayload};
use crate::sys::sysproxy::trigger_sys_proxy;
use crate::utils::dirs::{
    log_path, mihomo_core_path, mihomo_ipc_path, mihomo_profile_work_dir, mihomo_test_dir,
    mihomo_work_dir,
};
use crate::utils::elevation::is_running_as_admin;
use serde_json::{json, Value};
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{Mutex, Notify};

static CORE_OP_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn core_op_lock() -> &'static Mutex<()> {
    CORE_OP_LOCK.get_or_init(|| Mutex::new(()))
}

async fn set_core_state(
    state: &Arc<AppState>,
    app_handle: Option<&AppHandle>,
    status: &str,
    error: Option<CoreErrorPayload>,
) {
    let payload = CoreStatePayload {
        status: status.to_string(),
        error,
    };
    *state.core_state.write().await = payload.clone();
    if let Some(handle) = app_handle {
        let _ = handle.emit("ipc-coreStateChanged", json!([payload]));
        crate::desktop::tray::request_tray_menu_update(state, handle);
    }
}

pub async fn get_core_state(state: &Arc<AppState>) -> CoreStatePayload {
    state.core_state.read().await.clone()
}

pub async fn is_core_running(state: &Arc<AppState>) -> bool {
    let st = state.core_state.read().await;
    st.status == "running" && state.core_pid.load(Ordering::SeqCst) != 0
}

#[cfg(windows)]
fn apply_windows_cpu_priority(pid: u32, priority: &str) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, SetPriorityClass, ABOVE_NORMAL_PRIORITY_CLASS, BELOW_NORMAL_PRIORITY_CLASS,
        HIGH_PRIORITY_CLASS, IDLE_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS, PROCESS_SET_INFORMATION,
        REALTIME_PRIORITY_CLASS,
    };

    let class_flag = match priority {
        "realtime" => REALTIME_PRIORITY_CLASS,
        "high" => HIGH_PRIORITY_CLASS,
        "above_normal" => ABOVE_NORMAL_PRIORITY_CLASS,
        "below_normal" => BELOW_NORMAL_PRIORITY_CLASS,
        "idle" => IDLE_PRIORITY_CLASS,
        _ => NORMAL_PRIORITY_CLASS,
    };

    unsafe {
        let handle = OpenProcess(PROCESS_SET_INFORMATION, 0, pid);
        if !handle.is_null() {
            let _ = SetPriorityClass(handle, class_flag);
            let _ = CloseHandle(handle);
        }
    }
}

#[cfg(not(windows))]
fn apply_windows_cpu_priority(_pid: u32, _priority: &str) {}

/// Force-kill a process tree if the process is still alive.
/// Uses `taskkill /F /T /PID` on Windows for reliable tree termination.
#[cfg(windows)]
async fn kill_process_tree_if_alive(pid: u32) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE};
    // Check if process is still alive
    let alive = unsafe {
        let h = OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            false
        } else {
            CloseHandle(h);
            true
        }
    };
    if alive {
        let _ = tokio::process::Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .creation_flags(0x0800_0000)
            .output()
            .await;
    }
}

#[cfg(not(windows))]
async fn kill_process_tree_if_alive(_pid: u32) {}

/// Enumerate all running `mihomo.exe` processes whose executable path matches
/// our sidecar path, and terminate any that are not our currently managed PID.
/// Never touches processes from other VPN clients.
#[cfg(windows)]
pub async fn kill_orphan_cores(state: &Arc<AppState>) {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, TerminateProcess,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
    };

    let our_sidecar = match crate::utils::dirs::mihomo_core_path("mihomo") {
        Ok(p) => p.to_string_lossy().to_ascii_lowercase(),
        Err(_) => return,
    };
    let managed_pid = state.core_pid.load(Ordering::SeqCst);

    // Collect and kill orphans synchronously (no .await while holding raw handle)
    let killed_pids: Vec<u32> = unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot.is_null() {
            return;
        }
        let mut pids = Vec::new();
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                let name_len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(0);
                let name = OsString::from_wide(&entry.szExeFile[..name_len])
                    .to_string_lossy()
                    .to_ascii_lowercase();

                if name == "mihomo.exe" {
                    let pid = entry.th32ProcessID;
                    if pid != managed_pid || managed_pid == 0 {
                        let ph = OpenProcess(
                            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE,
                            0,
                            pid,
                        );
                        if !ph.is_null() {
                            let mut buf = [0u16; 1024];
                            let mut sz = buf.len() as u32;
                            if QueryFullProcessImageNameW(ph, 0, buf.as_mut_ptr(), &mut sz) != 0 {
                                let full_path = OsString::from_wide(&buf[..sz as usize])
                                    .to_string_lossy()
                                    .to_ascii_lowercase();
                                if full_path == our_sidecar || full_path.ends_with(r"\resources\sidecar\mihomo.exe") {
                                    TerminateProcess(ph, 1);
                                    pids.push(pid);
                                }
                            }
                            CloseHandle(ph);
                        }
                    }
                }
                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snapshot);
        pids
    };

    // Log kills asynchronously (safe to await now)
    for pid in killed_pids {
        let _ = append_core_log_line(&format!("[WARN] Killed orphan mihomo.exe (pid={pid})")).await;
    }
}

#[cfg(not(windows))]
pub async fn kill_orphan_cores(_state: &Arc<AppState>) {}

/// Create a Windows Job Object with KILL_ON_JOB_CLOSE and assign the child process to it.
/// Returns the raw job handle (as isize) to store in AppState.
#[cfg(windows)]
fn create_job_for_child(child_pid: u32) -> Option<isize> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_ALL_ACCESS};

    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return None;
        }

        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        ) == 0
        {
            CloseHandle(job);
            return None;
        }

        let ph = OpenProcess(PROCESS_ALL_ACCESS, 0, child_pid);
        if ph.is_null() {
            CloseHandle(job);
            return None;
        }

        let ok = AssignProcessToJobObject(job, ph);
        CloseHandle(ph);
        if ok == 0 {
            CloseHandle(job);
            return None;
        }

        Some(job as isize)
    }
}

#[cfg(not(windows))]
fn create_job_for_child(_child_pid: u32) -> Option<isize> {
    None
}

async fn append_core_log_line(line: &str) {
    let path = log_path();
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    if let Ok(mut f) = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .await
    {
        let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let _ = f.write_all(format!("[{ts}] [CORE] {line}\n").as_bytes()).await;
    }
}

async fn test_profile_config(core_path: &Path) -> Result<(), String> {
    let test_dir = mihomo_test_dir();
    let cfg_path = test_dir.join("config.yaml");
    if !cfg_path.exists() {
        return Ok(());
    }

    let mut cmd = Command::new(core_path);
    cmd.args([
        "-d",
        &test_dir.to_string_lossy(),
        "-f",
        &cfg_path.to_string_lossy(),
        "-t",
    ])
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());

    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);

    let out = cmd
        .output()
        .await
        .map_err(|e| format!("Failed testing config with mihomo -t: {e}"))?;

    if out.status.success() {
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let combined = format!("{stdout}\n{stderr}");
    let err_lines: Vec<&str> = combined
        .lines()
        .filter(|l| {
            let low = l.to_ascii_lowercase();
            low.contains("fata") || low.contains("error") || low.contains("failed")
        })
        .collect();
    let reason = if !err_lines.is_empty() {
        err_lines.join("\n")
    } else {
        combined.trim().to_string()
    };
    Err(if reason.is_empty() {
        "Mihomo configuration test (-t) failed".to_string()
    } else {
        reason
    })
}

async fn stop_core_inner(state: &Arc<AppState>, app_handle: Option<&AppHandle>) {
    stop_mihomo_streams(state).await;
    stop_network_detection(state).await;
    let pid = state.core_pid.swap(0, Ordering::SeqCst);

    // ── Service mode: ask service to stop first ──────────────────────────────
    let app_cfg = get_app_config(state, false).await.unwrap_or_default();
    if app_cfg.get("corePermissionMode").and_then(Value::as_str) == Some("service") {
        if let Err(e) = service_api_post(state, "/core/stop", None).await {
            let _ = append_core_log_line(&format!("[WARN] Service stop failed: {e}")).await;
        }
    }

    // ── Kill the tracked child process ───────────────────────────────────────
    let child_opt = state.core_child.lock().await.take();
    if let Some(mut child) = child_opt {
        let _ = child.kill().await;
        let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
    }

    // ── Verify the process is actually gone; if not, force-kill ─────────────
    if pid > 0 {
        kill_process_tree_if_alive(pid).await;
    }

    // ── Close Job Object (kills core if still somehow alive) ─────────────────
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        if let Some(raw_handle) = state.core_job.lock().await.take() {
            unsafe { CloseHandle(raw_handle as windows_sys::Win32::Foundation::HANDLE) };
        }
    }

    // Allow Windows OS to fully dismantle the named pipe and network adapter handles
    tokio::time::sleep(Duration::from_millis(300)).await;

    set_core_state(state, app_handle, "stopped", None).await;
}

pub async fn stop_core(
    state: &Arc<AppState>,
    app_handle: Option<&AppHandle>,
) -> Result<(), String> {
    let _guard = core_op_lock().lock().await;
    state.core_retry.store(0, Ordering::SeqCst);
    stop_core_inner(state, app_handle).await;
    Ok(())
}

pub async fn restart_core(
    state: &Arc<AppState>,
    app_handle: Option<&AppHandle>,
) -> Result<(), String> {
    let _guard = core_op_lock().lock().await;
    state.core_retry.store(0, Ordering::SeqCst);
    start_core_locked(state, app_handle).await
}

pub async fn start_core(
    state: &Arc<AppState>,
    app_handle: Option<&AppHandle>,
) -> Result<(), String> {
    let _guard = core_op_lock().lock().await;
    state.core_retry.store(0, Ordering::SeqCst);
    start_core_locked(state, app_handle).await
}

fn start_core_locked<'a>(
    state: &'a Arc<AppState>,
    app_handle: Option<&'a AppHandle>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>> {
    Box::pin(async move {
        stop_core_inner(state, app_handle).await;
        // Kill any orphaned mihomo.exe from previous sessions before starting
        kill_orphan_cores(state).await;
    set_core_state(state, app_handle, "starting", None).await;

    let app_cfg = get_app_config(state, false).await?;
    let core_name = app_cfg
        .get("core")
        .and_then(Value::as_str)
        .unwrap_or("mihomo")
        .to_string();

    let core_path = match mihomo_core_path(&core_name) {
        Ok(p) if p.exists() => p,
        Ok(p) => {
            let msg = format!("Core binary not found: {}", p.display());
            set_core_state(
                state,
                app_handle,
                "error",
                Some(CoreErrorPayload {
                    reason: "core_missing".into(),
                    message: msg.clone(),
                }),
            )
            .await;
            return Err(msg);
        }
        Err(e) => {
            set_core_state(
                state,
                app_handle,
                "error",
                Some(CoreErrorPayload {
                    reason: "core_invalid".into(),
                    message: e.clone(),
                }),
            )
            .await;
            return Err(e);
        }
    };

    let configured_log_level = match generate_profile(state, app_handle).await {
        Ok(lvl) => lvl,
        Err(e) => {
            set_core_state(
                state,
                app_handle,
                "error",
                Some(CoreErrorPayload {
                    reason: "profile_error".into(),
                    message: e.clone(),
                }),
            )
            .await;
            return Err(e);
        }
    };

    if let Err(e) = test_profile_config(&core_path).await {
        let _ = append_core_log_line(&format!("Config preflight failed: {e}")).await;
        set_core_state(
            state,
            app_handle,
            "error",
            Some(CoreErrorPayload {
                reason: "config_error".into(),
                message: e.clone(),
            }),
        )
        .await;
        return Err(e);
    }

    let use_service =
        app_cfg.get("corePermissionMode").and_then(Value::as_str) == Some("service")
            && test_service_connection(state).await.unwrap_or(false);

    if use_service {
        if let Err(e) = service_api_post(state, "/core/start", None).await {
            set_core_state(
                state,
                app_handle,
                "error",
                Some(CoreErrorPayload {
                    reason: "service_start_error".into(),
                    message: e.clone(),
                }),
            )
            .await;
            return Err(e);
        }
        state.core_pid.store(1, Ordering::SeqCst);
        on_core_ready(state, app_handle, configured_log_level).await;
        return Ok(());
    }

    let prof_cfg = get_profile_config(state, false).await.unwrap_or_default();
    let current_id = prof_cfg.get("current").and_then(Value::as_str);
    let diff_work_dir = app_cfg
        .get("diffWorkDir")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let work_dir = if diff_work_dir {
        mihomo_profile_work_dir(current_id)
    } else {
        mihomo_work_dir()
    };

    let mut cmd = Command::new(&core_path);
    cmd.args([
        "-d",
        &work_dir.to_string_lossy(),
        "-ext-ctl-pipe",
        mihomo_ipc_path(),
    ])
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());

    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);

    // Honor Mihomo environment overrides from app config
    if app_cfg
        .get("disableLoopbackDetector")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        cmd.env("DISABLE_LOOPBACK_DETECTOR", "true");
    }
    if app_cfg
        .get("disableEmbedCA")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        cmd.env("DISABLE_EMBED_CA", "true");
    }
    if app_cfg
        .get("disableSystemCA")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        cmd.env("DISABLE_SYSTEM_CA", "true");
    }
    if app_cfg
        .get("disableNftables")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        cmd.env("DISABLE_NFTABLES", "true");
    }
    if let Some(safe_paths) = app_cfg.get("safePaths").and_then(Value::as_array) {
        let joined: Vec<&str> = safe_paths.iter().filter_map(Value::as_str).collect();
        if !joined.is_empty() {
            let sep = if cfg!(windows) { ";" } else { ":" };
            cmd.env("SAFE_PATHS", joined.join(sep));
        }
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let msg = format!("Failed spawning mihomo core: {e}");
            set_core_state(
                state,
                app_handle,
                "error",
                Some(CoreErrorPayload {
                    reason: "spawn_error".into(),
                    message: msg.clone(),
                }),
            )
            .await;
            return Err(msg);
        }
    };

    let pid = child.id().unwrap_or(0);
    state.core_pid.store(pid, Ordering::SeqCst);
    if pid > 0 {
        let prio = app_cfg
            .get("mihomoCpuPriority")
            .and_then(Value::as_str)
            .unwrap_or("normal");
        apply_windows_cpu_priority(pid, prio);

        // Assign core to a Job Object so it dies with the client in all cases
        if let Some(job_handle) = create_job_for_child(pid) {
            *state.core_job.lock().await = Some(job_handle);
        }
    }

    let ready_notify = Arc::new(Notify::new());
    let fatal_error: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    if let Some(stdout) = child.stdout.take() {
        let rn = ready_notify.clone();
        let fe = fatal_error.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            let mut notified = false;
            while let Ok(Some(line)) = lines.next_line().await {
                append_core_log_line(&line).await;
                let low = line.to_ascii_lowercase();
                if (low.contains("controller pipe listen error") || low.contains("level=fatal")) && !notified {
                    let mut guard = fe.lock().await;
                    if guard.is_none() {
                        *guard = Some(line.clone());
                    }
                    notified = true;
                    rn.notify_one();
                } else if low.contains("level=error") && !notified {
                    let mut guard = fe.lock().await;
                    if guard.is_none() {
                        *guard = Some(line.clone());
                    }
                }
                if !notified
                    && (line.contains("RESTful API pipe listening at")
                        || line.contains("RESTful API unix listening at")
                        || line.contains("RESTful API listening at"))
                {
                    notified = true;
                    rn.notify_one();
                }
            }
        });
    }

    if let Some(stderr) = child.stderr.take() {
        let fe = fatal_error.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                append_core_log_line(&format!("[STDERR] {line}")).await;
                let mut guard = fe.lock().await;
                if guard.is_none() && !line.trim().is_empty() {
                    *guard = Some(line);
                }
            }
        });
    }

    *state.core_child.lock().await = Some(child);

    // Wait briefly for stdout signal OR successful Named Pipe /version handshake (up to 6s)
    let state_poll = state.clone();
    let pipe_ready = ready_notify.clone();
    tokio::spawn(async move {
        for _ in 0..40 {
            if state_poll.core_pid.load(Ordering::SeqCst) == 0 {
                return;
            }
            if mihomo_version().await.is_ok() {
                pipe_ready.notify_one();
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });

    let _ = tokio::time::timeout(Duration::from_secs(6), ready_notify.notified()).await;

    // Confirm process hasn't crashed immediately
    let still_alive = {
        let mut guard = state.core_child.lock().await;
        if let Some(ref mut ch) = *guard {
            match ch.try_wait() {
                Ok(Some(exit_status)) => {
                    *guard = None;
                    state.core_pid.store(0, Ordering::SeqCst);
                    let err_msg = fatal_error
                        .lock()
                        .await
                        .take()
                        .unwrap_or_else(|| format!("Mihomo exited early: {exit_status}"));
                    set_core_state(
                        state,
                        app_handle,
                        "error",
                        Some(CoreErrorPayload {
                            reason: "core_exit".into(),
                            message: err_msg.clone(),
                        }),
                    )
                    .await;
                    return Err(err_msg);
                }
                _ => true,
            }
        } else {
            false
        }
    };

    if !still_alive {
        return Err("Core failed to stay alive".to_string());
    }

    // Monitor unexpected exit & auto-restart up to 10 times unless stopped (matches Nullex retry=10)
    spawn_exit_watcher(state.clone(), app_handle.cloned(), pid);

    on_core_ready(state, app_handle, configured_log_level).await;
    Ok(())
    })
}

async fn on_core_ready(
    state: &Arc<AppState>,
    app_handle: Option<&AppHandle>,
    configured_log_level: String,
) {
    let _ = apply_log_level(state, app_handle, &configured_log_level).await;
    set_core_state(state, app_handle, "running", None).await;
    state.core_retry.store(0, Ordering::SeqCst);

    if let Some(handle) = app_handle {
        start_mihomo_streams(state, handle, configured_log_level).await;
        let _ = handle.emit("ipc-core-started", json!([]));
        let _ = handle.emit("ipc-groupsUpdated", json!([]));
        let _ = handle.emit("ipc-rulesUpdated", json!([]));
        crate::desktop::tray::request_tray_menu_update(state, handle);
    }

    let app_cfg = get_app_config(state, false).await.unwrap_or_default();
    let proxy_mode = app_cfg
        .get("proxyMode")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let sys_proxy_enabled = app_cfg
        .get("sysProxy")
        .and_then(|s| s.get("enable"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let only_active = app_cfg
        .get("onlyActiveDevice")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    if proxy_mode && sys_proxy_enabled {
        let _ = trigger_sys_proxy(state, true, only_active).await;
    }

    if app_cfg
        .get("networkDetection")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        start_network_detection(state, app_handle).await;
    }
}

fn spawn_exit_watcher(state: Arc<AppState>, app_handle: Option<AppHandle>, watched_pid: u32) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(800)).await;
            if state.is_quitting.load(Ordering::SeqCst)
                || state.core_pid.load(Ordering::SeqCst) != watched_pid
            {
                return;
            }
            let exited = {
                let mut guard = state.core_child.lock().await;
                if let Some(ref mut ch) = *guard {
                    match ch.try_wait() {
                        Ok(Some(_status)) => {
                            *guard = None;
                            true
                        }
                        _ => false,
                    }
                } else {
                    false
                }
            };
            if exited {
                if state.core_pid.load(Ordering::SeqCst) != watched_pid
                    || state.is_quitting.load(Ordering::SeqCst)
                {
                    return;
                }
                let prev = state.core_retry.fetch_add(1, Ordering::SeqCst);
                if prev < 10 {
                    let _ = append_core_log_line(&format!(
                        "Mihomo exited unexpectedly, auto-restarting ({}/10)...",
                        prev + 1
                    ))
                    .await;
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    let _ = restart_core(&state, app_handle.as_ref()).await;
                } else {
                    state.core_pid.store(0, Ordering::SeqCst);
                    set_core_state(
                        &state,
                        app_handle.as_ref(),
                        "error",
                        Some(CoreErrorPayload {
                            reason: "unexpected_exit".into(),
                            message: "Mihomo exited repeatedly (10 retries exceeded)".into(),
                        }),
                    )
                    .await;
                }
                return;
            }
        }
    });
}

pub fn start_network_detection<'a>(
    state: &'a Arc<AppState>,
    app_handle: Option<&'a AppHandle>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
    Box::pin(async move {
        stop_network_detection(state).await;
        let state_clone = state.clone();
        let app_clone = app_handle.cloned();
        let task = tokio::spawn(async move {
            let mut down_count = 0u32;
            loop {
                let app_cfg = get_app_config(&state_clone, false).await.unwrap_or_default();
                if !app_cfg
                    .get("networkDetection")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    break;
                }
                let interval_secs = app_cfg
                    .get("networkDetectionInterval")
                    .and_then(Value::as_u64)
                    .unwrap_or(10)
                    .max(3);
                let max_retries = app_cfg
                    .get("networkDetectionMaxRetries")
                    .and_then(Value::as_u64)
                    .unwrap_or(3) as u32;

                let online = tokio::net::TcpStream::connect("1.1.1.1:53").await.is_ok()
                    || tokio::net::TcpStream::connect("223.5.5.5:53").await.is_ok();

                if online {
                    if state_clone.network_down_handled.swap(false, Ordering::SeqCst) {
                        let _ = restart_core(&state_clone, app_clone.as_ref()).await;
                    }
                    down_count = 0;
                } else {
                    down_count += 1;
                    if down_count >= max_retries
                        && !state_clone.network_down_handled.swap(true, Ordering::SeqCst)
                    {
                        let _ = append_core_log_line(
                            "Network lost detected, pausing core until network restores",
                        )
                        .await;
                    }
                }

                tokio::time::sleep(Duration::from_secs(interval_secs)).await;
            }
        });
        *state.network_detect_abort.lock().await = Some(task.abort_handle());
    })
}

pub async fn stop_network_detection(state: &Arc<AppState>) {
    if let Some(abort) = state.network_detect_abort.lock().await.take() {
        abort.abort();
    }
}

pub async fn manual_grant_core_permission() -> Result<(), String> {
    if is_running_as_admin() {
        return Ok(());
    }
    Ok(())
}

pub async fn revoke_core_permission() -> Result<(), String> {
    Ok(())
}

pub fn check_core_permission() -> Value {
    json!({
        "mihomo": true,
        "mihomo-alpha": true
    })
}
