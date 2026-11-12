use crate::config::app::get_app_config;
use crate::config::template::default_controled_mihomo_config;
use crate::core::factory::generate_profile;
use crate::core::mihomo_api::{apply_log_level, patch_mihomo_config};
use crate::state::AppState;
use crate::utils::dirs::controled_mihomo_config_path;
use crate::utils::merge::deep_merge;
use crate::utils::yaml::{parse_yaml_to_json, stringify_json_to_yaml};
use serde_json::{json, Value};
use std::sync::Arc;

pub async fn get_controled_mihomo_config(
    state: &Arc<AppState>,
    force: bool,
) -> Result<Value, String> {
    if !force {
        if let Some(cached) = state.mihomo_config.read().await.clone() {
            return Ok(cached);
        }
    }

    let loaded = match tokio::fs::read_to_string(controled_mihomo_config_path()).await {
        Ok(raw) => match parse_yaml_to_json(&raw) {
            Ok(val) if val.is_object() => val,
            _ => default_controled_mihomo_config(),
        },
        Err(_) => default_controled_mihomo_config(),
    };

    *state.mihomo_config.write().await = Some(loaded.clone());
    Ok(loaded)
}

pub async fn patch_controled_mihomo_config(
    state: &Arc<AppState>,
    app_handle: Option<&tauri::AppHandle>,
    patch: Value,
) -> Result<(), String> {
    let mut cfg = get_controled_mihomo_config(state, false).await?;
    let mut patch_to_merge = patch.clone();
    let app_cfg = get_app_config(state, false).await?;

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

    let defaults = default_controled_mihomo_config();
    if let Some(map) = cfg.as_object_mut() {
        if !control_dns {
            map.remove("dns");
            map.remove("hosts");
        } else if map.get("dns").and_then(|d| d.get("ipv6")).is_none() {
            map.insert("dns".into(), defaults["dns"].clone());
        }

        if !control_sniff {
            map.remove("sniffer");
        } else if !map.contains_key("sniffer") {
            map.insert("sniffer".into(), defaults["sniffer"].clone());
        }

        if !control_tun {
            let prev_tun_enable = map
                .get("tun")
                .and_then(|t| t.get("enable"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let next_tun_enable = patch_to_merge
                .get("tun")
                .and_then(|t| t.get("enable"))
                .and_then(Value::as_bool)
                .unwrap_or(prev_tun_enable);
            let route_exclude = map
                .get("tun")
                .and_then(|t| t.get("route-exclude-address"))
                .cloned()
                .unwrap_or(Value::Null);

            let mut tun_obj = serde_json::Map::new();
            tun_obj.insert("enable".into(), json!(next_tun_enable));
            if !route_exclude.is_null() {
                tun_obj.insert("route-exclude-address".into(), route_exclude);
            }
            map.insert("tun".into(), Value::Object(tun_obj));

            if let Some(p_map) = patch_to_merge.as_object_mut() {
                if p_map.contains_key("tun") {
                    p_map.insert("tun".into(), json!({ "enable": next_tun_enable }));
                }
            }
        } else if !map.contains_key("tun") {
            map.insert("tun".into(), defaults["tun"].clone());
        }

        if let Some(policy) = patch_to_merge
            .get("dns")
            .and_then(|d| d.get("nameserver-policy"))
            .cloned()
        {
            let dns_entry = map.entry("dns").or_insert_with(|| json!({}));
            if let Some(dm) = dns_entry.as_object_mut() {
                dm.insert("nameserver-policy".into(), policy);
            }
        }
        if patch_to_merge
            .get("dns")
            .and_then(|d| d.get("use-hosts"))
            .and_then(Value::as_bool)
            == Some(true)
        {
            if let Some(hosts) = patch_to_merge.get("hosts").cloned() {
                map.insert("hosts".into(), hosts);
            }
        }
    }

    deep_merge(&mut cfg, &patch_to_merge);
    *state.mihomo_config.write().await = Some(cfg.clone());

    let log_level = generate_profile(state, app_handle).await?;
    let yaml = stringify_json_to_yaml(&cfg)?;
    tokio::fs::write(controled_mihomo_config_path(), yaml)
        .await
        .map_err(|e| format!("Failed writing mihomo.yaml: {e}"))?;

    if let Some(mut rest) = patch.as_object().cloned() {
        let patched_log = rest.remove("log-level");
        if !rest.is_empty() {
            let _ = patch_mihomo_config(Value::Object(rest)).await;
        }
        if patched_log.is_some() {
            let _ = apply_log_level(state, app_handle, &log_level).await;
        }
    }
    Ok(())
}
