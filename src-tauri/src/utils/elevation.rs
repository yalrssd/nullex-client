use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::OnceLock;

const CREATE_NO_WINDOW: u32 = 0x08000000;
static IS_ADMIN_CACHED: OnceLock<bool> = OnceLock::new();

pub fn is_running_as_admin() -> bool {
    *IS_ADMIN_CACHED.get_or_init(|| {
        #[cfg(windows)]
        {
            Command::new("net")
                .args(["session"])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        }
        #[cfg(not(windows))]
        {
            false
        }
    })
}

pub async fn exec_with_elevation(command: &str, args: &[String]) -> Result<(), String> {
    let cmd_owned = command.to_string();
    let args_owned = args.to_vec();

    tokio::task::spawn_blocking(move || exec_with_elevation_sync(&cmd_owned, &args_owned))
        .await
        .map_err(|e| format!("Task join error: {e}"))?
}

fn exec_with_elevation_sync(command: &str, args: &[String]) -> Result<(), String> {
    #[cfg(windows)]
    {
        if is_running_as_admin() {
            let output = Command::new(command)
                .args(args)
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .map_err(|e| format!("Windows elevation failed: {e}"))?;
            if !output.status.success() {
                let err = String::from_utf8_lossy(&output.stderr);
                let out = String::from_utf8_lossy(&output.stdout);
                return Err(format!("Command exited with {}: {} {}", output.status, out.trim(), err.trim()));
            }
            return Ok(());
        }

        let escaped_cmd = command.replace('\'', "''");
        let ps_args = args
            .iter()
            .map(|a| format!("'{}'", a.replace('\'', "''")))
            .collect::<Vec<_>>()
            .join(",");

        let script = if args.is_empty() {
            format!(
                "& {{ $p = Start-Process -FilePath '{escaped_cmd}' -Verb RunAs -WindowStyle Hidden -PassThru -Wait; exit $p.ExitCode }}"
            )
        } else {
            format!(
                "& {{ $p = Start-Process -FilePath '{escaped_cmd}' -ArgumentList @({ps_args}) -Verb RunAs -WindowStyle Hidden -PassThru -Wait; exit $p.ExitCode }}"
            )
        };

        let output = Command::new("powershell.exe")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &script])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("Windows elevation failed: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("canceled by the user") || stderr.contains("отменена пользователем") {
                return Err("User canceled elevation".to_string());
            }
            return Err(format!(
                "Windows elevation failed ({}): {}",
                output.status,
                stderr.trim()
            ));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (command, args);
        Err("Unsupported platform".to_string())
    }
}
