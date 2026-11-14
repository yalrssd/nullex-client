use crate::config::app::get_app_config;
use crate::config::mihomo::get_controled_mihomo_config;
use crate::config::profile::{get_current_profile_item, get_profile_config, get_profile_str};
use crate::config::template::default_profile;
use crate::state::AppState;
use crate::utils::dirs::{
    mihomo_profile_work_dir, mihomo_test_dir, mihomo_work_config_path, mihomo_work_dir, rule_path,
};
use crate::utils::geo::link_geo_files;
use crate::utils::merge::deep_merge;
use crate::utils::yaml::{parse_yaml_to_json, stringify_json_to_yaml};
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::sync::Arc;
use tauri::AppHandle;

fn port_val(cfg: &Value, key: &str) -> u16 {
    cfg.get(key)
        .and_then(Value::as_u64)
        .filter(|&n| n > 0 && n <= u16::MAX as u64)
        .map(|n| n as u16)
        .unwrap_or(0)
}

pub async fn get_effective_ports(state: &Arc<AppState>) -> Value {
    let app_cfg = get_app_config(state, false).await.unwrap_or_default();
    let mihomo_cfg = get_controled_mihomo_config(state, false)
        .await
        .unwrap_or_default();

    let proxy_mode = app_cfg
        .get("proxyMode")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let tun_enable = mihomo_cfg
        .get("tun")
        .and_then(|t| t.get("enable"))
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let is_active = proxy_mode || tun_enable;

    let port = port_val(&mihomo_cfg, "port");
    let socks_port = port_val(&mihomo_cfg, "socks-port");
    let redir_port = port_val(&mihomo_cfg, "redir-port");
    let tproxy_port = port_val(&mihomo_cfg, "tproxy-port");
    let mixed_port = {
        let p = port_val(&mihomo_cfg, "mixed-port");
        if p == 0 && is_active {
            7897
        } else {
            p
        }
    };

    let p_info = |val: u16| {
        json!({
            "value": if is_active { val } else { 0 },
            "profileValue": null,
            "fromProfile": false
        })
    };

    json!({
        "port": p_info(port),
        "socks-port": p_info(socks_port),
        "redir-port": p_info(redir_port),
        "tproxy-port": p_info(tproxy_port),
        "mixed-port": p_info(mixed_port),
        "portNum": if is_active { port } else { 0 },
        "socksPort": if is_active { socks_port } else { 0 },
        "redirPort": if is_active { redir_port } else { 0 },
        "tproxyPort": if is_active { tproxy_port } else { 0 },
        "mixedPort": if is_active { mixed_port } else { 0 }
    })
}

pub async fn get_effective_log_level(state: &Arc<AppState>) -> String {
    let mihomo_cfg = get_controled_mihomo_config(state, false)
        .await
        .unwrap_or_default();
    mihomo_cfg
        .get("log-level")
        .and_then(Value::as_str)
        .unwrap_or("info")
        .to_string()
}

fn startup_log_level(configured: &str) -> String {
    match configured {
        "debug" | "info" => configured.to_string(),
        _ => "info".to_string(),
    }
}

fn apply_rule_overrides(profile: &mut Value, profile_id: &str) {
    let r_path = rule_path(profile_id);
    if !r_path.exists() {
        return;
    }
    let Ok(raw) = std::fs::read_to_string(&r_path) else {
        return;
    };
    let Ok(rule_val) = parse_yaml_to_json(&raw) else {
        return;
    };

    let Some(p_obj) = profile.as_object_mut() else {
        return;
    };

    if let Some(arr) = rule_val.as_array() {
        p_obj.insert("rules".into(), Value::Array(arr.clone()));
        return;
    }

    let Some(r_obj) = rule_val.as_object() else {
        return;
    };

    let mut existing: Vec<Value> = p_obj
        .get("rules")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    if let Some(deletes) = r_obj.get("delete").and_then(Value::as_array) {
        let del_set: HashSet<String> = deletes
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
        existing.retain(|v| !v.as_str().map(|s| del_set.contains(s)).unwrap_or(false));
    }

    let prepend = r_obj
        .get("prepend")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let append = r_obj
        .get("append")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut combined = Vec::with_capacity(prepend.len() + existing.len() + append.len());
    combined.extend(prepend);
    combined.extend(existing);
    combined.extend(append);
    p_obj.insert("rules".into(), Value::Array(combined));
}

