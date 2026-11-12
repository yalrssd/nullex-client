use crate::config::app::{get_app_config, patch_app_config};
use crate::service::key::{generate_key_pair, sign_data};
use crate::service::pipe_http::pipe_request;
use crate::state::{AppState, KeyPairState};
use crate::utils::dirs::{service_ipc_path, service_path};
use crate::utils::elevation::exec_with_elevation;
use hyper::Method;
use serde_json::{json, Value};
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const CREATE_NO_WINDOW: u32 = 0x08000000;

pub async fn init_key_manager(state: &Arc<AppState>) -> KeyPairState {
    if let Ok(cfg) = get_app_config(state, false).await {
        if let Some(auth_key) = cfg.get("serviceAuthKey").and_then(Value::as_str) {
            if let Some((pub_k, priv_pem)) = auth_key.split_once(':') {
                if !pub_k.trim().is_empty() && !priv_pem.trim().is_empty() {
                    let kp = KeyPairState {
                        public_key: Some(pub_k.to_string()),
                        private_key_pem: Some(priv_pem.to_string()),
                    };
                    *state.key_pair.write().await = kp.clone();
                    return kp;
                }
            }
        }
    }

    if let Ok(kp) = generate_key_pair() {
        *state.key_pair.write().await = kp.clone();
        if let (Some(pub_k), Some(priv_pem)) = (&kp.public_key, &kp.private_key_pem) {
            let _ = patch_app_config(
                state,
                json!({ "serviceAuthKey": format!("{pub_k}:{priv_pem}") }),
            )
            .await;
        }
        return kp;
    }

    KeyPairState::default()
}

async fn signed_headers(state: &Arc<AppState>) -> Vec<(&'static str, String)> {
    let kp = state.key_pair.read().await.clone();
    let Some(priv_pem) = kp.private_key_pem else {
        return Vec::new();
    };
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string());
    match sign_data(&priv_pem, &ts) {
        Ok(sig) => vec![("X-Timestamp", ts), ("X-Signature", sig)],
        Err(_) => Vec::new(),
    }
}

pub async fn service_api_get(state: &Arc<AppState>, path: &str) -> Result<Value, String> {
    let hdrs = signed_headers(state).await;
    pipe_request(service_ipc_path(), Method::GET, path, None, &hdrs, 15)
        .await
        .map_err(|e| e.into_user_message())
}

pub async fn service_api_post(
    state: &Arc<AppState>,
    path: &str,
    body: Option<Value>,
) -> Result<Value, String> {
    let hdrs = signed_headers(state).await;
    pipe_request(service_ipc_path(), Method::POST, path, body, &hdrs, 15)
        .await
        .map_err(|e| e.into_user_message())
}

pub async fn test_service_connection(state: &Arc<AppState>) -> Result<bool, String> {
    match service_api_get(state, "/ping").await {
        Ok(v) => Ok(v.get("status").and_then(Value::as_str) == Some("ok") || !v.is_null()),
        Err(_) => Ok(false),
    }
}

pub async fn service_status() -> Result<String, String> {
    let exe = service_path();
    let exe_str = exe.to_string_lossy().to_string();
    tokio::task::spawn_blocking(move || {
        #[cfg(windows)]
        {
            let out = Command::new(&exe_str)
                .args(["service", "status"])
                .creation_flags(CREATE_NO_WINDOW)
                .output();
            match out {
                Ok(o) => {
                    let stdout = String::from_utf8_lossy(&o.stdout).to_lowercase();
                    let stderr = String::from_utf8_lossy(&o.stderr).to_lowercase();
                    let combined = format!("{stdout}\n{stderr}");
                    if combined.contains("not installed") || combined.contains("does not exist") {
                        Ok("not-installed".to_string())
                    } else if combined.contains("running") {
                        Ok("running".to_string())
                    } else if combined.contains("stopped") {
                        Ok("stopped".to_string())
                    } else if !o.status.success() {
                        Ok("unknown".to_string())
                    } else {
                        Ok("stopped".to_string())
                    }
                }
                Err(_) => Ok("unknown".to_string()),
            }
        }
        #[cfg(not(windows))]
        Ok("unknown".to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

pub async fn init_service(state: &Arc<AppState>) -> Result<(), String> {
    let kp = generate_key_pair()?;
    let pub_key = kp.public_key.clone().unwrap_or_default();
    let priv_pem = kp.private_key_pem.clone().unwrap_or_default();
    let exe = service_path().to_string_lossy().to_string();
    exec_with_elevation(
        &exe,
        &[
            "service".into(),
            "init".into(),
            "--public-key".into(),
            pub_key.clone(),
        ],
    )
    .await?;
    *state.key_pair.write().await = kp;
    patch_app_config(
        state,
        json!({ "serviceAuthKey": format!("{pub_key}:{priv_pem}") }),
    )
    .await?;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    Ok(())
}

async fn run_service_cmd(subcmd: &str) -> Result<(), String> {
    let exe = service_path().to_string_lossy().to_string();
    exec_with_elevation(&exe, &["service".into(), subcmd.to_string()]).await
}

pub async fn install_service() -> Result<(), String> {
    run_service_cmd("install").await
}
pub async fn uninstall_service() -> Result<(), String> {
    run_service_cmd("uninstall").await
}
pub async fn start_service() -> Result<(), String> {
    run_service_cmd("start").await
}
pub async fn stop_service() -> Result<(), String> {
    run_service_cmd("stop").await
}
pub async fn restart_service() -> Result<(), String> {
    run_service_cmd("restart").await
}
