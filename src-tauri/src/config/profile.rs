use crate::config::app::get_app_config;
use crate::config::mihomo::get_controled_mihomo_config;
use crate::config::template::{default_profile, default_profile_config};
use crate::core::manager::restart_core;
use crate::core::profile_updater::{add_profile_updater, remove_profile_updater};
use crate::state::AppState;
use crate::utils::device_info::{get_device_model, get_device_os, get_hwid, get_os_version};
use crate::utils::dirs::{
    data_dir, mihomo_core_path, mihomo_profile_work_dir, mihomo_work_dir, profile_config_path,
    profile_path, profiles_dir, rule_path, rules_dir,
};
use crate::utils::i18n::tr;
use crate::utils::sub_converter::{
    is_share_link_input, is_valid_mihomo_object, normalize_subscription_to_mihomo_yaml,
};
use crate::utils::yaml::{parse_yaml_to_json, stringify_json_to_yaml};
use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::Utc;
use rand::distributions::Alphanumeric;
use rand::Rng;
use reqwest::{header::HeaderMap, Client, Proxy};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::process::Command;
use url::Url;

const FALLBACK_USER_AGENTS: [&str; 3] = ["clash.meta/1.19.0", "v2rayN/6.60", "Happ/2.0"];

pub fn generate_profile_id() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(12)
        .map(char::from)
        .collect::<String>()
        .to_lowercase()
}

pub fn get_default_user_agent() -> String {
    format!("Nullex/{}", env!("CARGO_PKG_VERSION"))
}

pub async fn get_profile_config(state: &Arc<AppState>, force: bool) -> Result<Value, String> {
    if !force {
        if let Some(cached) = state.profile_config.read().await.clone() {
            return Ok(cached);
        }
    }

    let path = profile_config_path();
    let mut cfg = match tokio::fs::read_to_string(&path).await {
        Ok(raw) => match parse_yaml_to_json(&raw) {
            Ok(v) if v.is_object() => v,
            _ => default_profile_config(),
        },
        Err(_) => default_profile_config(),
    };

    if let Some(obj) = cfg.as_object_mut() {
        if !obj.get("items").map(Value::is_array).unwrap_or(false) {
            obj.insert("items".into(), Value::Array(Vec::new()));
        }
    }

    *state.profile_config.write().await = Some(cfg.clone());
    Ok(cfg)
}

pub async fn set_profile_config(
    state: &Arc<AppState>,
    app_handle: Option<&AppHandle>,
    mut cfg: Value,
) -> Result<(), String> {
    if !cfg.is_object() {
        cfg = default_profile_config();
    }
    if let Some(obj) = cfg.as_object_mut() {
        if !obj.get("items").map(Value::is_array).unwrap_or(false) {
            obj.insert("items".into(), Value::Array(Vec::new()));
        }
    }

    let yaml = stringify_json_to_yaml(&cfg)?;
    let path = profile_config_path();
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let tmp = path.with_extension("yaml.tmp");
    tokio::fs::write(&tmp, yaml)
        .await
        .map_err(|e| format!("Failed writing profile config: {e}"))?;
    if cfg!(windows) && path.exists() {
        let _ = tokio::fs::remove_file(&path).await;
    }
    tokio::fs::rename(&tmp, &path)
        .await
        .map_err(|e| format!("Failed replacing profile config: {e}"))?;

    *state.profile_config.write().await = Some(cfg);

    if let Some(handle) = app_handle {
        let _ = handle.emit("ipc-profileConfigUpdated", json!([]));
        crate::desktop::tray::request_tray_menu_update(state, handle);
    }
    Ok(())
}

pub async fn get_profile_item(state: &Arc<AppState>, id: Option<&str>) -> Result<Option<Value>, String> {
    let cfg = get_profile_config(state, false).await?;
    let target_id = id
        .filter(|s| !s.is_empty())
        .or_else(|| cfg.get("current").and_then(Value::as_str))
        .unwrap_or("");
    if target_id.is_empty() {
        return Ok(None);
    }
    let found = cfg
        .get("items")
        .and_then(Value::as_array)
        .and_then(|arr| {
            arr.iter()
                .find(|item| item.get("id").and_then(Value::as_str) == Some(target_id))
                .cloned()
        });
    Ok(found)
}