fn clean_runtime_profile(val: &mut Value) {
    let Some(obj) = val.as_object_mut() else {
        return;
    };

    // Strip zero ports and empty keep-alive integers
    for key in [
        "port",
        "socks-port",
        "redir-port",
        "tproxy-port",
        "mixed-port",
        "keep-alive-idle",
        "keep-alive-interval",
    ] {
        if obj.get(key).and_then(Value::as_u64) == Some(0) {
            obj.remove(key);
        }
    }

    // Strip blank string fields that aren't needed when empty
    for key in [
        "external-controller",
        "external-ui",
        "secret",
        "interface-name",
        "global-client-fingerprint",
    ] {
        if obj
            .get(key)
            .and_then(Value::as_str)
            .map(|s| s.trim().is_empty())
            .unwrap_or(false)
        {
            obj.remove(key);
        }
    }

    // Normalize global-client-fingerprint if present
    if let Some(gfp) = obj.get_mut("global-client-fingerprint") {
        if let Some(s) = gfp.as_str() {
            let normalized = crate::utils::sub_converter::normalize_client_fingerprint(s);
            *gfp = json!(normalized);
        }
    }

    // Normalize client-fingerprint on all proxies in 'proxies' array
    if let Some(proxies) = obj.get_mut("proxies").and_then(Value::as_array_mut) {
        for proxy in proxies {
            if let Some(p_obj) = proxy.as_object_mut() {
                if let Some(fp_val) = p_obj.get_mut("client-fingerprint") {
                    if let Some(s) = fp_val.as_str() {
                        let normalized =
                            crate::utils::sub_converter::normalize_client_fingerprint(s);
                        *fp_val = json!(normalized);
                    }
                }
            }
        }
    }
}

