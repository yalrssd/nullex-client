use crate::config::template::default_app_config;
use crate::state::AppState;
use crate::utils::crypto_dpapi::{decrypt_string, encrypt_string, is_encrypted};
use crate::utils::dirs::app_config_path;
use crate::utils::merge::deep_merge;
use crate::utils::yaml::{parse_yaml_to_json, stringify_json_to_yaml};
use serde_json::Value;
use std::sync::Arc;

fn is_valid_config(cfg: &Value) -> bool {
    cfg.get("sysProxy").map(Value::is_object).unwrap_or(false)
}

fn decrypt_config(mut cfg: Value) -> Value {
    if let Some(obj) = cfg.as_object_mut() {
        if let Some(val) = obj.get("serviceAuthKey").and_then(Value::as_str) {
            let dec = if is_encrypted(val) {
                decrypt_string(val)
            } else {
                val.to_string()
            };
            obj.insert("serviceAuthKey".into(), Value::String(dec));
        }
    }
    cfg
}

fn encrypt_config(mut cfg: Value) -> Value {
    if let Some(obj) = cfg.as_object_mut() {
        if let Some(val) = obj.get("serviceAuthKey").and_then(Value::as_str) {
            if !val.is_empty() {
                obj.insert("serviceAuthKey".into(), Value::String(encrypt_string(val)));
            }
        }
    }
    cfg
}

async fn safe_write_config(content: &str) -> Result<(), String> {
    let cfg_path = app_config_path();
    let tmp_path = cfg_path.with_extension("yaml.tmp");
    let backup_path = cfg_path.with_extension("yaml.backup");

    if let Some(parent) = cfg_path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }

    tokio::fs::write(&tmp_path, content)
        .await
        .map_err(|e| format!("Failed writing temp config: {e}"))?;

    if cfg_path.exists() {
        let _ = tokio::fs::copy(&cfg_path, &backup_path).await;
        if cfg!(windows) {
            let _ = tokio::fs::remove_file(&cfg_path).await;
        }
    }

    if let Err(e) = tokio::fs::rename(&tmp_path, &cfg_path).await {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(format!("Failed renaming temp config: {e}"));
    }
    Ok(())
}

pub fn get_app_config_sync() -> Value {
    let path = app_config_path();
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return default_app_config();
    };
    let Ok(parsed) = parse_yaml_to_json(&raw) else {
        return default_app_config();
    };
    if parsed.is_object() {
        decrypt_config(parsed)
    } else {
        default_app_config()
    }
}

pub async fn get_app_config(state: &Arc<AppState>, force: bool) -> Result<Value, String> {
    if !force {
        if let Some(cached) = state.app_config.read().await.clone() {
            return Ok(cached);
        }
    }

    let path = app_config_path();
    let loaded = match tokio::fs::read_to_string(&path).await {
        Ok(raw) => match parse_yaml_to_json(&raw) {
            Ok(parsed) if is_valid_config(&parsed) => decrypt_config(parsed),
            _ => {
                let backup = tokio::fs::read_to_string(path.with_extension("yaml.backup"))
                    .await
                    .ok()
                    .and_then(|b| parse_yaml_to_json(&b).ok())
                    .filter(is_valid_config);
                decrypt_config(backup.unwrap_or_else(default_app_config))
            }
        },
        Err(_) => default_app_config(),
    };

    *state.app_config.write().await = Some(loaded.clone());
    Ok(loaded)
}

pub async fn patch_app_config(state: &Arc<AppState>, patch: Value) -> Result<(), String> {
    let _guard = state.app_config_write_lock.lock().await;
    let mut current = get_app_config(state, false).await?;
    deep_merge(&mut current, &patch);
    let encrypted = encrypt_config(current.clone());
    let yaml = stringify_json_to_yaml(&encrypted)?;
    safe_write_config(&yaml).await?;
    *state.app_config.write().await = Some(current);
    Ok(())
}
