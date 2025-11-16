use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::Result;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

use crate::bus::MessageBus;
use crate::socket_endpoint::SocketEndpointHandle;
use crate::{service, ServiceContext};

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
    pub child: Option<PythonChildConfig>,
}

impl PythonBridgeConfig {
    pub fn new<P: AsRef<Path>>(socket_path: P) -> Self {
        Self {
            socket_path: socket_path.as_ref().to_path_buf(),
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
}

impl PythonBridgeService {
    pub fn new(bus: MessageBus, config: PythonBridgeConfig) -> Self {
        Self {
            bus,
            config,
            handle: None,
            child: None,
        }
    }

    async fn spawn_child(&mut self, ctx: &ServiceContext, cfg: &PythonChildConfig) -> Result<()> {
        let mut cmd = Command::new(&cfg.program);
        cmd.args(&cfg.args);
        cmd.env("MYCELIUM_SOCKET", &self.config.socket_path);
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
        ctx.info(&format!(
            "Binding Python bridge socket at {}",
            self.config.socket_path.display()
        ));
        let handle = self.bus.bind_unix_endpoint(&self.config.socket_path).await?;
        self.handle = Some(handle);

        if let Some(child_cfg) = self.config.child.clone() {
            self.spawn_child(ctx, &child_cfg).await?;
        }

        while !ctx.is_shutting_down() {
            ctx.sleep(Duration::from_secs(1)).await;
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
