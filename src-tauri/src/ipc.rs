use crate::config::app::{get_app_config, patch_app_config};
use crate::config::mihomo::{get_controled_mihomo_config, patch_controled_mihomo_config};
use crate::config::profile::{
    add_profile_item, change_current_profile, convert_mrs_ruleset, get_current_profile_item,
    get_default_user_agent, get_file_str, get_profile_config, get_profile_item,
    get_profile_parse_str, get_profile_str, get_rule_str, remove_profile_item, set_file_str,
    set_profile_config, set_profile_str, set_rule_str, update_profile_item,
};
use crate::core::alpha_core::download_alpha_core;
use crate::core::factory::{
    get_current_profile_str, get_effective_log_level, get_effective_ports, get_raw_profile_str,
    get_runtime_config, get_runtime_config_str,
};
use crate::core::manager::{
    check_core_permission, get_core_state, manual_grant_core_permission, restart_core,
    revoke_core_permission, start_network_detection, stop_core, stop_network_detection,
};
use crate::core::mihomo_api::{
    apply_log_level, mihomo_change_proxy, mihomo_close_all_connections, mihomo_close_connection,
    mihomo_config, mihomo_group_delay, mihomo_groups, mihomo_hot_reload_config, mihomo_proxies,
    mihomo_proxy_delay, mihomo_proxy_providers, mihomo_rule_providers, mihomo_rules,
    mihomo_unfixed_proxy, mihomo_update_proxy_providers, mihomo_update_rule_providers,
    mihomo_upgrade, mihomo_upgrade_geo, mihomo_upgrade_ui, mihomo_version, patch_mihomo_config,
    restart_mihomo_connections,
};
use crate::desktop::floating::{close_floating_window, show_context_menu, show_floating_window};
use crate::desktop::shortcuts::{register_shortcut, trigger_main_window};
use crate::desktop::theme::{
    apply_theme, fetch_themes, import_themes, read_theme, resolve_themes, write_theme,
};
use crate::desktop::tray::{
    close_tray_icon, request_tray_menu_update, show_tray_icon, update_tray_icon,
};
use crate::desktop::updater::{cancel_update, check_update, download_and_install_update};
use crate::service::manager::{
    init_service, install_service, restart_service, service_status, start_service, stop_service,
    test_service_connection, uninstall_service,
};
use crate::state::AppState;
use crate::sys::autorun::{check_auto_run, disable_auto_run, enable_auto_run};
use crate::sys::misc::{
    check_elevate_task, copy_env, decline_elevation, delete_elevate_task, get_app_name,
    get_file_path_dialog, get_icon_data_url, get_image_data_url, get_interfaces, open_data_dir,
    open_file_by_id, open_uwp_tool, read_text_file, reset_app_config, restart_as_admin,
    setup_firewall,
};
use crate::sys::sysproxy::{disable_sys_proxy, trigger_sys_proxy};
use crate::utils::dirs::installed_cores;
use crate::utils::i18n::set_language;
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

fn arg_str(args: &[Value], idx: usize) -> Option<&str> {
    args.get(idx).and_then(Value::as_str)
}

fn arg_bool(args: &[Value], idx: usize, default: bool) -> bool {
    args.get(idx).and_then(Value::as_bool).unwrap_or(default)
}

fn arg_val(args: &[Value], idx: usize) -> Value {
    args.get(idx).cloned().unwrap_or(Value::Null)
}

pub async fn request_quit_or_confirm(state: &Arc<AppState>, app_handle: &AppHandle) {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let prev_ms = state.last_quit_attempt_ms.swap(now_ms, Ordering::SeqCst);
    let quick_double = now_ms - prev_ms < 3000;

    if state.not_quit_dialog.load(Ordering::SeqCst) || quick_double {
        state.is_quitting.store(true, Ordering::SeqCst);
        let _ = stop_core(state, Some(app_handle)).await;
        let _ = disable_sys_proxy(state, false).await;
        app_handle.exit(0);
        return;
    }

    if let Some(win) = app_handle.get_webview_window("main") {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
    }

    let (tx, rx) = tokio::sync::oneshot::channel::<bool>();
    *state.quit_confirm_tx.lock().await = Some(tx);
    let _ = app_handle.emit("ipc-show-quit-confirm", json!([]));

    let st = state.clone();
    let ah = app_handle.clone();
    tokio::spawn(async move {
        if let Ok(true) = rx.await {
            st.is_quitting.store(true, Ordering::SeqCst);
            let _ = stop_core(&st, Some(&ah)).await;
            let _ = disable_sys_proxy(&st, false).await;
            ah.exit(0);
        }
    });
}