pub async fn get_current_profile_item(state: &Arc<AppState>) -> Result<Value, String> {
    let cfg = get_profile_config(state, false).await?;
    if let Some(item) = get_profile_item(state, None).await? {
        return Ok(item);
    }
    let fallback_id = cfg
        .get("current")
        .and_then(Value::as_str)
        .unwrap_or("default");
    Ok(json!({
        "id": fallback_id,
        "type": "local",
        "name": "Blank"
    }))
}

pub async fn change_current_profile(
    state: &Arc<AppState>,
    app_handle: Option<&AppHandle>,
    id: &str,
) -> Result<(), String> {
    let new_item = {
        let _guard = state.profile_lock.lock().await;
        let mut cfg = get_profile_config(state, true).await?;
        let exists = cfg
            .get("items")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .any(|it| it.get("id").and_then(Value::as_str) == Some(id))
            })
            .unwrap_or(false);
        if !exists && !id.is_empty() {
            return Err(format!("Profile '{id}' not found"));
        }

        if let Some(obj) = cfg.as_object_mut() {
            if id.is_empty() {
                obj.remove("current");
            } else {
                obj.insert("current".into(), Value::String(id.to_string()));
            }
        }
        set_profile_config(state, app_handle, cfg.clone()).await?;
        get_profile_item(state, Some(id)).await?
    };

    if let Some(item) = new_item {
        add_profile_updater(state, app_handle, &item).await;
    }

    restart_core(state, app_handle).await?;
    Ok(())
}

pub async fn add_profile_item(
    state: &Arc<AppState>,
    app_handle: Option<&AppHandle>,
    item: Value,
) -> Result<(), String> {
    let created = create_profile(state, app_handle, item).await?;
    let id = created
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "Missing profile id".to_string())?
        .to_string();

    let should_restart = {
        let _guard = state.profile_lock.lock().await;
        let mut cfg = get_profile_config(state, true).await?;
        let mut restart_needed = false;

        if let Some(obj) = cfg.as_object_mut() {
            let current = obj
                .get("current")
                .and_then(Value::as_str)
                .map(str::to_string)
                .filter(|s| !s.is_empty());

            let items = obj
                .entry("items")
                .or_insert_with(|| Value::Array(Vec::new()))
                .as_array_mut()
                .ok_or_else(|| "Invalid items list".to_string())?;

            if let Some(pos) = items
                .iter()
                .position(|it| it.get("id").and_then(Value::as_str) == Some(id.as_str()))
            {
                items[pos] = created.clone();
            } else {
                items.push(created.clone());
            }

            if current.is_none() {
                obj.insert("current".into(), Value::String(id.clone()));
                restart_needed = true;
            } else if current.as_deref() == Some(id.as_str()) {
                restart_needed = true;
            }
        }

        set_profile_config(state, app_handle, cfg).await?;
        restart_needed
    };

    add_profile_updater(state, app_handle, &created).await;
    if should_restart {
        restart_core(state, app_handle).await?;
    }
    Ok(())
}

pub async fn update_profile_item(
    state: &Arc<AppState>,
    app_handle: Option<&AppHandle>,
    item: Value,
) -> Result<(), String> {
    let id = item
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "Profile item missing id".to_string())?
        .to_string();

    let updated_item = {
        let _guard = state.profile_lock.lock().await;
        let mut cfg = get_profile_config(state, true).await?;
        let items = cfg
            .get_mut("items")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| "Invalid profile items".to_string())?;

        let pos = items
            .iter()
            .position(|it| it.get("id").and_then(Value::as_str) == Some(id.as_str()))
            .ok_or_else(|| format!("Profile '{id}' not found"))?;

        let mut merged = items[pos].clone();
        if let (Some(dst), Some(src)) = (merged.as_object_mut(), item.as_object()) {
            for (k, v) in src {
                dst.insert(k.clone(), v.clone());
            }
        } else {
            merged = item;
        }
        items[pos] = merged.clone();
        set_profile_config(state, app_handle, cfg).await?;
        merged
    };

    add_profile_updater(state, app_handle, &updated_item).await;
    Ok(())
}

