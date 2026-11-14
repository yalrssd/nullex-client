use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::client::conn::http1;
use hyper::{Method, Request, StatusCode};
use hyper_util::rt::TokioIo;
use serde_json::Value;
use std::time::Duration;

#[cfg(windows)]
pub async fn open_named_pipe(pipe_path: &str) -> Result<tokio::net::windows::named_pipe::NamedPipeClient, String> {
    use tokio::net::windows::named_pipe::ClientOptions;

    let mut attempts = 0u32;
    loop {
        match ClientOptions::new().open(pipe_path) {
            Ok(client) => return Ok(client),
            Err(e) if (e.raw_os_error() == Some(231) || e.raw_os_error() == Some(536)) && attempts < 40 => {
                attempts += 1;
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(e) => return Err(format!("Pipe open failed ({pipe_path}, os_error={:?}): {e}", e.raw_os_error())),
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
