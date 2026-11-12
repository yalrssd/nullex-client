use sha2::{Digest, Sha256};
use std::sync::OnceLock;

static HWID_CACHE: OnceLock<String> = OnceLock::new();

#[cfg(windows)]
fn read_machine_guid() -> String {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;
    RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"SOFTWARE\Microsoft\Cryptography")
        .and_then(|k| k.get_value::<String, _>("MachineGuid"))
        .unwrap_or_default()
        .trim()
        .to_string()
}

#[cfg(not(windows))]
fn read_machine_guid() -> String {
    String::new()
}

pub fn get_hwid() -> String {
    HWID_CACHE
        .get_or_init(|| {
            let mut raw = read_machine_guid();
            if raw.is_empty() {
                raw = std::env::var("COMPUTERNAME")
                    .or_else(|_| std::env::var("HOSTNAME"))
                    .unwrap_or_else(|_| "nullex-host".to_string());
            }
            let hash = Sha256::digest(raw.as_bytes());
            hex::encode(hash).chars().take(16).collect()
        })
        .clone()
}

pub fn get_device_os() -> &'static str {
    if cfg!(windows) {
        "Windows"
    } else if cfg!(target_os = "macos") {
        "macOS"
    } else {
        "Linux"
    }
}

#[cfg(windows)]
pub fn get_os_version() -> String {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;
    let Ok(key) = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
    else {
        return "Windows 10/11".to_string();
    };
    let display: String = key.get_value("DisplayVersion").unwrap_or_default();
    let build: String = key.get_value("CurrentBuildNumber").unwrap_or_default();
    let build_num: u32 = build.trim().parse().unwrap_or(0);
    let prefix = if build_num >= 22000 { "11" } else { "10" };
    if display.is_empty() {
        format!("{prefix} (Build {build})")
    } else {
        format!("{prefix} {display}")
    }
}

#[cfg(not(windows))]
pub fn get_os_version() -> String {
    std::env::consts::OS.to_string()
}

pub fn get_device_model() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "PC".to_string())
}