pub async fn remove_profile_item(
    state: &Arc<AppState>,
    app_handle: Option<&AppHandle>,
    id: &str,
) -> Result<(), String> {
    let was_current = {
        let _guard = state.profile_lock.lock().await;
        let mut cfg = get_profile_config(state, true).await?;
        let current = cfg
            .get("current")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_default();

        if let Some(items) = cfg.get_mut("items").and_then(Value::as_array_mut) {
            items.retain(|it| it.get("id").and_then(Value::as_str) != Some(id));
        }

        let is_current = current == id;
        if is_current {
            let next_id = cfg
                .get("items")
                .and_then(Value::as_array)
                .and_then(|arr| arr.first())
                .and_then(|it| it.get("id"))
                .and_then(Value::as_str)
                .map(str::to_string);

            if let Some(obj) = cfg.as_object_mut() {
                if let Some(next) = next_id {
                    obj.insert("current".into(), Value::String(next));
                } else {
                    obj.remove("current");
                }
            }
        }

        set_profile_config(state, app_handle, cfg).await?;
        is_current
    };

    remove_profile_updater(id).await;
    let p_file = profile_path(id);
    if p_file.exists() {
        let _ = tokio::fs::remove_file(&p_file).await;
    }
    let work_dir = mihomo_profile_work_dir(Some(id));
    if work_dir.exists() {
        let _ = tokio::fs::remove_dir_all(&work_dir).await;
    }

    if was_current {
        restart_core(state, app_handle).await?;
    }
    Ok(())
}

fn decode_header_value(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed
        .strip_prefix("base64:")
        .or_else(|| trimmed.strip_prefix("BASE64:"))
    {
        let normalized = rest.trim().replace('-', "+").replace('_', "/");
        let pad = (4 - (normalized.len() % 4)) % 4;
        let padded = format!("{normalized}{}", "=".repeat(pad));
        if let Ok(bytes) = STANDARD.decode(padded.as_bytes()) {
            if let Ok(s) = String::from_utf8(bytes) {
                return s;
            }
        }
    }
    urlencoding::decode(trimmed)
        .map(|s| s.into_owned())
        .unwrap_or_else(|_| trimmed.to_string())
}

