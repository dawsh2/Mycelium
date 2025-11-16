use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

use crate::bus::MessageBus;
use crate::socket_endpoint::{EndpointStats, SocketEndpointHandle};
use crate::{service, ServiceContext};
use mycelium_protocol::SCHEMA_DIGEST;

/// Configuration for launching Python workers (optional).
#[derive(Debug, Clone)]
pub struct PythonChildConfig {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub working_dir: Option<PathBuf>,
}

impl Default for PythonChildConfig {
    fn default() -> Self {
        Self {
            program: "python3".to_string(),
            args: Vec::new(),
            env: Vec::new(),
            working_dir: None,
        }
    }
}

/// Bridge configuration.
#[derive(Debug, Clone)]
pub struct PythonBridgeConfig {
    pub socket_path: PathBuf,
    pub schema_digest: [u8; 32],
    pub child: Option<PythonChildConfig>,
}

impl PythonBridgeConfig {
    pub fn new<P: AsRef<Path>>(socket_path: P) -> Self {
        Self {
            socket_path: socket_path.as_ref().to_path_buf(),
            schema_digest: SCHEMA_DIGEST,
            child: None,
        }
    }
}

/// Service that binds a Unix socket bridge for Python clients and
/// optionally supervises a Python worker process.
pub struct PythonBridgeService {
    bus: MessageBus,
    config: PythonBridgeConfig,
    handle: Option<SocketEndpointHandle>,
    child: Option<Child>,
    stats: Arc<EndpointStats>,
}

impl PythonBridgeService {
    pub fn new(bus: MessageBus, config: PythonBridgeConfig) -> Self {
        Self {
            bus,
            config,
            handle: None,
            child: None,
            stats: Arc::new(EndpointStats::default()),
        }
    }

    async fn spawn_child(&mut self, ctx: &ServiceContext, cfg: &PythonChildConfig) -> Result<()> {
        let mut cmd = Command::new(&cfg.program);
        cmd.args(&cfg.args);
        cmd.env("MYCELIUM_SOCKET", &self.config.socket_path);
        cmd.env(
            "MYCELIUM_SCHEMA_DIGEST",
            hex_digest(&self.config.schema_digest),
        );
        for (k, v) in &cfg.env {
            cmd.env(k, v);
        }
        if let Some(dir) = &cfg.working_dir {
            cmd.current_dir(dir);
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn()?;
        if let Some(stdout) = child.stdout.take() {
            tokio::spawn(log_pipe(stdout, "python-stdout"));
        }
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(log_pipe(stderr, "python-stderr"));
        }

        ctx.info(&format!(
            "Spawned Python worker '{}' for socket {}",
            cfg.program,
            self.config.socket_path.display()
        ));
        self.child = Some(child);
        Ok(())
    }
}

#[service]
impl PythonBridgeService {
    async fn run(&mut self, ctx: &ServiceContext) -> Result<()> {
        self.stats.attach_metrics(ctx.metrics().clone());
        ctx.info(&format!(
            "Binding Python bridge socket at {}",
            self.config.socket_path.display()
        ));
        let handle = self
            .bus
            .bind_unix_endpoint_with_digest_and_stats(
                &self.config.socket_path,
                self.config.schema_digest,
                Some(self.stats.clone()),
            )
            .await?;
        self.handle = Some(handle);

        if let Some(child_cfg) = self.config.child.clone() {
            self.spawn_child(ctx, &child_cfg).await?;
        }

        while !ctx.is_shutting_down() {
            ctx.sleep(Duration::from_secs(5)).await;
            self.log_stats();
        }

        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await;
        }
        if let Some(handle) = self.handle.take() {
            handle.shutdown().await?;
        }
        self.log_stats();
        Ok(())
    }
}

async fn log_pipe<T>(reader: T, label: &'static str)
where
    T: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        tracing::info!(bridge = label, "{}", line);
    }
}

fn hex_digest(bytes: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for b in bytes {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

impl PythonBridgeService {
    fn log_stats(&self) {
        let (connections, failures, frames) = self.stats.snapshot();
        tracing::info!(
            connections,
            handshake_failures = failures,
            frames_forwarded = frames,
            "python bridge stats"
        );
    }
}
