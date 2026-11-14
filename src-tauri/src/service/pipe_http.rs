use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::client::conn::http1;
use hyper::{Method, Request, StatusCode};
use hyper_util::rt::TokioIo;
use serde_json::Value;
use std::time::Duration;

static PIPE_MUTEX: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();

#[cfg(windows)]
pub async fn open_named_pipe(pipe_path: &str) -> Result<tokio::net::windows::named_pipe::NamedPipeClient, String> {
    use tokio::net::windows::named_pipe::ClientOptions;

    let mut attempts = 0u32;
    loop {
        match ClientOptions::new().open(pipe_path) {
            Ok(client) => return Ok(client),
            Err(e) => {
                let code = e.raw_os_error();
                // 2 = ERROR_FILE_NOT_FOUND (pipe not yet created / restarting)
                // 109 = ERROR_BROKEN_PIPE (pipe ended)
                // 231 = ERROR_PIPE_BUSY (all instances busy)
                // 232 = ERROR_NO_DATA (pipe closing)
                // 233 = ERROR_PIPE_NOT_CONNECTED (not connected yet)
                // 536 = ERROR_PIPE_LISTENING (pipe in listening state)
                let is_transient = matches!(code, Some(2 | 109 | 231 | 232 | 233 | 536));
                if is_transient && attempts < 60 {
                    attempts += 1;
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                }
                return Err(format!("Pipe open failed ({pipe_path}, os_error={:?}): {e}", code));
            }
        }
    }
}

#[derive(Debug)]
pub enum PipeHttpError {
    Unreachable(String),
    Controller { status: StatusCode, body: String },
    Other(String),
}

impl PipeHttpError {
    pub fn into_user_message(self) -> String {
        match self {
            PipeHttpError::Unreachable(_) => crate::utils::i18n::tr("error.coreUnreachable").to_string(),
            PipeHttpError::Controller { body, .. } => {
                if let Ok(v) = serde_json::from_str::<Value>(&body) {
                    if let Some(msg) = v.get("message").and_then(Value::as_str) {
                        return msg.to_string();
                    }
                }
                body
            }
            PipeHttpError::Other(m) => m,
        }
    }

    pub fn is_controller_rejection(&self) -> bool {
        matches!(self, PipeHttpError::Controller { .. })
    }
}

pub async fn pipe_request(
    pipe_path: &str,
    method: Method,
    uri_path: &str,
    body: Option<Value>,
    extra_headers: &[(&str, String)],
    timeout_secs: u64,
) -> Result<Value, PipeHttpError> {
    let _lock = PIPE_MUTEX
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;

    let fut = async {
        #[cfg(windows)]
        let stream = open_named_pipe(pipe_path)
            .await
            .map_err(PipeHttpError::Unreachable)?;
        #[cfg(not(windows))]
        let stream = tokio::net::UnixStream::connect(pipe_path)
            .await
            .map_err(|e| PipeHttpError::Unreachable(e.to_string()))?;

        let io = TokioIo::new(stream);
        let (mut sender, conn) = http1::handshake(io)
            .await
            .map_err(|e| PipeHttpError::Other(format!("HTTP handshake failed: {e}")))?;

        // Driver task with guaranteed abort guard so pipe handle is closed immediately on finish/timeout
        let conn_task = tokio::spawn(async move {
            let _ = conn.await;
        });
        struct ConnGuard(tokio::task::JoinHandle<()>);
        impl Drop for ConnGuard {
            fn drop(&mut self) {
                self.0.abort();
            }
        }
        let _guard = ConnGuard(conn_task);

        let payload_bytes = match body {
            Some(v) => Bytes::from(v.to_string()),
            None => Bytes::new(),
        };

        let mut builder = Request::builder()
            .method(method)
            .uri(uri_path)
            .header("Host", "localhost")
            .header("Connection", "close");

        if !payload_bytes.is_empty() {
            builder = builder.header("Content-Type", "application/json");
        }
        for (k, v) in extra_headers {
            builder = builder.header(*k, v.as_str());
        }

        let req = builder
            .body(Full::new(payload_bytes))
            .map_err(|e| PipeHttpError::Other(format!("Build request failed: {e}")))?;

        let resp = sender
            .send_request(req)
            .await
            .map_err(|e| PipeHttpError::Other(format!("Send request failed: {e}")))?;

        let status = resp.status();
        let collected = resp
            .into_body()
            .collect()
            .await
            .map_err(|e| PipeHttpError::Other(format!("Read response body failed: {e}")))?;
        let text = String::from_utf8_lossy(&collected.to_bytes()).to_string();

        if !status.is_success() {
            return Err(PipeHttpError::Controller { status, body: text });
        }

        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        Ok(serde_json::from_str(&text).unwrap_or(Value::String(text)))
    };

    tokio::time::timeout(Duration::from_secs(timeout_secs), fut)
        .await
        .map_err(|_| PipeHttpError::Other("Pipe HTTP request timed out".to_string()))?
}