fn header_str(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn parse_subscription_userinfo(raw: &str) -> Option<Value> {
    let mut map = Map::new();
    for part in raw.split(';') {
        let part = part.trim();
        let Some((k, v)) = part.split_once('=') else {
            continue;
        };
        let key = k.trim().to_ascii_lowercase();
        let val_str = v.trim();
        if let Ok(num) = val_str.parse::<u64>() {
            match key.as_str() {
                "upload" | "download" | "total" | "expire" => {
                    map.insert(key, json!(num));
                }
                _ => {}
            }
        } else if let Ok(fnum) = val_str.parse::<f64>() {
            match key.as_str() {
                "upload" | "download" | "total" | "expire" => {
                    map.insert(key, json!(fnum as u64));
                }
                _ => {}
            }
        }
    }
    if map.is_empty() {
        None
    } else {
        Some(Value::Object(map))
    }
}

fn parse_content_disposition_name(raw: &str) -> Option<String> {
    for part in raw.split(';') {
        let p = part.trim();
        if let Some(rest) = p
            .strip_prefix("filename*=")
            .or_else(|| p.strip_prefix("FILENAME*="))
        {
            let cleaned = rest
                .trim_matches('"')
                .trim_start_matches("UTF-8''")
                .trim_start_matches("utf-8''");
            let decoded = urlencoding::decode(cleaned)
                .map(|s| s.into_owned())
                .unwrap_or_else(|_| cleaned.to_string());
            let stem = decoded.trim_end_matches(".yaml").trim_end_matches(".yml");
            if !stem.is_empty() {
                return Some(stem.to_string());
            }
        } else if let Some(rest) = p
            .strip_prefix("filename=")
            .or_else(|| p.strip_prefix("FILENAME="))
        {
            let cleaned = rest.trim_matches('"').trim_matches('\'');
            let decoded = urlencoding::decode(cleaned)
                .map(|s| s.into_owned())
                .unwrap_or_else(|_| cleaned.to_string());
            let stem = decoded.trim_end_matches(".yaml").trim_end_matches(".yml");
            if !stem.is_empty() {
                return Some(stem.to_string());
            }
        }
    }
    None
}

async fn get_runtime_proxy_port(state: &Arc<AppState>) -> Option<u16> {
    if let Some(p) = *state.last_mixed_port.lock().await {
        if p > 0 {
            return Some(p);
        }
    }
    if let Ok(cfg) = get_controled_mihomo_config(state, false).await {
        if let Some(p) = cfg.get("mixed-port").and_then(Value::as_u64) {
            if p > 0 && p <= u16::MAX as u64 {
                return Some(p as u16);
            }
        }
    }
    None
}

fn verify_and_prepare_yaml(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim_start_matches('\u{FEFF}').trim();
    if trimmed.is_empty() {
        return Err(tr("error.subscriptionFormatError").to_string());
    }
    if let Ok(parsed) = parse_yaml_to_json(trimmed) {
        if is_valid_mihomo_object(&parsed) {
            // Re-stringify normalized YAML so any short-id integers or subtle anchors don't break parser
            return stringify_json_to_yaml(&parsed);
        }
    }
    if let Some(converted) = normalize_subscription_to_mihomo_yaml(trimmed) {
        return Ok(converted.yaml);
    }
    Err(tr("error.subscriptionFormatError").to_string())
}

fn is_bool_header_true(val: &str) -> bool {
    matches!(val.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

async fn write_profile_yaml(id: &str, content: &str) -> Result<(), String> {
    let dir = profiles_dir();
    let _ = tokio::fs::create_dir_all(&dir).await;
    let target = profile_path(id);
    let tmp = target.with_extension("yaml.tmp");
    tokio::fs::write(&tmp, content)
        .await
        .map_err(|e| format!("Failed writing profile file: {e}"))?;
    if cfg!(windows) && target.exists() {
        let _ = tokio::fs::remove_file(&target).await;
    }
    tokio::fs::rename(&tmp, &target)
        .await
        .map_err(|e| format!("Failed replacing profile file: {e}"))?;
    Ok(())
}

pub async fn create_profile(
    state: &Arc<AppState>,
    app_handle: Option<&AppHandle>,
    item: Value,
) -> Result<Value, String> {
    let mut obj = item
        .as_object()
        .cloned()
        .ok_or_else(|| "Profile item must be an object".to_string())?;

    let id = obj
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(generate_profile_id);
    obj.insert("id".into(), Value::String(id.clone()));

    let p_type = obj
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("local")
        .to_string();

    if p_type == "remote" {
        let url_str = obj
            .get("url")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or("")
            .to_string();
        if url_str.is_empty() {
            return Err("Remote profile missing URL".to_string());
        }

        // Convert pasted share links directly into a local Mihomo profile without failing HTTP parsing.
        if is_share_link_input(&url_str) {
            let converted = normalize_subscription_to_mihomo_yaml(&url_str)
                .ok_or_else(|| tr("error.subscriptionFormatError").to_string())?;
            write_profile_yaml(&id, &converted.yaml).await?;

            obj.insert("type".into(), json!("local"));
            obj.remove("url");
            let needs_name = obj
                .get("name")
                .and_then(Value::as_str)
                .map(|s| s.trim().is_empty() || s == "Remote")
                .unwrap_or(true);
            if needs_name {
                let title = converted
                    .profile_title
                    .or(converted.first_node_name)
                    .unwrap_or_else(|| "Imported Profile".to_string());
                obj.insert("name".into(), json!(title));
            }
            obj.insert("updated".into(), json!(Utc::now().timestamp_millis()));
            return Ok(Value::Object(obj));
        }

        let parsed_url = Url::parse(&url_str).map_err(|e| format!("Invalid subscription URL: {e}"))?;
        let app_cfg = get_app_config(state, false).await.unwrap_or_default();
        let disable_hwid = app_cfg
            .get("disableHwid")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let custom_ua = app_cfg
            .get("userAgent")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        let mut ua_candidates: Vec<String> = Vec::with_capacity(4);
        ua_candidates.push(custom_ua.unwrap_or_else(get_default_user_agent));
        for fb in FALLBACK_USER_AGENTS {
            if !ua_candidates.iter().any(|u| u.eq_ignore_ascii_case(fb)) {
                ua_candidates.push(fb.to_string());
            }
        }

        let use_proxy = obj.get("useProxy").and_then(Value::as_bool).unwrap_or(false);
        let mut client_builder = Client::builder()
            .timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::limited(10))
            .danger_accept_invalid_certs(true);

        if use_proxy {
            if let Some(port) = get_runtime_proxy_port(state).await {
                if let Ok(proxy) = Proxy::all(format!("http://127.0.0.1:{port}")) {
                    client_builder = client_builder.proxy(proxy);
                }
            }
        }
        let client = client_builder
            .build()
            .map_err(|e| format!("Failed creating HTTP client: {e}"))?;

        let hwid = if !disable_hwid { Some(get_hwid()) } else { None };
        let os_name = get_device_os();
        let os_ver = get_os_version();
        let device_model = get_device_model();

        let mut last_err: Option<String> = None;
        let mut fetched_body: Option<String> = None;
        let mut fetched_headers: HeaderMap = HeaderMap::new();
        let mut converted_meta: Option<crate::utils::sub_converter::ConvertedSubscriptionResult> =
            None;

        for ua in &ua_candidates {
            let mut req = client.get(parsed_url.as_str()).header("User-Agent", ua);
            if let Some(ref id_val) = hwid {
                req = req
                    .header("x-hwid", id_val)
                    .header("x-device-os", os_name)
                    .header("x-ver-os", &os_ver)
                    .header("x-device-model", &device_model);
            }

            match req.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    let headers = resp.headers().clone();

                    if let Some(hwid_limit_val) = header_str(&headers, "x-hwid-limit") {
                        let support = header_str(&headers, "support-url").unwrap_or_default();
                        if is_bool_header_true(&hwid_limit_val) || status.as_u16() == 403 {
                            if let Some(handle) = app_handle {
                                let _ = handle.emit("ipc-show-hwid-limit-error", json!([support]));
                            }
                            return Err(format!("HWID_LIMIT:{support}"));
                        }
                    }

                    if !status.is_success() {
                        last_err = Some(format!("HTTP {} for {}", status.as_u16(), ua));
                        continue;
                    }

                    let body = match resp.text().await {
                        Ok(t) => t,
                        Err(e) => {
                            last_err = Some(format!("Failed reading body: {e}"));
                            continue;
                        }
                    };

                    if let Ok(yaml) = verify_and_prepare_yaml(&body) {
                        converted_meta = normalize_subscription_to_mihomo_yaml(&body);
                        fetched_body = Some(yaml);
                        fetched_headers = headers;
                        break;
                    } else {
                        last_err = Some(tr("error.subscriptionFormatError").to_string());
                    }
                }
                Err(e) => {
                    last_err = Some(format!("Request error ({ua}): {e}"));
                }
            }
        }

        let final_yaml = fetched_body.ok_or_else(|| {
            last_err.unwrap_or_else(|| tr("error.subscriptionFormatError").to_string())
        })?;

        write_profile_yaml(&id, &final_yaml).await?;

        // Extract response headers & subscription metadata
        if let Some(title_raw) = header_str(&fetched_headers, "profile-title") {
            let decoded = decode_header_value(&title_raw);
            let lock_name = obj.get("lockName").and_then(Value::as_bool).unwrap_or(false);
            let current_name = obj.get("name").and_then(Value::as_str).unwrap_or("");
            if !lock_name && !decoded.is_empty() && (current_name.is_empty() || current_name == "Remote") {
                obj.insert("name".into(), json!(decoded));
            }
        } else if let Some(cd) = header_str(&fetched_headers, "content-disposition") {
            if let Some(cd_name) = parse_content_disposition_name(&cd) {
                let current_name = obj.get("name").and_then(Value::as_str).unwrap_or("");
                if current_name.is_empty() || current_name == "Remote" {
                    obj.insert("name".into(), json!(cd_name));
                }
            }
        }

        if let Some(ref meta) = converted_meta {
            let current_name = obj.get("name").and_then(Value::as_str).unwrap_or("");
            if current_name.is_empty() || current_name == "Remote" {
                if let Some(ref title) = meta.profile_title {
                    obj.insert("name".into(), json!(title));
                }
            }
        }

        if obj
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .is_none()
        {
            let fallback_name = parsed_url
                .path_segments()
                .and_then(|s| s.last())
                .filter(|s| !s.is_empty())
                .or_else(|| parsed_url.host_str())
                .unwrap_or("Remote");
            obj.insert("name".into(), json!(fallback_name));
        }

        if let Some(ui_raw) = header_str(&fetched_headers, "subscription-userinfo") {
            if let Some(extra) = parse_subscription_userinfo(&ui_raw) {
                obj.insert("extra".into(), extra);
            }
        } else if let Some(ref meta) = converted_meta {
            if let Some(ref ui_raw) = meta.subscription_userinfo {
                if let Some(extra) = parse_subscription_userinfo(ui_raw) {
                    obj.insert("extra".into(), extra);
                }
            }
        }

        if let Some(iv_raw) = header_str(&fetched_headers, "profile-update-interval") {
            if let Ok(hours) = iv_raw.trim().parse::<u64>() {
                if hours > 0 {
                    obj.entry("interval".to_string())
                        .or_insert_with(|| json!(hours * 60));
                }
            }
        }

        if let Some(home) = header_str(&fetched_headers, "profile-web-page-url") {
            obj.insert("home".into(), json!(home));
        } else if let Some(ref meta) = converted_meta {
            if let Some(ref wp) = meta.web_page_url {
                obj.insert("home".into(), json!(wp));
            }
        }

        if let Some(logo) = header_str(&fetched_headers, "profile-logo") {
            obj.insert("logo".into(), json!(logo));
        }
        if let Some(support) = header_str(&fetched_headers, "support-url") {
            obj.insert("supportUrl".into(), json!(support));
        } else if let Some(ref meta) = converted_meta {
            if let Some(ref sup) = meta.support_url {
                obj.insert("supportUrl".into(), json!(sup));
            }
        }
        if let Some(gm) = header_str(&fetched_headers, "global-mode") {
            obj.insert("globalMode".into(), json!(is_bool_header_true(&gm)));
        }
        if let Some(ex) = header_str(&fetched_headers, "expand-proxy-groups") {
            obj.insert("expandProxyGroups".into(), json!(is_bool_header_true(&ex)));
        }
        if let Some(ann) = header_str(&fetched_headers, "announce") {
            obj.insert("announce".into(), json!(decode_header_value(&ann)));
        }
        if let Some(css) = header_str(&fetched_headers, "custom-css") {
            obj.insert("customCss".into(), json!(decode_header_value(&css)));
        }

        obj.insert("updated".into(), json!(Utc::now().timestamp_millis()));
        return Ok(Value::Object(obj));
    }

    // Local profile creation / import
    let raw_file = obj
        .get("file")
        .and_then(Value::as_str)
        .map(str::to_string);

    if let Some(content) = raw_file {
        let yaml = verify_and_prepare_yaml(&content)?;
        write_profile_yaml(&id, &yaml).await?;
        obj.remove("file");
    } else if !profile_path(&id).exists() {
        let yaml = stringify_json_to_yaml(&default_profile())?;
        write_profile_yaml(&id, &yaml).await?;
    }

    if obj
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_none()
    {
        obj.insert("name".into(), json!("Local"));
    }
    obj.insert("type".into(), json!("local"));
    obj.insert("updated".into(), json!(Utc::now().timestamp_millis()));
    Ok(Value::Object(obj))
}

