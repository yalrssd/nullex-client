use crate::config::profile::{add_profile_item, get_profile_config};
use crate::state::AppState;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tauri::AppHandle;
use tokio::sync::Mutex;
use tokio::task::AbortHandle;

static UPDATERS: OnceLock<Mutex<HashMap<String, AbortHandle>>> = OnceLock::new();

fn updaters_map() -> &'static Mutex<HashMap<String, AbortHandle>> {
    UPDATERS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub async fn init_profile_updater(state: &Arc<AppState>, app_handle: Option<&AppHandle>) {
    let cfg = match get_profile_config(state, false).await {
        Ok(c) => c,
        Err(_) => return,
    };
    let Some(items) = cfg.get("items").and_then(Value::as_array) else {
        return;
    };
    for item in items {
        add_profile_updater(state, app_handle, item).await;
    }
}

pub fn add_profile_updater<'a>(
    state: &'a Arc<AppState>,
    app_handle: Option<&'a AppHandle>,
    item: &'a Value,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
    Box::pin(async move {
        let Some(id) = item.get("id").and_then(Value::as_str).map(str::to_string) else {
            return;
        };
        remove_profile_updater(&id).await;

        let is_remote = item.get("type").and_then(Value::as_str) == Some("remote");
        let interval_mins = item.get("interval").and_then(Value::as_u64).unwrap_or(0);
        let allow_auto = item
            .get("allowAutoUpdate")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let auto_update = item
            .get("autoUpdate")
            .and_then(Value::as_bool)
            .unwrap_or(true);

        if !is_remote || interval_mins == 0 || !allow_auto || !auto_update {
            return;
        }

        let state_clone = state.clone();
        let app_clone = app_handle.cloned();
        let item_clone = item.clone();
        let id_clone = id.clone();

        let handle = tokio::spawn(async move {
            let interval = Duration::from_secs(interval_mins.saturating_mul(60));
            let updated_ms = item_clone
                .get("updated")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let now_ms = chrono::Utc::now().timestamp_millis();
            let elapsed_ms = (now_ms - updated_ms).max(0) as u64;
            let interval_ms = interval.as_millis() as u64;

            let initial_delay = if elapsed_ms >= interval_ms {
                Duration::from_secs(30)
            } else {
                Duration::from_millis((interval_ms - elapsed_ms).min(u64::MAX))
            };

            tokio::time::sleep(initial_delay).await;
            loop {
                if let Ok(Some(latest)) =
                    crate::config::profile::get_profile_item(&state_clone, Some(&id_clone)).await
                {
                    let _ = add_profile_item(&state_clone, app_clone.as_ref(), latest).await;
                } else {
                    break;
                }
                tokio::time::sleep(interval).await;
            }
        });

        updaters_map()
            .lock()
            .await
            .insert(id, handle.abort_handle());
    })
}

pub async fn remove_profile_updater(id: &str) {
    if let Some(abort) = updaters_map().lock().await.remove(id) {
        abort.abort();
    }
}
