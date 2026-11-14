use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU16, AtomicU32, AtomicU64};
use std::sync::Arc;
use tokio::process::Child;
use tokio::sync::{Mutex, Notify, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreStatePayload {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<CoreErrorPayload>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreErrorPayload {
    pub reason: String,
    pub message: String,
}

impl Default for CoreStatePayload {
    fn default() -> Self {
        Self {
            status: "stopped".to_string(),
            error: None,
        }
    }
}

#[derive(Clone, Default)]
pub struct KeyPairState {
    pub public_key: Option<String>,
    pub private_key_pem: Option<String>,
}

pub struct StreamHandles {
    pub traffic_abort: Option<tokio::task::AbortHandle>,
    pub memory_abort: Option<tokio::task::AbortHandle>,
    pub logs_abort: Option<tokio::task::AbortHandle>,
    pub connections_abort: Option<tokio::task::AbortHandle>,
    pub logs_level: String,
}

impl Default for StreamHandles {
    fn default() -> Self {
        Self {
            traffic_abort: None,
            memory_abort: None,
            logs_abort: None,
            connections_abort: None,
            logs_level: "info".to_string(),
        }
    }
}

pub struct AppState {
    pub app_config: RwLock<Option<Value>>,
    pub app_config_write_lock: Mutex<()>,
    pub mihomo_config: RwLock<Option<Value>>,
    pub profile_config: RwLock<Option<Value>>,
    pub profile_lock: Mutex<()>,

    pub runtime_config: RwLock<Option<Value>>,
    pub runtime_config_str: RwLock<String>,
    pub raw_profile_str: RwLock<String>,
    pub current_profile_str: RwLock<String>,
    pub last_mixed_port: Mutex<Option<u16>>,

    pub core_state: RwLock<CoreStatePayload>,
    pub core_child: Mutex<Option<Child>>,
    pub core_pid: AtomicU32,
    pub core_retry: AtomicU32,
    pub core_starting: Mutex<bool>,
    pub core_start_notify: Notify,

    pub key_pair: RwLock<KeyPairState>,
    pub pac_port: AtomicU16,
    pub pac_shutdown: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,

    pub streams: Mutex<StreamHandles>,
    pub network_detect_abort: Mutex<Option<tokio::task::AbortHandle>>,
    pub network_down_handled: AtomicBool,

    pub needs_first_run_admin: AtomicBool,
    pub is_quitting: AtomicBool,
    pub not_quit_dialog: AtomicBool,
    pub last_quit_attempt_ms: AtomicI64,

    /// Windows Job Object handle (isize = HANDLE). The core sidecar is assigned
    /// to this job with KILL_ON_JOB_CLOSE so it dies with the client in all
    /// termination scenarios (crash, Task Manager kill, etc.).
    pub core_job: Mutex<Option<isize>>,

    pub quit_confirm_tx: Mutex<Option<tokio::sync::oneshot::Sender<bool>>>,
    pub install_confirm_tx: Mutex<Option<tokio::sync::oneshot::Sender<bool>>>,

    pub update_cancel: Arc<AtomicBool>,
    pub shortcuts: Mutex<HashMap<String, String>>,
    pub current_css: RwLock<String>,
    pub tray_icon: Mutex<Option<tauri::tray::TrayIcon>>,
    pub lightweight_seq: AtomicU64,
    pub tcp_delay_history: RwLock<HashMap<String, Vec<Value>>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            app_config: RwLock::new(None),
            app_config_write_lock: Mutex::new(()),
            mihomo_config: RwLock::new(None),
            profile_config: RwLock::new(None),
            profile_lock: Mutex::new(()),
            runtime_config: RwLock::new(None),
            runtime_config_str: RwLock::new(String::new()),
            raw_profile_str: RwLock::new(String::new()),
            current_profile_str: RwLock::new(String::new()),
            last_mixed_port: Mutex::new(None),
            core_state: RwLock::new(CoreStatePayload::default()),
            core_child: Mutex::new(None),
            core_pid: AtomicU32::new(0),
            core_retry: AtomicU32::new(10),
            core_starting: Mutex::new(false),
            core_start_notify: Notify::new(),
            key_pair: RwLock::new(KeyPairState::default()),
            pac_port: AtomicU16::new(0),
            pac_shutdown: Mutex::new(None),
            streams: Mutex::new(StreamHandles::default()),
            network_detect_abort: Mutex::new(None),
            network_down_handled: AtomicBool::new(false),
            needs_first_run_admin: AtomicBool::new(false),
            is_quitting: AtomicBool::new(false),
            not_quit_dialog: AtomicBool::new(false),
            last_quit_attempt_ms: AtomicI64::new(0),
            core_job: Mutex::new(None),
            quit_confirm_tx: Mutex::new(None),
            install_confirm_tx: Mutex::new(None),
            update_cancel: Arc::new(AtomicBool::new(false)),
            shortcuts: Mutex::new(HashMap::new()),
            current_css: RwLock::new(String::new()),
            tray_icon: Mutex::new(None),
            lightweight_seq: AtomicU64::new(0),
            tcp_delay_history: RwLock::new(HashMap::new()),
        }
    }
}
