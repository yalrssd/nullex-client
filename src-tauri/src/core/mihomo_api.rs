use crate::config::app::get_app_config;
use crate::core::factory::{generate_profile, get_effective_log_level, get_runtime_config};
use crate::service::pipe_http::pipe_request;
use crate::state::AppState;
use crate::utils::dirs::mihomo_ipc_path;
use futures_util::future::join_all;
use futures_util::StreamExt;
use hyper::Method;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::{client_async, tungstenite::Message};

trait AsyncRwStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> AsyncRwStream for T {}

async fn open_mihomo_stream() -> Result<Box<dyn AsyncRwStream>, String> {
    #[cfg(windows)]
    {
        let client = crate::service::pipe_http::open_named_pipe(mihomo_ipc_path()).await?;
        Ok(Box::new(client))
    }
    #[cfg(not(windows))]
    {
        let stream = tokio::net::UnixStream::connect(mihomo_ipc_path())
            .await
            .map_err(|e| format!("Socket connect failed: {e}"))?;
        Ok(Box::new(stream))
    }
}

async fn mihomo_req(
    method: Method,
    path: &str,
    body: Option<Value>,
    timeout_secs: u64,
) -> Result<Value, String> {
    pipe_request(mihomo_ipc_path(), method, path, body, &[], timeout_secs)
        .await
        .map_err(|e| e.into_user_message())
}

pub async fn mihomo_version() -> Result<Value, String> {
    mihomo_req(Method::GET, "/version", None, 10).await
}

pub async fn mihomo_config() -> Result<Value, String> {
    mihomo_req(Method::GET, "/configs", None, 10).await
}

pub async fn patch_mihomo_config(patch: Value) -> Result<(), String> {
    let _ = mihomo_req(Method::PATCH, "/configs", Some(patch), 10).await?;
    Ok(())
}

pub async fn apply_log_level(
    state: &Arc<AppState>,
    app_handle: Option<&AppHandle>,
    log_level: &str,
) -> Result<(), String> {
    let target = if log_level.trim().is_empty() {
        get_effective_log_level(state).await
    } else {
        log_level.trim().to_string()
    };
    patch_mihomo_config(json!({ "log-level": target })).await?;
    if let Some(handle) = app_handle {
        restart_logs_stream(state, handle, target).await;
    }
    Ok(())
}

pub async fn mihomo_hot_reload_config(
    state: &Arc<AppState>,
    app_handle: Option<&AppHandle>,
) -> Result<(), String> {
    let configured_log_level = generate_profile(state, app_handle).await?;
    let app_cfg = get_app_config(state, false).await.unwrap_or_default();
    let diff_work_dir = app_cfg
        .get("diffWorkDir")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let prof_cfg = crate::config::profile::get_profile_config(state, false)
        .await
        .unwrap_or_default();
    let cur = prof_cfg.get("current").and_then(Value::as_str);
    let cfg_path = crate::utils::dirs::mihomo_work_config_path(if diff_work_dir {
        cur
    } else {
        Some("work")
    });

    let _ = mihomo_req(
        Method::PUT,
        "/configs?force=true",
        Some(json!({ "path": cfg_path.to_string_lossy().to_string() })),
        15,
    )
    .await?;
    let _ = apply_log_level(state, app_handle, &configured_log_level).await;
    if let Some(handle) = app_handle {
        let _ = handle.emit("ipc-groupsUpdated", json!([]));
        let _ = handle.emit("ipc-rulesUpdated", json!([]));
        crate::desktop::tray::request_tray_menu_update(state, handle);
    }
    Ok(())
}

pub async fn mihomo_proxies() -> Result<Value, String> {
    mihomo_req(Method::GET, "/proxies", None, 10).await
}

pub async fn mihomo_proxy_providers() -> Result<Value, String> {
    mihomo_req(Method::GET, "/providers/proxies", None, 10).await
}

pub async fn mihomo_rule_providers() -> Result<Value, String> {
    mihomo_req(Method::GET, "/providers/rules", None, 10).await
}

pub async fn mihomo_rules() -> Result<Value, String> {
    mihomo_req(Method::GET, "/rules", None, 10).await
}

pub async fn mihomo_update_proxy_providers(
    app_handle: Option<&AppHandle>,
    name: &str,
) -> Result<(), String> {
    let enc = urlencoding::encode(name);
    let _ = mihomo_req(
        Method::PUT,
        &format!("/providers/proxies/{enc}"),
        None,
        60,
    )
    .await?;
    if let Some(handle) = app_handle {
        let _ = handle.emit("ipc-groupsUpdated", json!([]));
    }
    Ok(())
}