pub async fn get_profile(state: &Arc<AppState>, id: Option<&str>) -> Result<Value, String> {
    let target_id = match id.filter(|s| !s.is_empty()) {
        Some(s) => s.to_string(),
        None => get_current_profile_item(state)
            .await?
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("default")
            .to_string(),
    };
    let path = profile_path(&target_id);
    if !path.exists() {
        return Ok(default_profile());
    }
    let raw = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| format!("Failed reading profile: {e}"))?;
    match parse_yaml_to_json(&raw) {
        Ok(v) if v.is_object() => Ok(v),
        _ => Ok(default_profile()),
    }
}

pub async fn get_profile_str(id: &str) -> Result<String, String> {
    let path = profile_path(id);
    if !path.exists() {
        return Ok(String::new());
    }
    tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| format!("Failed reading profile string: {e}"))
}

pub async fn get_profile_parse_str(state: &Arc<AppState>, id: &str) -> Result<String, String> {
    let val = get_profile(state, Some(id)).await?;
    stringify_json_to_yaml(&val)
}

pub async fn set_profile_str(
    state: &Arc<AppState>,
    app_handle: Option<&AppHandle>,
    id: &str,
    content: &str,
) -> Result<(), String> {
    write_profile_yaml(id, content).await?;
    let cfg = get_profile_config(state, false).await?;
    let current = cfg.get("current").and_then(Value::as_str);
    if current == Some(id) {
        restart_core(state, app_handle).await?;
    }
    Ok(())
}