pub async fn generate_profile(
    state: &Arc<AppState>,
    _app_handle: Option<&AppHandle>,
) -> Result<String, String> {
    let app_cfg = get_app_config(state, false).await?;
    let mihomo_cfg = get_controled_mihomo_config(state, false).await?;
    let current_item = get_current_profile_item(state).await?;
    let current_id = current_item
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("default")
        .to_string();

    let raw_str = get_profile_str(&current_id).await.unwrap_or_default();
    *state.raw_profile_str.write().await = raw_str.clone();
    *state.current_profile_str.write().await = raw_str.clone();

    let mut profile = if raw_str.trim().is_empty() {
        default_profile()
    } else {
        parse_yaml_to_json(&raw_str).unwrap_or_else(|_| default_profile())
    };
    if !profile.is_object() {
        profile = default_profile();
    }

    apply_rule_overrides(&mut profile, &current_id);

    let mut controlled = mihomo_cfg.clone();
    let control_dns = app_cfg
        .get("controlDns")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let control_sniff = app_cfg
        .get("controlSniff")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let control_tun = app_cfg
        .get("controlTun")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    if let Some(c_obj) = controlled.as_object_mut() {
        if !control_dns {
            c_obj.remove("dns");
            c_obj.remove("hosts");
        }
        if !control_sniff {
            c_obj.remove("sniffer");
        }
        if !control_tun {
            let tun_enable = c_obj
                .get("tun")
                .and_then(|t| t.get("enable"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let route_exclude = c_obj
                .get("tun")
                .and_then(|t| t.get("route-exclude-address"))
                .cloned();
            let mut mini_tun = Map::new();
            mini_tun.insert("enable".into(), json!(tun_enable));
            if let Some(re) = route_exclude {
                if !re.is_null() {
                    mini_tun.insert("route-exclude-address".into(), re);
                }
            }
            c_obj.insert("tun".into(), Value::Object(mini_tun));
        }

        // Apply effective ports based on proxyMode + tun.enable
        let ports = get_effective_ports(state).await;
        let p_port = ports["port"].as_u64().unwrap_or(0);
        let p_socks = ports["socksPort"].as_u64().unwrap_or(0);
        let p_redir = ports["redirPort"].as_u64().unwrap_or(0);
        let p_tproxy = ports["tproxyPort"].as_u64().unwrap_or(0);
        let p_mixed = ports["mixedPort"].as_u64().unwrap_or(0);

        c_obj.insert("port".into(), json!(p_port));
        c_obj.insert("socks-port".into(), json!(p_socks));
        c_obj.insert("redir-port".into(), json!(p_redir));
        c_obj.insert("tproxy-port".into(), json!(p_tproxy));
        c_obj.insert("mixed-port".into(), json!(p_mixed));

        *state.last_mixed_port.lock().await = if p_mixed > 0 && p_mixed <= u16::MAX as u64 {
            Some(p_mixed as u16)
        } else {
            None
        };
    }

    let configured_log_level = get_effective_log_level(state).await;
    deep_merge(&mut profile, &controlled);

    // Apply hosts from appConfig if present
    if control_dns {
        if let Some(hosts_arr) = app_cfg.get("hosts").and_then(Value::as_array) {
            if !hosts_arr.is_empty() {
                let mut hosts_map = Map::new();
                for item in hosts_arr {
                    if let (Some(domain), Some(val)) = (
                        item.get("domain").and_then(Value::as_str),
                        item.get("value").cloned(),
                    ) {
                        if !domain.trim().is_empty() {
                            hosts_map.insert(domain.trim().to_string(), val);
                        }
                    }
                }
                if !hosts_map.is_empty() {
                    if let Some(p_obj) = profile.as_object_mut() {
                        p_obj.insert("hosts".into(), Value::Object(hosts_map));
                    }
                }
            }
        }
    }

    // Override log-level during startup so initialization stdout prints cleanly
    if let Some(p_obj) = profile.as_object_mut() {
        p_obj.insert(
            "log-level".into(),
            json!(startup_log_level(&configured_log_level)),
        );
    }

    clean_runtime_profile(&mut profile);

    let yaml = stringify_json_to_yaml(&profile)?;
    *state.runtime_config.write().await = Some(profile);
    *state.runtime_config_str.write().await = yaml.clone();

    let diff_work_dir = app_cfg
        .get("diffWorkDir")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let prof_cfg = get_profile_config(state, false).await.unwrap_or_default();
    let cur = prof_cfg.get("current").and_then(Value::as_str);

    let (work_dir, work_cfg_path) = if diff_work_dir {
        (
            mihomo_profile_work_dir(cur),
            mihomo_work_config_path(cur),
        )
    } else {
        (mihomo_work_dir(), mihomo_work_config_path(Some("work")))
    };

    tokio::fs::create_dir_all(&work_dir)
        .await
        .map_err(|e| format!("Failed creating mihomo work dir: {e}"))?;
    link_geo_files(&work_dir).await;

    tokio::fs::write(&work_cfg_path, &yaml)
        .await
        .map_err(|e| format!("Failed writing runtime config.yaml: {e}"))?;

    // Also prepare test dir config for `-t` preflight checks
    let test_dir = mihomo_test_dir();
    let _ = tokio::fs::create_dir_all(&test_dir).await;
    link_geo_files(&test_dir).await;
    let _ = tokio::fs::write(test_dir.join("config.yaml"), &yaml).await;

    Ok(configured_log_level)
}

pub async fn get_runtime_config(state: &Arc<AppState>) -> Result<Value, String> {
    if let Some(cached) = state.runtime_config.read().await.clone() {
        return Ok(cached);
    }
    let _ = generate_profile(state, None).await?;
    Ok(state
        .runtime_config
        .read()
        .await
        .clone()
        .unwrap_or_else(default_profile))
}

pub async fn get_runtime_config_str(state: &Arc<AppState>) -> Result<String, String> {
    let cached = state.runtime_config_str.read().await.clone();
    if !cached.is_empty() {
        return Ok(cached);
    }
    let _ = generate_profile(state, None).await?;
    Ok(state.runtime_config_str.read().await.clone())
}

pub async fn get_raw_profile_str(state: &Arc<AppState>) -> Result<String, String> {
    let cached = state.raw_profile_str.read().await.clone();
    if !cached.is_empty() {
        return Ok(cached);
    }
    let item = get_current_profile_item(state).await?;
    let id = item.get("id").and_then(Value::as_str).unwrap_or("default");
    get_profile_str(id).await
}

pub async fn get_current_profile_str(state: &Arc<AppState>) -> Result<String, String> {
    let cached = state.current_profile_str.read().await.clone();
    if !cached.is_empty() {
        return Ok(cached);
    }
    get_raw_profile_str(state).await
}