pub async fn mihomo_update_rule_providers(
    app_handle: Option<&AppHandle>,
    name: &str,
) -> Result<(), String> {
    let enc = urlencoding::encode(name);
    let _ = mihomo_req(Method::PUT, &format!("/providers/rules/{enc}"), None, 60).await?;
    if let Some(handle) = app_handle {
        let _ = handle.emit("ipc-rulesUpdated", json!([]));
    }
    Ok(())
}

pub async fn mihomo_change_proxy(
    state: &Arc<AppState>,
    app_handle: Option<&AppHandle>,
    group: &str,
    proxy: &str,
) -> Result<(), String> {
    let enc = urlencoding::encode(group);
    let _ = mihomo_req(
        Method::PUT,
        &format!("/proxies/{enc}"),
        Some(json!({ "name": proxy })),
        10,
    )
    .await?;
    if let Some(handle) = app_handle {
        let _ = handle.emit("ipc-groupsUpdated", json!([]));
        crate::desktop::tray::request_tray_menu_update(state, handle);
    }
    Ok(())
}

pub async fn mihomo_unfixed_proxy(
    state: &Arc<AppState>,
    app_handle: Option<&AppHandle>,
    group: &str,
) -> Result<(), String> {
    let enc = urlencoding::encode(group);
    let _ = mihomo_req(Method::DELETE, &format!("/proxies/{enc}"), None, 10).await?;
    if let Some(handle) = app_handle {
        let _ = handle.emit("ipc-groupsUpdated", json!([]));
        crate::desktop::tray::request_tray_menu_update(state, handle);
    }
    Ok(())
}

fn is_tcp_delay_mode(app_cfg: &Value) -> bool {
    app_cfg
        .get("delayTestMode")
        .and_then(Value::as_str)
        .map(|s| s.eq_ignore_ascii_case("tcp"))
        .unwrap_or(false)
}

pub async fn mihomo_proxy_delay(
    state: &Arc<AppState>,
    proxy: &str,
    url: Option<&str>,
    provider: Option<&str>,
) -> Result<Value, String> {
    let app_cfg = get_app_config(state, false).await.unwrap_or_default();
    let timeout_ms = app_cfg
        .get("delayTestTimeout")
        .and_then(Value::as_u64)
        .filter(|&n| n > 0)
        .unwrap_or(5000);

    if is_tcp_delay_mode(&app_cfg) {
        return crate::core::tcping::tcp_proxy_delay(state, proxy, timeout_ms).await;
    }

    let test_url = url
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| app_cfg.get("delayTestUrl").and_then(Value::as_str))
        .unwrap_or("https://www.gstatic.com/generate_204");

    let enc_proxy = urlencoding::encode(proxy);
    let enc_url = urlencoding::encode(test_url);

    let path = if let Some(prov) = provider.map(str::trim).filter(|s| !s.is_empty()) {
        let enc_prov = urlencoding::encode(prov);
        format!(
            "/providers/proxies/{enc_prov}/{enc_proxy}/healthcheck?url={enc_url}&timeout={timeout_ms}"
        )
    } else {
        format!("/proxies/{enc_proxy}/delay?url={enc_url}&timeout={timeout_ms}")
    };

    let timeout_secs = (timeout_ms / 1000).max(5) + 5;
    mihomo_req(Method::GET, &path, None, timeout_secs).await
}

pub async fn mihomo_group_delay(
    state: &Arc<AppState>,
    group: &str,
    url: Option<&str>,
) -> Result<Value, String> {
    let app_cfg = get_app_config(state, false).await.unwrap_or_default();
    let timeout_ms = app_cfg
        .get("delayTestTimeout")
        .and_then(Value::as_u64)
        .filter(|&n| n > 0)
        .unwrap_or(5000);

    if is_tcp_delay_mode(&app_cfg) {
        let concurrency = app_cfg
            .get("delayTestConcurrency")
            .and_then(Value::as_u64)
            .filter(|&n| n > 0)
            .unwrap_or(50) as usize;
        return crate::core::tcping::tcp_group_delay(state, group, timeout_ms, concurrency).await;
    }

    let test_url = url
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| app_cfg.get("delayTestUrl").and_then(Value::as_str))
        .unwrap_or("https://www.gstatic.com/generate_204");

    let enc_group = urlencoding::encode(group);
    let enc_url = urlencoding::encode(test_url);
    let timeout_secs = (timeout_ms / 1000).max(5) + 10;
    mihomo_req(
        Method::GET,
        &format!("/group/{enc_group}/delay?url={enc_url}&timeout={timeout_ms}"),
        None,
        timeout_secs,
    )
    .await
}