fn resolve_work_file_path(state_cfg: &Value, current_id: Option<&str>, id: &str) -> PathBuf {
    let p = Path::new(id);
    if p.is_absolute() {
        return p.to_path_buf();
    }
    let diff_work_dir = state_cfg
        .get("diffWorkDir")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let base = if diff_work_dir {
        mihomo_profile_work_dir(current_id)
    } else {
        mihomo_work_dir()
    };
    let work_candidate = base.join(id);
    if work_candidate.exists() {
        return work_candidate;
    }
    let data_candidate = data_dir().join(id);
    if data_candidate.exists() {
        return data_candidate;
    }
    work_candidate
}

pub async fn get_file_str(state: &Arc<AppState>, id: &str) -> Result<String, String> {
    let app_cfg = get_app_config(state, false).await.unwrap_or_default();
    let prof_cfg = get_profile_config(state, false).await.unwrap_or_default();
    let current = prof_cfg.get("current").and_then(Value::as_str);
    let path = resolve_work_file_path(&app_cfg, current, id);
    if !path.exists() {
        return Ok(String::new());
    }
    tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| format!("Failed reading file '{}': {e}", path.display()))
}

pub async fn set_file_str(state: &Arc<AppState>, id: &str, content: &str) -> Result<(), String> {
    let app_cfg = get_app_config(state, false).await.unwrap_or_default();
    let prof_cfg = get_profile_config(state, false).await.unwrap_or_default();
    let current = prof_cfg.get("current").and_then(Value::as_str);
    let path = resolve_work_file_path(&app_cfg, current, id);
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    tokio::fs::write(&path, content)
        .await
        .map_err(|e| format!("Failed writing file '{}': {e}", path.display()))
}

