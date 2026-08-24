//! A minimal HTTP client over the Unix socket.
//!
//! Hand-written rather than pulling `hyper` and a connector stack. The daemon
//! is on the same machine over a Unix socket, requests are small, and there is
//! no TLS, no redirects, no proxies and no connection pooling to get right —
//! the entire surface is "write a request, read a response". A dependency for
//! that would cost more than it saves.
//!
//! HTTP/1.1 with `Connection: close`, so there is no keep-alive state machine.

use anyhow::{Context, Result, bail};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

pub struct Client {
    socket: PathBuf,
}

pub struct Response {
    pub status: u16,
    pub body: String,
}

impl Client {
    /// Connect to the daemon, starting one if it is not already running.
    ///
    /// Autostart is what keeps the API-only rule (§3.1) from being hostile:
    /// `aios project list` must not fail because launchd was never set up. The
    /// same pattern as `docker` and `tailscale` — the daemon is an
    /// implementation detail of the CLI until you deliberately install it.
    pub fn connect() -> Result<Self> {
        let socket = crate::daemon::socket_path();
        if socket.exists() && UnixStream::connect(&socket).is_ok() {
            return Ok(Self { socket });
        }
        // A socket file with nothing behind it is a daemon that died badly.
        // Removing it here means the next bind succeeds instead of failing with
        // "address already in use".
        if socket.exists() {
            let _ = std::fs::remove_file(&socket);
        }
        Self::autostart(socket)
    }

    /// Connect without starting anything — for `daemon status`, which must be
    /// able to report "not running" rather than causing it to run.
    pub fn connect_existing() -> Result<Self> {
        let socket = crate::daemon::socket_path();
        UnixStream::connect(&socket)
            .with_context(|| format!("no daemon at {}", socket.display()))?;
        Ok(Self { socket })
    }

    fn autostart(socket: PathBuf) -> Result<Self> {
        let binary = std::env::current_exe().context("locating the aios binary")?;
        std::process::Command::new(&binary)
            .arg("serve")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .context("starting the daemon")?;

        // Poll rather than sleeping a fixed amount: startup is usually
        // milliseconds, and a fixed delay would be both slower and less
        // reliable than waiting for the thing we actually need.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            if UnixStream::connect(&socket).is_ok() {
                return Ok(Self { socket });
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        bail!("daemon did not come up within 10s — try `aios serve` directly to see why")
    }

    /// Invoke a capability through the daemon.
    ///
    /// Every capability-backed command goes through here, which is what makes
    /// the API-only rule real rather than aspirational: there is one call site,
    /// and it speaks HTTP.
    pub fn call_capability(
        &self,
        name: &str,
        input: serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.post(&format!("/api/capabilities/{name}"), &input)
    }

    pub fn health(&self) -> Result<()> {
        let response = self.request("GET", "/api/health", None)?;
        if response.status != 200 {
            bail!("daemon answered {}", response.status);
        }
        Ok(())
    }

    pub fn get(&self, path: &str) -> Result<serde_json::Value> {
        self.json(self.request("GET", path, None)?)
    }

    pub fn post(&self, path: &str, body: &serde_json::Value) -> Result<serde_json::Value> {
        self.json(self.request("POST", path, Some(body))?)
    }

    /// Turn a response into JSON, converting a typed `ApiError` body back into
    /// an error rather than handing the caller a success-shaped failure.
    fn json(&self, response: Response) -> Result<serde_json::Value> {
        let value: serde_json::Value = serde_json::from_str(&response.body)
            .with_context(|| format!("daemon returned {}: {}", response.status, response.body))?;
        if response.status >= 400 {
            let message = value["message"].as_str().unwrap_or(&response.body);
            bail!("{message}");
        }
        Ok(value)
    }

    /// Follow an SSE stream, calling `on_record` for each event until the
    /// server signals `done`.
    ///
    /// A hand-rolled SSE reader for the same reason as the rest of this client:
    /// the format is `field: value` lines separated by blank lines, and the
    /// only fields we need are `event` and `data`.
    pub fn stream(&self, path: &str, mut on_record: impl FnMut(serde_json::Value)) -> Result<()> {
        let mut stream = UnixStream::connect(&self.socket)
            .with_context(|| format!("connecting to {}", self.socket.display()))?;
        stream.write_all(
            format!("GET {path} HTTP/1.1\r\nHost: aios\r\nAccept: text/event-stream\r\n\r\n")
                .as_bytes(),
        )?;
        stream.flush()?;

        let mut reader = BufReader::new(stream);
        // Skip the response head.
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line)? == 0 || line == "\r\n" || line == "\n" {
                break;
            }
        }

        let mut event_name = String::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line)? == 0 {
                break;
            }
            let line = line.trim_end_matches(['\r', '\n']);
            if let Some(name) = line.strip_prefix("event:") {
                event_name = name.trim().to_string();
            } else if let Some(payload) = line.strip_prefix("data:") {
                if event_name == "done" {
                    break;
                }
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(payload.trim()) {
                    on_record(value);
                }
            } else if line.is_empty() {
                event_name.clear();
            }
        }
        Ok(())
    }

    fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<Response> {
        let mut stream = UnixStream::connect(&self.socket)
            .with_context(|| format!("connecting to {}", self.socket.display()))?;

        let payload = body.map(|b| b.to_string()).unwrap_or_default();
        let mut request = format!(
            "{method} {path} HTTP/1.1\r\nHost: aios\r\nConnection: close\r\nAccept: application/json\r\n"
        );
        if body.is_some() {
            request.push_str("Content-Type: application/json\r\n");
        }
        // Always send Content-Length, including 0 — axum treats a POST with no
        // length as having no body, and an empty JSON body then fails to parse
        // rather than deserializing as `{}`.
        request.push_str(&format!("Content-Length: {}\r\n\r\n", payload.len()));
        request.push_str(&payload);

        stream.write_all(request.as_bytes())?;
        stream.flush()?;

        let mut reader = BufReader::new(stream);
        let mut status_line = String::new();
        reader.read_line(&mut status_line)?;
        let status: u16 = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .with_context(|| format!("unparseable status line: {status_line:?}"))?;

        // Skip headers. `Connection: close` means the body runs to EOF, so no
        // chunked decoding or content-length tracking is needed.
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line)? == 0 || line == "\r\n" || line == "\n" {
                break;
            }
        }
        let mut body = String::new();
        reader.read_to_string(&mut body)?;
        Ok(Response { status, body })
    }
}