pub async fn mihomo_close_connection(id: &str) -> Result<(), String> {
    let enc = urlencoding::encode(id);
    let _ = mihomo_req(Method::DELETE, &format!("/connections/{enc}"), None, 5).await?;
    Ok(())
}

pub async fn mihomo_close_all_connections(name: Option<&str>) -> Result<(), String> {
    let target = name.map(str::trim).filter(|s| !s.is_empty());
    match target {
        None => {
            let _ = mihomo_req(Method::DELETE, "/connections", None, 5).await?;
            Ok(())
        }
        Some(group_name) => {
            let conns = mihomo_req(Method::GET, "/connections", None, 5).await?;
            let Some(arr) = conns.get("connections").and_then(Value::as_array) else {
                return Ok(());
            };
            let ids: Vec<String> = arr
                .iter()
                .filter(|c| {
                    c.get("chains")
                        .and_then(Value::as_array)
                        .map(|chains| {
                            chains
                                .iter()
                                .any(|ch| ch.as_str() == Some(group_name))
                        })
                        .unwrap_or(false)
                })
                .filter_map(|c| c.get("id").and_then(Value::as_str).map(str::to_string))
                .collect();

            join_all(ids.into_iter().map(|id| async move {
                let _ = mihomo_close_connection(&id).await;
            }))
            .await;
            Ok(())
        }
    }
}

pub async fn mihomo_upgrade() -> Result<(), String> {
    let _ = mihomo_req(Method::POST, "/upgrade", None, 120).await?;
    Ok(())
}

pub async fn mihomo_upgrade_geo() -> Result<(), String> {
    let _ = mihomo_req(Method::POST, "/configs/geo", None, 120).await?;
    Ok(())
}

pub async fn mihomo_upgrade_ui() -> Result<(), String> {
    let _ = mihomo_req(Method::POST, "/upgrade/ui", None, 120).await?;
    Ok(())
}

fn ensure_history(mut obj: Value) -> Value {
    if let Some(map) = obj.as_object_mut() {
        if !map.get("history").map(Value::is_array).unwrap_or(false) {
            map.insert("history".into(), Value::Array(Vec::new()));
        }
    }
    obj
}