#[tauri::command]
pub async fn ipc_invoke(
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
    channel: String,
    args: Option<Vec<Value>>,
) -> Result<Value, String> {
    let args = args.unwrap_or_default();
    let st = state.inner();

    match channel.as_str() {
        // Mihomo REST API & Streams
        "mihomoVersion" => mihomo_version().await,
        "mihomoConfig" => mihomo_config().await,
        "patchMihomoConfig" => {
            patch_mihomo_config(arg_val(&args, 0)).await?;
            Ok(Value::Null)
        }
        "mihomoProxies" => mihomo_proxies().await,
        "mihomoGroups" => mihomo_groups(st).await,
        "mihomoProxyProviders" => mihomo_proxy_providers().await,
        "mihomoRuleProviders" => mihomo_rule_providers().await,
        "mihomoRules" => mihomo_rules().await,
        "mihomoUpdateProxyProviders" => {
            let name = arg_str(&args, 0).unwrap_or("");
            mihomo_update_proxy_providers(Some(&app_handle), name).await?;
            Ok(Value::Null)
        }
        "mihomoUpdateRuleProviders" => {
            let name = arg_str(&args, 0).unwrap_or("");
            mihomo_update_rule_providers(Some(&app_handle), name).await?;
            Ok(Value::Null)
        }
        "mihomoChangeProxy" => {
            let group = arg_str(&args, 0).unwrap_or("");
            let proxy = arg_str(&args, 1).unwrap_or("");
            mihomo_change_proxy(st, Some(&app_handle), group, proxy).await?;
            Ok(Value::Null)
        }
        "mihomoUnfixedProxy" => {
            let group = arg_str(&args, 0).unwrap_or("");
            mihomo_unfixed_proxy(st, Some(&app_handle), group).await?;
            Ok(Value::Null)
        }
        "mihomoProxyDelay" => {
            let proxy = arg_str(&args, 0).unwrap_or("");
            let url = arg_str(&args, 1);
            let provider = arg_str(&args, 2);
            mihomo_proxy_delay(st, proxy, url, provider).await
        }
        "mihomoGroupDelay" => {
            let group = arg_str(&args, 0).unwrap_or("");
            let url = arg_str(&args, 1);
            mihomo_group_delay(st, group, url).await
        }
        "mihomoCloseConnection" => {
            let id = arg_str(&args, 0).unwrap_or("");
            mihomo_close_connection(id).await?;
            Ok(Value::Null)
        }
        "mihomoCloseAllConnections" => {
            mihomo_close_all_connections(arg_str(&args, 0)).await?;
            Ok(Value::Null)
        }
        "mihomoApplyLogLevel" => {
            let lvl = get_effective_log_level(st).await;
            apply_log_level(st, Some(&app_handle), &lvl).await?;
            Ok(Value::Null)
        }
        "mihomoHotReloadConfig" => {
            mihomo_hot_reload_config(st, Some(&app_handle)).await?;
            Ok(Value::Null)
        }
        "restartMihomoConnections" => {
            restart_mihomo_connections(st, &app_handle).await;
            Ok(Value::Null)
        }
        "mihomoUpgrade" => {
            mihomo_upgrade().await?;
            Ok(Value::Null)
        }
        "mihomoUpgradeGeo" => {
            mihomo_upgrade_geo().await?;
            Ok(Value::Null)
        }
        "mihomoUpgradeUI" => {
            mihomo_upgrade_ui().await?;
            Ok(Value::Null)
        }

        // AutoRun & System Proxy & Core
        "checkAutoRun" => check_auto_run().await.map(|b| json!(b)),
        "enableAutoRun" => {
            enable_auto_run().await?;
            Ok(Value::Null)
        }
        "disableAutoRun" => {
            disable_auto_run().await?;
            Ok(Value::Null)
        }
        "triggerSysProxy" => {
            let enable = arg_bool(&args, 0, false);
            let only_active = arg_bool(&args, 1, false);
            trigger_sys_proxy(st, enable, only_active).await?;
            Ok(Value::Null)
        }
        "restartCore" => {
            restart_core(st, Some(&app_handle)).await?;
            Ok(Value::Null)
        }
        "getCoreState" => {
            let s = get_core_state(st).await;
            serde_json::to_value(s).map_err(|e| e.to_string())
        }
        "installedCores" => Ok(json!(installed_cores())),
        "downloadAlphaCore" => {
            download_alpha_core(st).await?;
            Ok(Value::Null)
        }
        "manualGrantCorePermition" => {
            manual_grant_core_permission().await?;
            Ok(Value::Null)
        }
        "revokeCorePermission" => {
            revoke_core_permission().await?;
            Ok(Value::Null)
        }
        "checkCorePermission" => Ok(check_core_permission()),
        "startNetworkDetection" => {
            start_network_detection(st, Some(&app_handle)).await;
            Ok(Value::Null)
        }
        "stopNetworkDetection" => {
            stop_network_detection(st).await;
            Ok(Value::Null)
        }

        // App & Mihomo Config
        "getAppConfig" => {
            let force = arg_bool(&args, 0, false);
            get_app_config(st, force).await
        }
        "patchAppConfig" => {
            let patch = arg_val(&args, 0);
            patch_app_config(st, patch.clone()).await?;
            if let Some(lang) = patch.get("language").and_then(Value::as_str) {
                set_language(lang);
            }
            let _ = app_handle.emit("ipc-appConfigUpdated", json!([]));
            if patch.get("delayTestMode").is_some() {
                let _ = app_handle.emit("ipc-groupsUpdated", json!([]));
            }
            request_tray_menu_update(st, &app_handle);
            Ok(Value::Null)
        }
        "resetAppConfig" => {
            reset_app_config(st, &app_handle).await?;
            Ok(Value::Null)
        }
        "getControledMihomoConfig" => {
            let force = arg_bool(&args, 0, false);
            get_controled_mihomo_config(st, force).await
        }
        "patchControledMihomoConfig" => {
            let patch = arg_val(&args, 0);
            patch_controled_mihomo_config(st, Some(&app_handle), patch).await?;
            let _ = app_handle.emit("ipc-controledMihomoConfigUpdated", json!([]));
            request_tray_menu_update(st, &app_handle);
            Ok(Value::Null)
        }

        // Profiles & Rules & Files
        "getProfileConfig" => {
            let force = arg_bool(&args, 0, false);
            get_profile_config(st, force).await
        }
        "setProfileConfig" => {
            set_profile_config(st, Some(&app_handle), arg_val(&args, 0)).await?;
            Ok(Value::Null)
        }
        "getProfileItem" => {
            let item = get_profile_item(st, arg_str(&args, 0)).await?;
            Ok(item.unwrap_or(Value::Null))
        }
        "getCurrentProfileItem" => get_current_profile_item(st).await,
        "changeCurrentProfile" => {
            let id = arg_str(&args, 0).unwrap_or("");
            change_current_profile(st, Some(&app_handle), id).await?;
            Ok(Value::Null)
        }
        "addProfileItem" => {
            add_profile_item(st, Some(&app_handle), arg_val(&args, 0)).await?;
            Ok(Value::Null)
        }
        "removeProfileItem" => {
            let id = arg_str(&args, 0).unwrap_or("");
            remove_profile_item(st, Some(&app_handle), id).await?;
            Ok(Value::Null)
        }
        "updateProfileItem" => {
            update_profile_item(st, Some(&app_handle), arg_val(&args, 0)).await?;
            Ok(Value::Null)
        }
        "getProfileStr" => {
            let id = arg_str(&args, 0).unwrap_or("");
            get_profile_str(id).await.map(|s| json!(s))
        }
        "getProfileParseStr" => {
            let id = arg_str(&args, 0).unwrap_or("");
            get_profile_parse_str(st, id).await.map(|s| json!(s))
        }
        "setProfileStr" => {
            let id = arg_str(&args, 0).unwrap_or("");
            let content = arg_str(&args, 1).unwrap_or("");
            set_profile_str(st, Some(&app_handle), id, content).await?;
            Ok(Value::Null)
        }
        "getFileStr" => {
            let id = arg_str(&args, 0).unwrap_or("");
            get_file_str(st, id).await.map(|s| json!(s))
        }
        "setFileStr" => {
            let id = arg_str(&args, 0).unwrap_or("");
            let content = arg_str(&args, 1).unwrap_or("");
            set_file_str(st, id, content).await?;
            Ok(Value::Null)
        }
        "getRuleStr" => {
            let id = arg_str(&args, 0).unwrap_or("");
            get_rule_str(id).await.map(|s| json!(s))
        }
        "setRuleStr" => {
            let id = arg_str(&args, 0).unwrap_or("");
            let content = arg_str(&args, 1).unwrap_or("");
            set_rule_str(id, content).await?;
            Ok(Value::Null)
        }
        "convertMrsRuleset" => {
            let path = arg_str(&args, 0).unwrap_or("");
            let behavior = arg_str(&args, 1).unwrap_or("domain");
            convert_mrs_ruleset(st, path, behavior)
                .await
                .map(|s| json!(s))
        }
        "getRawProfileStr" => get_raw_profile_str(st).await.map(|s| json!(s)),
        "getCurrentProfileStr" => get_current_profile_str(st).await.map(|s| json!(s)),
        "getRuntimeConfigStr" => get_runtime_config_str(st).await.map(|s| json!(s)),
        "getRuntimeConfig" => get_runtime_config(st).await,
        "getEffectivePorts" => Ok(get_effective_ports(st).await),
        "getEffectiveLogLevel" => Ok(json!({
            "value": get_effective_log_level(st).await,
            "profileValue": null,
            "fromProfile": false
        })),

        // Windows Service
        "serviceStatus" => service_status().await.map(|s| json!(s)),
        "testServiceConnection" => test_service_connection(st).await.map(|b| json!(b)),
        "initService" => {
            init_service(st).await?;
            Ok(Value::Null)
        }
        "installService" => {
            install_service().await?;
            Ok(Value::Null)
        }
        "uninstallService" => {
            uninstall_service().await?;
            Ok(Value::Null)
        }
        "startService" => {
            start_service().await?;
            Ok(Value::Null)
        }
        "stopService" => {
            stop_service().await?;
            Ok(Value::Null)
        }
        "restartService" => {
            restart_service().await?;
            Ok(Value::Null)
        }

        // System / Elevation / Utils
        "checkElevateTask" => Ok(json!(check_elevate_task().await)),
        "deleteElevateTask" => {
            delete_elevate_task().await?;
            Ok(Value::Null)
        }
        "isRunningAsAdmin" => Ok(json!(crate::utils::elevation::is_running_as_admin())),
        "needsFirstRunAdmin" => Ok(json!(st.needs_first_run_admin.load(Ordering::SeqCst))),
        "declineElevation" => {
            st.needs_first_run_admin.store(false, Ordering::SeqCst);
            decline_elevation(st).await?;
            Ok(Value::Null)
        }
        "restartAsAdmin" => {
            restart_as_admin(&app_handle, &st).await?;
            Ok(Value::Null)
        }
        "setupFirewall" => {
            setup_firewall().await?;
            Ok(Value::Null)
        }
        "openUWPTool" => {
            open_uwp_tool().await?;
            Ok(Value::Null)
        }
        "getFilePath" => {
            let exts: Vec<String> = args
                .first()
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(Value::as_str)
                        .map(|s| s.trim_start_matches('.').to_string())
                        .collect()
                })
                .unwrap_or_default();
            let ah = app_handle.clone();
            let picked = tokio::task::spawn_blocking(move || get_file_path_dialog(&ah, &exts))
                .await
                .map_err(|e| e.to_string())?;
            Ok(json!(picked))
        }
        "readTextFile" => {
            let path = arg_str(&args, 0).unwrap_or("");
            read_text_file(path).await.map(|s| json!(s))
        }
        "openFile" => {
            let id = arg_str(&args, 0).unwrap_or("");
            open_file_by_id(&app_handle, id)?;
            Ok(Value::Null)
        }
        "openDataDir" => {
            open_data_dir(&app_handle)?;
            Ok(Value::Null)
        }
        "getVersion" => Ok(json!(env!("CARGO_PKG_VERSION"))),
        "getPlatform" => Ok(json!("win32")),
        "getUserAgent" => Ok(json!(get_default_user_agent())),
        "getInterfaces" => Ok(get_interfaces()),
        "copyEnv" => {
            let env_type = arg_str(&args, 0).unwrap_or("powershell");
            copy_env(st, env_type).await?;
            Ok(Value::Null)
        }
        "getAppName" => Ok(json!(get_app_name(arg_str(&args, 0).unwrap_or("")).await)),
        "getIconDataURL" => Ok(json!(get_icon_data_url(arg_str(&args, 0).unwrap_or("")).await)),
        "getImageDataURL" => {
            let url = arg_str(&args, 0).unwrap_or("");
            get_image_data_url(st, url).await.map(|s| json!(s))
        }
        "setLanguage" => {
            if let Some(lang) = arg_str(&args, 0) {
                set_language(lang);
                request_tray_menu_update(st, &app_handle);
            }
            Ok(Value::Null)
        }

        // Window / Theme / Tray / Floating / Shortcuts / Updater
        "setNativeTheme" | "setTitleBarOverlay" | "setDockVisible" | "createHeapSnapshot" => {
            Ok(Value::Null)
        }
        "setAlwaysOnTop" => {
            let on_top = arg_bool(&args, 0, false);
            if let Some(win) = app_handle.get_webview_window("main") {
                let _ = win.set_always_on_top(on_top);
            }
            Ok(Value::Null)
        }
        "isAlwaysOnTop" => {
            let on_top = app_handle
                .get_webview_window("main")
                .and_then(|w| w.is_always_on_top().ok())
                .unwrap_or(false);
            Ok(json!(on_top))
        }
        "windowMinimize" => {
            if let Some(win) = app_handle.get_webview_window("main") {
                let _ = win.minimize();
            }
            Ok(Value::Null)
        }
        "windowMaximize" => {
            if let Some(win) = app_handle.get_webview_window("main") {
                if win.is_maximized().unwrap_or(false) {
                    let _ = win.unmaximize();
                } else {
                    let _ = win.maximize();
                }
            }
            Ok(Value::Null)
        }
        "windowClose" | "closeMainWindow" => {
            if let Some(win) = app_handle.get_webview_window("main") {
                let _ = win.hide();
            }
            Ok(Value::Null)
        }
        "windowIsMaximized" => {
            let max = app_handle
                .get_webview_window("main")
                .and_then(|w| w.is_maximized().ok())
                .unwrap_or(false);
            Ok(json!(max))
        }
        "openDevTools" => {
            if let Some(win) = app_handle.get_webview_window("main") {
                win.open_devtools();
            }
            Ok(Value::Null)
        }
        "showMainWindow" => {
            if let Some(win) = app_handle.get_webview_window("main") {
                let _ = win.unminimize();
                let _ = win.show();
                let _ = win.set_focus();
            }
            Ok(Value::Null)
        }
        "triggerMainWindow" => {
            trigger_main_window(&app_handle);
            Ok(Value::Null)
        }
        "showFloatingWindow" => {
            show_floating_window(st, &app_handle).await?;
            Ok(Value::Null)
        }
        "closeFloatingWindow" => {
            close_floating_window(st, &app_handle).await?;
            Ok(Value::Null)
        }
        "showTrayIcon" => {
            show_tray_icon(st, &app_handle).await?;
            Ok(Value::Null)
        }
        "closeTrayIcon" => {
            close_tray_icon(&app_handle).await?;
            Ok(Value::Null)
        }
        "updateTrayIcon" => {
            update_tray_icon(st, &app_handle).await?;
            Ok(Value::Null)
        }
        "showContextMenu" => {
            show_context_menu(&app_handle);
            Ok(Value::Null)
        }
        "registerShortcut" => {
            let old_s = arg_str(&args, 0).unwrap_or("");
            let new_s = arg_str(&args, 1).unwrap_or("");
            let action = arg_str(&args, 2).unwrap_or("");
            let ok = register_shortcut(st, &app_handle, old_s, new_s, action).await?;
            Ok(json!(ok))
        }
        "resolveThemes" => resolve_themes().await.map(|v| json!(v)),
        "fetchThemes" => {
            fetch_themes(st).await?;
            Ok(Value::Null)
        }
        "importThemes" => {
            let files: Vec<String> = args
                .first()
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            import_themes(files).await?;
            Ok(Value::Null)
        }
        "readTheme" => {
            let t = arg_str(&args, 0).unwrap_or("");
            read_theme(t).await.map(|s| json!(s))
        }
        "writeTheme" => {
            let t = arg_str(&args, 0).unwrap_or("");
            let css = arg_str(&args, 1).unwrap_or("");
            write_theme(t, css).await?;
            Ok(Value::Null)
        }
        "applyTheme" => {
            let t = arg_str(&args, 0).unwrap_or("default.css");
            apply_theme(st, &app_handle, t).await?;
            Ok(Value::Null)
        }
        "checkUpdate" => check_update().await.map(|v| v.unwrap_or(Value::Null)),
        "downloadAndInstallUpdate" => {
            let v = arg_str(&args, 0).unwrap_or("");
            download_and_install_update(st, &app_handle, v).await?;
            Ok(Value::Null)
        }
        "cancelUpdate" => {
            cancel_update(st).await?;
            Ok(Value::Null)
        }
        "notDialogQuit" => {
            st.not_quit_dialog.store(true, Ordering::SeqCst);
            Ok(Value::Null)
        }
        "quitWithoutCore" => {
            st.not_quit_dialog.store(true, Ordering::SeqCst);
            st.is_quitting.store(true, Ordering::SeqCst);
            app_handle.exit(0);
            Ok(Value::Null)
        }
        "quitApp" => {
            request_quit_or_confirm(st, &app_handle).await;
            Ok(Value::Null)
        }
        "relaunchApp" => {
            st.not_quit_dialog.store(true, Ordering::SeqCst);
            st.is_quitting.store(true, Ordering::SeqCst);
            let _ = stop_core(st, Some(&app_handle)).await;
            let _ = disable_sys_proxy(st, false).await;
            app_handle.restart();
        }

        _ => Err(format!("Unsupported IPC channel: {channel}")),
    }
}

#[tauri::command]
pub async fn ipc_send(
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
    channel: String,
    args: Option<Vec<Value>>,
) -> Result<(), String> {
    let args = args.unwrap_or_default();
    let st = state.inner();

    match channel.as_str() {
        "updateTrayMenu" => {
            request_tray_menu_update(st, &app_handle);
        }
        "updateFloatingWindow" => {
            let _ = app_handle.emit("ipc-floatingUpdated", json!([]));
        }
        "quit-confirm-result" => {
            let confirmed = arg_bool(&args, 0, false);
            if let Some(tx) = st.quit_confirm_tx.lock().await.take() {
                let _ = tx.send(confirmed);
            }
        }
        "profile-install-confirm-result" => {
            let confirmed = arg_bool(&args, 0, false);
            if let Some(tx) = st.install_confirm_tx.lock().await.take() {
                let _ = tx.send(confirmed);
            }
        }
        _ => {}
    }
    Ok(())
}
