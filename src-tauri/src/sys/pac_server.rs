use crate::config::app::get_app_config;
use crate::core::factory::get_runtime_config;
use crate::state::AppState;
use serde_json::Value;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

const DEFAULT_PAC: &str = r#"function FindProxyForURL(url, host) {
  return "PROXY 127.0.0.1:%mixed-port%; SOCKS5 127.0.0.1:%mixed-port%; DIRECT;";
}"#;

async fn bind_pac_listener() -> Result<TcpListener, String> {
    for port in 10000u16..=10100u16 {
        if let Ok(listener) = TcpListener::bind(("127.0.0.1", port)).await {
            return Ok(listener);
        }
    }
    TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("Failed binding PAC server: {e}"))
}

pub async fn start_pac_server(state: &Arc<AppState>) -> Result<u16, String> {
    let existing = state.pac_port.load(Ordering::SeqCst);
    if existing != 0 {
        return Ok(existing);
    }

    let listener = bind_pac_listener().await?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("Failed reading PAC local_addr: {e}"))?
        .port();
    state.pac_port.store(port, Ordering::SeqCst);

    let (tx, mut rx) = oneshot::channel::<()>();
    *state.pac_shutdown.lock().await = Some(tx);

    let state_clone = state.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut rx => {
                    break;
                }
                accept_res = listener.accept() => {
                    let Ok((mut socket, _)) = accept_res else {
                        break;
                    };
                    let st = state_clone.clone();
                    tokio::spawn(async move {
                        let mut buf = [0u8; 2048];
                        let n = socket.read(&mut buf).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        let req_str = String::from_utf8_lossy(&buf[..n]);
                        let first_line = req_str.lines().next().unwrap_or("");
                        let path = first_line.split_whitespace().nth(1).unwrap_or("/");

                        if path.starts_with("/pac") {
                            let app_cfg = get_app_config(&st, false).await.unwrap_or_default();
                            let script_tpl = app_cfg
                                .get("sysProxy")
                                .and_then(|s| s.get("pacScript"))
                                .and_then(Value::as_str)
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                                .unwrap_or(DEFAULT_PAC);

                            let mixed_port = if let Some(p) = *st.last_mixed_port.lock().await {
                                p
                            } else {
                                get_runtime_config(&st)
                                    .await
                                    .ok()
                                    .and_then(|r| r.get("mixed-port").and_then(Value::as_u64))
                                    .filter(|&p| p > 0)
                                    .unwrap_or(7897) as u16
                            };

                            let body = script_tpl.replace("%mixed-port%", &mixed_port.to_string());
                            let resp = format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: application/x-ns-proxy-autoconfig; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                body.len(),
                                body
                            );
                            let _ = socket.write_all(resp.as_bytes()).await;
                        } else {
                            let resp = "HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\nConnection: close\r\n\r\nNot Found";
                            let _ = socket.write_all(resp.as_bytes()).await;
                        }
                    });
                }
            }
        }
        state_clone.pac_port.store(0, Ordering::SeqCst);
    });

    Ok(port)
}

pub async fn stop_pac_server(state: &Arc<AppState>) {
    if let Some(tx) = state.pac_shutdown.lock().await.take() {
        let _ = tx.send(());
    }
    state.pac_port.store(0, Ordering::SeqCst);
}
