//! Async client for talking to a running `chumd`.
//!
//! [`DaemonClient`] takes a socket path and exposes one method per
//! v0.1 verb, plus a low-level [`DaemonClient::request`] for callers
//! that need to drive the protocol directly. Each method opens a
//! fresh connection, sends one JSON-line request, reads one JSON-line
//! response, and closes the connection. Pipelining is not supported
//! in v0.1.

use std::path::{Path, PathBuf};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use crate::error::IpcError;
use crate::ipc::{
    ListProcessesResponse, PROTOCOL_VERSION, PingResponse, ProcessKeyArgs, ProcessStatusResponse,
    Request, Response, RestartProcessResponse, SpawnResponse, StatusResponse, TailLogsArgs,
    TailLogsResponse, TerminateArgs, TerminateResponse,
};

/// One-request-per-connection client for the daemon's IPC socket.
#[derive(Debug, Clone)]
pub struct DaemonClient {
    socket_path: PathBuf,
}

impl DaemonClient {
    /// Construct a client targeting the socket at `path`. No
    /// connection is made until a method is called.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: path.into(),
        }
    }

    /// Path the client connects to. Useful for diagnostic output.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Send a raw [`Request`], read one [`Response`].
    ///
    /// # Errors
    /// - [`IpcError::ConnectFailed`] if the socket cannot be reached.
    /// - [`IpcError::Io`] for read / write failures mid-conversation.
    /// - [`IpcError::Json`] for an unparseable response.
    pub async fn request(&self, req: &Request) -> Result<Response, IpcError> {
        let stream = UnixStream::connect(&self.socket_path).await.map_err(|e| {
            IpcError::ConnectFailed {
                path: self.socket_path.clone(),
                source: e,
            }
        })?;
        let (read_half, mut write_half) = stream.into_split();

        let body = serde_json::to_vec(req)?;
        write_half.write_all(&body).await?;
        write_half.write_all(b"\n").await?;
        write_half.shutdown().await?;

        let mut reader = BufReader::new(read_half);
        let mut resp_buf = Vec::new();
        let n = reader.read_until(b'\n', &mut resp_buf).await?;
        if n == 0 {
            return Err(IpcError::ProtocolError {
                reason: "daemon closed connection without sending a response".to_string(),
            });
        }
        let resp: Response = serde_json::from_slice(&resp_buf).map_err(|e| {
            IpcError::ProtocolError {
                reason: format!("response not valid JSON: {e}"),
            }
        })?;
        Ok(resp)
    }

    /// Send a `ping` verb and decode the typed payload.
    pub async fn ping(&self) -> Result<PingResponse, IpcError> {
        let req = Request {
            protocol_version: PROTOCOL_VERSION,
            verb: "ping".to_string(),
            args: serde_json::Value::Null,
        };
        self.decode_ok(self.request(&req).await?)
    }

    /// Send a `status` verb and decode the typed payload.
    pub async fn status(&self) -> Result<StatusResponse, IpcError> {
        let req = Request {
            protocol_version: PROTOCOL_VERSION,
            verb: "status".to_string(),
            args: serde_json::Value::Null,
        };
        self.decode_ok(self.request(&req).await?)
    }

    /// Send a `list_processes` verb and decode the typed payload.
    pub async fn list_processes(&self) -> Result<ListProcessesResponse, IpcError> {
        let req = Request {
            protocol_version: PROTOCOL_VERSION,
            verb: "list_processes".to_string(),
            args: serde_json::Value::Null,
        };
        self.decode_ok(self.request(&req).await?)
    }

    /// Send a `spawn` verb and decode the typed payload. The daemon
    /// looks up the install_dir in the registry and re-parses
    /// `<install_dir>/chum-manifest.toml` before handing the
    /// resulting Manifest to `Supervisor::spawn`.
    pub async fn spawn_process(
        &self,
        name: impl Into<String>,
        version: impl Into<String>,
    ) -> Result<SpawnResponse, IpcError> {
        let args = ProcessKeyArgs {
            name: name.into(),
            version: version.into(),
        };
        let req = Request {
            protocol_version: PROTOCOL_VERSION,
            verb: "spawn".to_string(),
            args: serde_json::to_value(args)?,
        };
        self.decode_ok(self.request(&req).await?)
    }

    /// Send a `terminate` verb with an optional grace duration.
    /// Defaults the grace to 5 seconds when `grace_secs` is `None`.
    pub async fn terminate_process(
        &self,
        name: impl Into<String>,
        version: impl Into<String>,
        grace_secs: Option<u64>,
    ) -> Result<TerminateResponse, IpcError> {
        let args = TerminateArgs {
            name: name.into(),
            version: version.into(),
            grace_secs,
        };
        let req = Request {
            protocol_version: PROTOCOL_VERSION,
            verb: "terminate".to_string(),
            args: serde_json::to_value(args)?,
        };
        self.decode_ok(self.request(&req).await?)
    }

    /// Send a `restart` verb and decode the typed payload.
    pub async fn restart_process(
        &self,
        name: impl Into<String>,
        version: impl Into<String>,
    ) -> Result<RestartProcessResponse, IpcError> {
        let args = ProcessKeyArgs {
            name: name.into(),
            version: version.into(),
        };
        let req = Request {
            protocol_version: PROTOCOL_VERSION,
            verb: "restart".to_string(),
            args: serde_json::to_value(args)?,
        };
        self.decode_ok(self.request(&req).await?)
    }

    /// Send a `process_status` verb and decode the typed payload.
    pub async fn process_status(
        &self,
        name: impl Into<String>,
        version: impl Into<String>,
    ) -> Result<ProcessStatusResponse, IpcError> {
        let args = ProcessKeyArgs {
            name: name.into(),
            version: version.into(),
        };
        let req = Request {
            protocol_version: PROTOCOL_VERSION,
            verb: "process_status".to_string(),
            args: serde_json::to_value(args)?,
        };
        self.decode_ok(self.request(&req).await?)
    }

    /// Send a `tail_logs` verb. `stream` is one of `"stdout"`,
    /// `"stderr"`, `"both"`. `lines` is capped server-side at
    /// [`crate::ipc::server::MAX_TAIL_LINES`] (10_000).
    pub async fn tail_logs(
        &self,
        name: impl Into<String>,
        version: impl Into<String>,
        stream: impl Into<String>,
        lines: usize,
    ) -> Result<TailLogsResponse, IpcError> {
        let args = TailLogsArgs {
            name: name.into(),
            version: version.into(),
            stream: stream.into(),
            lines,
        };
        let req = Request {
            protocol_version: PROTOCOL_VERSION,
            verb: "tail_logs".to_string(),
            args: serde_json::to_value(args)?,
        };
        self.decode_ok(self.request(&req).await?)
    }

    fn decode_ok<T: serde::de::DeserializeOwned>(
        &self,
        resp: Response,
    ) -> Result<T, IpcError> {
        match resp {
            Response::Ok { data, .. } => {
                serde_json::from_value(data).map_err(|e| IpcError::ProtocolError {
                    reason: format!("response data did not decode into expected shape: {e}"),
                })
            }
            Response::Error { code, message, .. } => {
                Err(IpcError::ServerError { code, message })
            }
        }
    }
}
