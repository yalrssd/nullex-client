use chrono::Local;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static RESOURCE_BASE: OnceLock<PathBuf> = OnceLock::new();

pub fn set_resource_dir(dir: PathBuf) {
    let _ = RESOURCE_BASE.set(dir);
}

pub fn exe_path() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("Nullex.exe"))
}

pub fn exe_dir() -> PathBuf {
    exe_path()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn home_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(exe_dir)
}

pub fn is_portable() -> bool {
    exe_dir().join("PORTABLE").exists()
}

/// Keep `%APPDATA%/nullex` parity with the Electron build so existing user configs/profiles transfer cleanly.
pub fn user_data_dir() -> PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return PathBuf::from(appdata).join("nullex");
    }
    home_dir().join(".config").join("nullex")
}

pub fn data_dir() -> PathBuf {
    if is_portable() {
        exe_dir().join("data")
    } else {
        user_data_dir()
    }
}

pub fn task_dir() -> PathBuf {
    let dir = user_data_dir().join("tasks");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn resources_dir() -> PathBuf {
    if let Some(base) = RESOURCE_BASE.get() {
        let nested = base.join("resources");
        if nested.join("sidecar").exists() || nested.join("files").exists() {
            return nested;
        }
        if base.join("sidecar").exists() || base.join("files").exists() {
            return base.clone();
        }
    }
    let beside_exe = exe_dir().join("resources");
    if beside_exe.join("sidecar").exists() || beside_exe.join("files").exists() {
        return beside_exe;
    }
    let dev_res = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources");
    if dev_res.exists() {
        return dev_res;
    }
    beside_exe
}

pub fn resources_files_dir() -> PathBuf {
    resources_dir().join("files")
}

pub fn app_icons_dir() -> PathBuf {
    resources_dir().join("app")
}

pub fn themes_dir() -> PathBuf {
    data_dir().join("themes")
}

static MIHOMO_PIPE: OnceLock<String> = OnceLock::new();

pub fn mihomo_ipc_path() -> &'static str {
    MIHOMO_PIPE
        .get_or_init(|| {
            if let Ok(custom) = std::env::var("NULLEX_MIHOMO_PIPE") {
                if !custom.trim().is_empty() {
                    return custom;
                }
            }
            if cfg!(windows) {
                r"\\.\pipe\Nullex\mihomo".to_string()
            } else {
                "/tmp/nullex-mihomo-api.sock".to_string()
            }
        })
        .as_str()
}

pub fn service_ipc_path() -> &'static str {
    if cfg!(windows) {
        r"\\.\pipe\sparkle\service"
    } else {
        "/tmp/sparkle-service.sock"
    }
}

pub fn mihomo_core_dir() -> PathBuf {
    resources_dir().join("sidecar")
}

pub fn downloaded_core_dir() -> PathBuf {
    data_dir().join("sidecar")
}

pub fn mihomo_core_path(core: &str) -> Result<PathBuf, String> {
    if core != "mihomo" && core != "mihomo-alpha" {
        return Err("Unsupported core name".to_string());
    }
    let ext = if cfg!(windows) { ".exe" } else { "" };
    let file = format!("{core}{ext}");
    let bundled = mihomo_core_dir().join(&file);
    if core == "mihomo-alpha" && !bundled.exists() {
        return Ok(downloaded_core_dir().join(&file));
    }
    Ok(bundled)
}

pub fn installed_cores() -> Vec<String> {
    ["mihomo", "mihomo-alpha"]
        .into_iter()
        .filter(|c| mihomo_core_path(c).map(|p| p.exists()).unwrap_or(false))
        .map(str::to_string)
        .collect()
}

pub fn service_path() -> PathBuf {
    let name = if cfg!(windows) {
        "sparkle-service.exe"
    } else {
        "sparkle-service"
    };
    resources_files_dir().join(name)
}

pub fn app_config_path() -> PathBuf {
    data_dir().join("config.yaml")
}

pub fn controled_mihomo_config_path() -> PathBuf {
    data_dir().join("mihomo.yaml")
}

pub fn profile_config_path() -> PathBuf {
    data_dir().join("profile.yaml")
}

pub fn profiles_dir() -> PathBuf {
    data_dir().join("profiles")
}

pub fn profile_path(id: &str) -> PathBuf {
    profiles_dir().join(format!("{id}.yaml"))
}

pub fn mihomo_work_dir() -> PathBuf {
    data_dir().join("work")
}

pub fn mihomo_profile_work_dir(id: Option<&str>) -> PathBuf {
    mihomo_work_dir().join(id.filter(|s| !s.is_empty()).unwrap_or("default"))
}

pub fn mihomo_test_dir() -> PathBuf {
    data_dir().join("test")
}

pub fn mihomo_work_config_path(id: Option<&str>) -> PathBuf {
    match id {
        Some("work") => mihomo_work_dir().join("config.yaml"),
        other => mihomo_profile_work_dir(other).join("config.yaml"),
    }
}

pub fn log_dir() -> PathBuf {
    data_dir().join("logs")
}

pub fn log_path() -> PathBuf {
    let now = Local::now();
    log_dir().join(format!("{}-{}-{}.log", now.format("%Y"), now.format("%-m"), now.format("%-d")))
}

pub fn rules_dir() -> PathBuf {
    data_dir().join("rules")
}

pub fn rule_path(id: &str) -> PathBuf {
    rules_dir().join(format!("{id}.yaml"))
}
