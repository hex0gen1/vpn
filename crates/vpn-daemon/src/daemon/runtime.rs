use crate::linux::tun::{TunInterface, create_interface};
use anyhow::Result;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::signal::unix::{Signal, SignalKind};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug)]
pub enum RuntimeStatus {
    Starting,
    Running,
    Stopping,
    Stopped,
    Failed(String),
}
pub type Packet = Vec<u8>;
pub struct RuntimeStatistics {
    packets_from_tun: AtomicU64,
    bytes_from_tun: AtomicU64,
    packets_to_tun: AtomicU64,
    bytes_to_tun: AtomicU64,
    packets_from_transport: AtomicU64,
    bytes_from_transport: AtomicU64,
    bytes_to_transport: AtomicU64,
    packets_to_transport: AtomicU64,
    errors: AtomicU64,
    last_error: std::sync::Mutex<Option<String>>,
}
#[derive(Debug, Clone)]
pub struct RuntimeStatsSnapshot {
    pub packets_from_tun: u64,
    pub bytes_from_tun: u64,
    pub packets_to_tun: u64,
    pub bytes_to_tun: u64,
    pub packets_from_transport: u64,
    pub bytes_from_transport: u64,
    pub packets_to_transport: u64,
    pub bytes_to_transport: u64,
    pub errors: u64,
    pub last_error: Option<String>,
}
impl RuntimeStatistics {
    pub fn new() -> Self {
        Self {
            packets_from_tun: AtomicU64::new(0),
            bytes_from_tun: AtomicU64::new(0),
            packets_to_tun: AtomicU64::new(0),
            bytes_to_tun: AtomicU64::new(0),
            packets_from_transport: AtomicU64::new(0),
            bytes_from_transport: AtomicU64::new(0),
            packets_to_transport: AtomicU64::new(0),
            bytes_to_transport: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            last_error: std::sync::Mutex::new(None),
        }
    }
    pub fn on_tun_rx(&self, n: usize) {
        self.packets_from_tun
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.bytes_from_tun
            .fetch_add(n as u64, std::sync::atomic::Ordering::Relaxed);
    }
    pub fn on_tun_tx(&self, n: usize) {
        self.packets_to_tun.fetch_add(1, Ordering::Relaxed);
        self.bytes_to_tun.fetch_add(n as u64, Ordering::Relaxed);
    }
    pub fn on_transport_tx(&self, n: usize) {
        self.packets_to_transport.fetch_add(1, Ordering::Relaxed);
        self.bytes_to_transport
            .fetch_add(n as u64, Ordering::Relaxed);
    }
    pub fn on_trasport_rx(&self, n: usize) {
        self.packets_from_transport.fetch_add(1, Ordering::Relaxed);
        self.bytes_from_transport
            .fetch_add(n as u64, Ordering::Relaxed);
    }
    pub fn on_error(&mut self, error: &std::io::Error) {
        self.last_error = std::sync::Mutex::from(Some(error.to_string()));
    }
}

#[derive(Clone, Debug)]
pub struct RuntimePlan {
    pub tun_name: String,
}
pub struct RuntimeTask {
    join: JoinHandle<Result<()>>,
}

impl RuntimeTask {
    pub fn new(join: JoinHandle<Result<()>>) -> Self {
        Self { join }
    }

    pub async fn wait(self) -> Result<()> {
        self.join.await?
    }
}
pub struct Runtime;
impl RuntimePlan {
    fn new(tun_dec_name: String) -> Self {
        Self {
            tun_name: tun_dec_name,
        }
    }
}
impl Runtime {
    pub async fn start(plan: RuntimePlan) -> anyhow::Result<(RuntimeHandle, RuntimeTask)> {
        let (status_tx, status_rx) = watch::channel(RuntimeStatus::Starting);
        let (tun_to_transport_tx, tun_to_transport_rx) = tokio::sync::mpsc::channel::<Packet>(256);

        let shutdown = CancellationToken::new();
        let shutdown_child = shutdown.child_token();
        let (fd, real_name) = create_interface("tun0".trim_start())?;

        let tun = TunInterface::new(fd, real_name)?;

        let join = tokio::spawn({
            let status_tx = status_tx.clone();

            async move {
                let _ = status_tx.send(RuntimeStatus::Running);
                let stats = RuntimeStatistics::new();
                let result = run_tun_loop(tun, shutdown_child, status_tx.clone(), stats).await;

                match &result {
                    Ok(()) => {
                        let _ = status_tx.send(RuntimeStatus::Stopped);
                    }
                    Err(err) => {
                        let _ = status_tx.send(RuntimeStatus::Failed(err.to_string()));
                    }
                }
                result
            }
        });
        let task = RuntimeTask::new(join);
        let handle = RuntimeHandle::new(shutdown, status_rx);
        Ok((handle, task))
    }
}
#[derive(Clone, Debug)]
pub struct RuntimeHandle {
    pub(crate) shutdown: CancellationToken,
    pub(crate) status_rx: watch::Receiver<RuntimeStatus>,
}
impl RuntimeHandle {
    pub fn new(shutdown: CancellationToken, status_rx: watch::Receiver<RuntimeStatus>) -> Self {
        Self {
            shutdown,
            status_rx,
        }
    }
    pub fn stop(&self) {
        self.shutdown.cancel();
    }
    pub fn status(&self) -> RuntimeStatus {
        self.status_rx.borrow().clone()
    }
    pub async fn changed(&mut self) -> Result<RuntimeStatus> {
        self.status_rx.changed().await?;
        Ok(self.status())
    }
}
pub async fn run_tun_loop(
    tun: TunInterface,
    shutdown: CancellationToken,
    status_tx: watch::Sender<RuntimeStatus>,
    mut stats: RuntimeStatistics,
) -> Result<()> {
    let mut buf = vec![0u8; 65535];
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => {
                let _ = status_tx.send(RuntimeStatus::Stopping);
                break;
            }

            res = tun.read_packet(&mut buf) => {
                match res {
                    Ok(n) => {
                        stats.on_tun_rx(n);
                        tracing::debug!(
                        iface = tun.name(),
                        bytes = n,
                        "packet received from TUN"
                        );
                    }
                    Err(err) => {
                        stats.on_error(&err);
                        return Err(err.into())
                    }


                }
            }

        }
    }
    Ok(())
}