pub async fn get_rule_str(id: &str) -> Result<String, String> {
    let path = rule_path(id);
    if !path.exists() {
        return Ok(String::new());
    }
    tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| format!("Failed reading rule file: {e}"))
}

pub async fn set_rule_str(id: &str, content: &str) -> Result<(), String> {
    let _ = tokio::fs::create_dir_all(rules_dir()).await;
    tokio::fs::write(rule_path(id), content)
        .await
        .map_err(|e| format!("Failed writing rule file: {e}"))
}

pub async fn convert_mrs_ruleset(
    state: &Arc<AppState>,
    file_path: &str,
    behavior: &str,
) -> Result<String, String> {
    let app_cfg = get_app_config(state, false).await.unwrap_or_default();
    let prof_cfg = get_profile_config(state, false).await.unwrap_or_default();
    let core = app_cfg
        .get("core")
        .and_then(Value::as_str)
        .unwrap_or("mihomo");
    let core_path = mihomo_core_path(core)?;
    let diff_work_dir = app_cfg
        .get("diffWorkDir")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let current = prof_cfg.get("current").and_then(Value::as_str);

    let full_path = if Path::new(file_path).is_absolute() {
        PathBuf::from(file_path)
    } else if diff_work_dir {
        mihomo_profile_work_dir(current).join(file_path)
    } else {
        mihomo_work_dir().join(file_path)
    };

    let tmp_name = format!("mrs-convert-{}.txt", generate_profile_id());
    let tmp_path = std::env::temp_dir().join(tmp_name);

    let mut cmd = Command::new(&core_path);
    cmd.args([
        "convert-ruleset",
        behavior,
        "mrs",
        &full_path.to_string_lossy(),
        &tmp_path.to_string_lossy(),
    ])
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());

    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);

    let output = cmd
        .output()
        .await
        .map_err(|e| format!("Failed executing convert-ruleset: {e}"))?;

    if !output.status.success() {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("convert-ruleset failed: {stderr}"));
    }

    let result = tokio::fs::read_to_string(&tmp_path).await;
    let _ = tokio::fs::remove_file(&tmp_path).await;
    result.map_err(|e| format!("Failed reading converted ruleset: {e}"))
}

#[allow(dead_code)]
pub fn parse_userinfo_map(raw: &str) -> HashMap<String, u64> {
    parse_subscription_userinfo(raw)
        .and_then(|v| v.as_object().cloned())
        .map(|m| {
            m.into_iter()
                .filter_map(|(k, v)| v.as_u64().map(|n| (k, n)))
                .collect()
        })
        .unwrap_or_default()
}