pub async fn mihomo_groups(state: &Arc<AppState>) -> Result<Value, String> {
    let (cfg_res, proxies_res, providers_res) = tokio::join!(
        mihomo_config(),
        mihomo_proxies(),
        mihomo_proxy_providers()
    );
    let proxies_root = proxies_res?;
    let mode = cfg_res
        .ok()
        .and_then(|c| c.get("mode").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_else(|| "rule".to_string());

    let Some(proxies_map) = proxies_root.get("proxies").and_then(Value::as_object) else {
        return Ok(Value::Array(Vec::new()));
    };

    let mut all_nodes: HashMap<String, Value> = HashMap::with_capacity(proxies_map.len() + 16);
    for (k, v) in proxies_map {
        all_nodes.insert(k.clone(), ensure_history(v.clone()));
    }

    if let Ok(prov_root) = providers_res {
        if let Some(prov_map) = prov_root.get("providers").and_then(Value::as_object) {
            for (prov_name, prov_val) in prov_map {
                if let Some(p_arr) = prov_val.get("proxies").and_then(Value::as_array) {
                    for p in p_arr {
                        if let Some(name) = p.get("name").and_then(Value::as_str) {
                            let mut node = ensure_history(p.clone());
                            if let Some(m) = node.as_object_mut() {
                                m.entry("provider-name".to_string())
                                    .or_insert_with(|| json!(prov_name));
                            }
                            all_nodes.entry(name.to_string()).or_insert(node);
                        }
                    }
                }
            }
        }
    }

    // Overlay TCP ping history when delayTestMode == "tcp" so UI never mixes URL & TCP latencies
    let tcp_mode = get_app_config(state, false)
        .await
        .map(|cfg| is_tcp_delay_mode(&cfg))
        .unwrap_or(false);
    if tcp_mode {
        let tcp_hist = state.tcp_delay_history.read().await;
        let now_map: HashMap<String, String> = all_nodes
            .iter()
            .filter_map(|(k, v)| {
                v.get("now")
                    .and_then(Value::as_str)
                    .map(|now| (k.clone(), now.to_string()))
            })
            .collect();
        for (name, node) in all_nodes.iter_mut() {
            let entries = tcp_hist
                .get(name)
                .or_else(|| now_map.get(name).and_then(|now| tcp_hist.get(now)))
                .cloned()
                .unwrap_or_default();
            if let Some(m) = node.as_object_mut() {
                m.insert("history".into(), Value::Array(entries));
            }
        }
    }

    let mut ordered_names: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    if mode == "global" && all_nodes.contains_key("GLOBAL") {
        ordered_names.push("GLOBAL".to_string());
        seen.insert("GLOBAL".to_string());
    }

    if let Some(global_all) = all_nodes
        .get("GLOBAL")
        .and_then(|g| g.get("all"))
        .and_then(Value::as_array)
    {
        for name_val in global_all {
            if let Some(name) = name_val.as_str() {
                if seen.contains(name) {
                    continue;
                }
                if let Some(candidate) = all_nodes.get(name) {
                    let is_group = candidate.get("all").map(Value::is_array).unwrap_or(false);
                    let is_hidden = candidate
                        .get("hidden")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    if is_group && !is_hidden {
                        seen.insert(name.to_string());
                        ordered_names.push(name.to_string());
                    }
                }
            }
        }
    }

    // Fallback to runtime config proxy-groups order if GLOBAL is absent
    if ordered_names.is_empty() {
        if let Ok(rt) = get_runtime_config(state).await {
            if let Some(pgs) = rt.get("proxy-groups").and_then(Value::as_array) {
                for pg in pgs {
                    if let Some(name) = pg.get("name").and_then(Value::as_str) {
                        if !seen.contains(name) && all_nodes.contains_key(name) {
                            seen.insert(name.to_string());
                            ordered_names.push(name.to_string());
                        }
                    }
                }
            }
        }
    }

    let mut out_groups: Vec<Value> = Vec::with_capacity(ordered_names.len());
    for gname in ordered_names {
        let Some(raw_group) = all_nodes.get(&gname).cloned() else {
            continue;
        };
        let mut group_obj = match raw_group {
            Value::Object(m) => m,
            _ => continue,
        };
        let member_names: Vec<String> = group_obj
            .get("all")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();

        let resolved_members: Vec<Value> = member_names
            .into_iter()
            .filter_map(|mname| {
                all_nodes.get(&mname).cloned().or_else(|| {
                    Some(json!({
                        "name": mname,
                        "type": "Unknown",
                        "history": []
                    }))
                })
            })
            .collect();

        group_obj.insert("all".into(), Value::Array(resolved_members));
        if !group_obj.get("history").map(Value::is_array).unwrap_or(false) {
            group_obj.insert("history".into(), Value::Array(Vec::new()));
        }
        out_groups.push(Value::Object(group_obj));
    }

    Ok(Value::Array(out_groups))
}

async fn run_ws_loop<F>(ws_path: String, mut on_text: F)
where
    F: FnMut(&str) + Send + 'static,
{
    let url = format!("ws://localhost{ws_path}");
    let mut backoff_ms = 200u64;
    loop {
        let stream_res = tokio::time::timeout(Duration::from_secs(3), open_mihomo_stream()).await;
        match stream_res {
            Ok(Ok(stream)) => {
                let ws_res = tokio::time::timeout(Duration::from_secs(3), client_async(&url, stream)).await;
                match ws_res {
                    Ok(Ok((mut ws, _))) => {
                        backoff_ms = 200;
                        while let Some(msg) = ws.next().await {
                            match msg {
                                Ok(Message::Text(txt)) => on_text(&txt),
                                Ok(Message::Binary(bin)) => {
                                    if let Ok(s) = std::str::from_utf8(&bin) {
                                        on_text(s);
                                    }
                                }
                                Ok(Message::Close(_)) | Err(_) => break,
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
        backoff_ms = (backoff_ms * 2).min(2000);
    }
}

fn spawn_connections_stream(app_handle: &AppHandle) -> tokio::task::AbortHandle {
    let app_c = app_handle.clone();
    let mut last_conn_emit = Instant::now() - Duration::from_millis(200);
    let connections_task = tokio::spawn(async move {
        run_ws_loop("/connections".to_string(), move |txt| {
            if last_conn_emit.elapsed() < Duration::from_millis(100) {
                return;
            }
            if let Ok(val) = serde_json::from_str::<Value>(txt) {
                last_conn_emit = Instant::now();
                let _ = app_c.emit("ipc-mihomoConnections", json!([val]));
            }
        })
        .await;
    });
    connections_task.abort_handle()
}

pub async fn set_mihomo_connections_wanted(
    state: &Arc<AppState>,
    app_handle: &AppHandle,
    wanted: bool,
) {
    let mut handles = state.streams.lock().await;
    handles.connections_wanted = wanted;
    if !wanted {
        if let Some(a) = handles.connections_abort.take() {
            a.abort();
        }
    } else if handles.connections_abort.is_none() {
        handles.connections_abort = Some(spawn_connections_stream(app_handle));
    }
}

pub async fn start_mihomo_streams(
    state: &Arc<AppState>,
    app_handle: &AppHandle,
    log_level: String,
) {
    stop_mihomo_streams(state).await;

    let app_t = app_handle.clone();
    let state_t = state.clone();
    let traffic_task = tokio::spawn(async move {
        run_ws_loop("/traffic".to_string(), move |txt| {
            if let Ok(val) = serde_json::from_str::<Value>(txt) {
                let up = val.get("up").and_then(Value::as_u64).unwrap_or(0);
                let down = val.get("down").and_then(Value::as_u64).unwrap_or(0);
                let _ = app_t.emit("ipc-mihomoTraffic", json!([val]));
                crate::desktop::tray::update_tray_traffic_tooltip(&state_t, &app_t, up, down);
            }
        })
        .await;
    });

    let conn_abort = {
        let handles = state.streams.lock().await;
        if handles.connections_wanted {
            Some(spawn_connections_stream(app_handle))
        } else {
            None
        }
    };

    let logs_task = spawn_logs_stream(app_handle, &log_level);

    let mut handles = state.streams.lock().await;
    handles.traffic_abort = Some(traffic_task.abort_handle());
    handles.memory_abort = None;
    if let Some(ca) = conn_abort {
        handles.connections_abort = Some(ca);
    }
    handles.logs_abort = Some(logs_task);
    handles.logs_level = log_level;
}

fn spawn_logs_stream(app_handle: &AppHandle, level: &str) -> tokio::task::AbortHandle {
    let app_l = app_handle.clone();
    let lvl = if level.trim().is_empty() {
        "info".to_string()
    } else {
        level.trim().to_string()
    };
    let task = tokio::spawn(async move {
        run_ws_loop(format!("/logs?level={lvl}"), move |txt| {
            if let Ok(mut val) = serde_json::from_str::<Value>(txt) {
                if let Some(obj) = val.as_object_mut() {
                    if !obj.contains_key("time") {
                        obj.insert(
                            "time".into(),
                            json!(chrono::Local::now().format("%H:%M:%S").to_string()),
                        );
                    }
                }
                let _ = app_l.emit("ipc-mihomoLogs", json!([val]));
            }
        })
        .await;
    });
    task.abort_handle()
}

pub async fn restart_logs_stream(
    state: &Arc<AppState>,
    app_handle: &AppHandle,
    log_level: String,
) {
    let mut handles = state.streams.lock().await;
    if let Some(abort) = handles.logs_abort.take() {
        abort.abort();
    }
    handles.logs_abort = Some(spawn_logs_stream(app_handle, &log_level));
    handles.logs_level = log_level;
}

pub async fn stop_mihomo_streams(state: &Arc<AppState>) {
    let mut handles = state.streams.lock().await;
    if let Some(a) = handles.traffic_abort.take() {
        a.abort();
    }
    if let Some(a) = handles.memory_abort.take() {
        a.abort();
    }
    if let Some(a) = handles.logs_abort.take() {
        a.abort();
    }
    if let Some(a) = handles.connections_abort.take() {
        a.abort();
    }
}

pub async fn restart_mihomo_connections(state: &Arc<AppState>, app_handle: &AppHandle) {
    let mut handles = state.streams.lock().await;
    if let Some(a) = handles.connections_abort.take() {
        a.abort();
    }
    if handles.connections_wanted {
        handles.connections_abort = Some(spawn_connections_stream(app_handle));
    }
}
