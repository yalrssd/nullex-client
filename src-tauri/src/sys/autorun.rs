use crate::utils::dirs::{exe_path, resources_files_dir, task_dir};
use crate::utils::elevation::{exec_with_elevation, is_running_as_admin};
use std::process::Stdio;
use tokio::process::Command;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const AUTORUN_TASK_NAME: &str = "nullex";

pub async fn check_auto_run() -> Result<bool, String> {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("schtasks.exe");
        cmd.args(["/Query", "/TN", AUTORUN_TASK_NAME])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW);
        let status = cmd
            .status()
            .await
            .map_err(|e| format!("Failed checking schtasks: {e}"))?;
        Ok(status.success())
    }
    #[cfg(not(windows))]
    Ok(false)
}

fn encode_utf16le_bom(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + text.len() * 2);
    out.push(0xFF);
    out.push(0xFE);
    for unit in text.encode_utf16() {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out
}

pub async fn enable_auto_run() -> Result<(), String> {
    #[cfg(windows)]
    {
        let target_dir = task_dir();
        let runner_dst = target_dir.join("nullex-run.exe");
        let runner_src = resources_files_dir().join("nullex-run.exe");
        if runner_src.exists() && !runner_dst.exists() {
            let _ = tokio::fs::copy(&runner_src, &runner_dst).await;
        }

        let app_exe = exe_path().to_string_lossy().to_string();
        let (command_str, args_str) = if runner_dst.exists() {
            (
                runner_dst.to_string_lossy().to_string(),
                format!("\"{app_exe}\""),
            )
        } else {
            (app_exe, String::new())
        };

        let run_level = if is_running_as_admin() {
            "HighestAvailable"
        } else {
            "LeastPrivilege"
        };

        let args_xml = if args_str.is_empty() {
            String::new()
        } else {
            format!("\n      <Arguments>{}</Arguments>", xml_escape(&args_str))
        };

        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>{run_level}</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>false</AllowHardTerminate>
    <StartWhenAvailable>true</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>3</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{}</Command>{args_xml}
    </Exec>
  </Actions>
</Task>"#,
            xml_escape(&command_str)
        );

        let xml_path = target_dir.join(format!("{AUTORUN_TASK_NAME}.xml"));
        tokio::fs::write(&xml_path, encode_utf16le_bom(&xml))
            .await
            .map_err(|e| format!("Failed writing autorun XML: {e}"))?;

        let xml_str = xml_path.to_string_lossy().to_string();
        let mut cmd = Command::new("schtasks.exe");
        cmd.args(["/Create", "/TN", AUTORUN_TASK_NAME, "/XML", &xml_str, "/F"])
            .creation_flags(CREATE_NO_WINDOW);
        let out = cmd.output().await;
        if let Ok(o) = out {
            if o.status.success() {
                return Ok(());
            }
        }

        exec_with_elevation(
            "schtasks.exe",
            &[
                "/Create".into(),
                "/TN".into(),
                AUTORUN_TASK_NAME.into(),
                "/XML".into(),
                xml_str,
                "/F".into(),
            ],
        )
        .await
    }
    #[cfg(not(windows))]
    Ok(())
}

pub async fn disable_auto_run() -> Result<(), String> {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("schtasks.exe");
        cmd.args(["/Delete", "/TN", AUTORUN_TASK_NAME, "/F"])
            .creation_flags(CREATE_NO_WINDOW);
        if let Ok(out) = cmd.output().await {
            if out.status.success() {
                return Ok(());
            }
            let combined = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            )
            .to_lowercase();
            if combined.contains("cannot find") || combined.contains("does not exist") {
                return Ok(());
            }
        }
        exec_with_elevation(
            "schtasks.exe",
            &[
                "/Delete".into(),
                "/TN".into(),
                AUTORUN_TASK_NAME.into(),
                "/F".into(),
            ],
        )
        .await
    }
    #[cfg(not(windows))]
    Ok(())
}

pub fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
