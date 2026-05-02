use crate::app::ConnectionState;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc, watch};
use vpn_daemon::linux::tun::TunInterface;
use vpn_daemon::transport::server::{ServerState, TrafficCounters, TrafficSnapshot};
use vpn_types::VpnProfile;
//backend struct for tui -> backend communication
#[derive(Debug, Clone)]
pub enum UiCommand {
    Connect(VpnProfile),
    Disconnect,
    AddProfile(VpnProfile),
    RemoveProfile(usize),
    RequestLogs,
    RequestMetrics,
}

#[derive(Debug, Clone)]
pub enum BackendEvent {
    LogAdded {
        level: LogLevel,
        message: String,
        source: String,
    },
    MetricsUpdated {
        rx_bytes: u64,
        tx_bytes: u64,
        peers: usize,
    },
    ConnectionStateChanged(crate::app::ConnectionState),
    ProfileAdded(VpnProfile),
    ProfileRemoved(usize),
    Error {
        code: String,
        message: String,
    },
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BackendState {
    pub connections: crate::app::ConnectionState,
    pub active_profile: Option<String>,
    pub peer_count: usize,
    pub uptime_dur: u64,
    pub last_error: Option<String>,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_packets: u64,
    pub tx_packets: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}
impl LogLevel {
    pub fn as_str(&self) -> String {
        match self {
            LogLevel::Debug => "Debug".into(),
            LogLevel::Warn => "Warn".into(),
            LogLevel::Info => "Info".into(),
            LogLevel::Error => "Error".into(),
        }
    }
}
#[derive(Debug)]
pub struct BackendHandle {
    pub cmd_tx: mpsc::Sender<UiCommand>,
    pub state_rx: watch::Receiver<BackendState>,
    pub event_rx: broadcast::Receiver<BackendEvent>,
    pub metrics_rx: watch::Receiver<TrafficSnapshot>,
}
#[derive(Debug)]
pub struct Backend {
    pub cmd_rx: mpsc::Receiver<UiCommand>,
    pub state_tx: watch::Sender<BackendState>,
    pub event_tx: broadcast::Sender<BackendEvent>,
    pub state: BackendState,
    pub server_state: std::sync::Arc<ServerState>,
    pub metrics: std::sync::Arc<TrafficCounters>,
    pub active_cancel: Option<tokio_util::sync::CancellationToken>,
    pub tun: Arc<tokio::sync::Mutex<TunInterface>>,
    pub crypto: Arc<vpn_daemon::transport::frame::CryptoState>,
}

//implementation of structs
pub struct BackendConfig {
    pub subnet: std::net::Ipv4Addr,
    pub pool_size: u32,
    pub tun_name: String,
}
impl BackendHandle {
    pub fn new(
        cmd_tx: mpsc::Sender<UiCommand>,
        state_rx: watch::Receiver<BackendState>,
        event_rx: broadcast::Receiver<BackendEvent>,
        metrics_rx: watch::Receiver<TrafficSnapshot>,
    ) -> Self {
        Self {
            cmd_tx,
            state_rx,
            event_rx,
            metrics_rx,
        }
    }
    pub fn try_send_cmd(&self, cmd: UiCommand) -> anyhow::Result<()> {
        match self.cmd_tx.try_send(cmd) {
            Ok(_) => tracing::debug!("[HANDLE] Cmd sended."),
            Err(e) => tracing::error!("[HANDLE] Error while sending command: {}", e),
        };
        Ok(())
    }
    pub async fn send_cmd(&self, cmd: UiCommand) -> anyhow::Result<()> {
        self.cmd_tx
            .send(cmd)
            .await
            .map_err(|_| anyhow::anyhow!("Backend channel closed"))
    }
    pub fn current_state(&self) -> BackendState {
        self.state_rx.borrow().clone()
    }
    pub fn subscribe_events(&self) -> broadcast::Receiver<BackendEvent> {
        self.event_rx.resubscribe()
    }
}
impl Backend {
    pub async fn run(mut self) {
        let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(10));

        loop {
            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    self.handle_command(cmd).await;
                }

                _ = heartbeat.tick() => {
                    self.state.uptime_dur += 10;
                    self.state.peer_count = self.server_state.peer_count().await;
                    self.state_tx.send_replace(self.state.clone());
                }
            }
        }
    }
    pub fn new(
        cfg: BackendConfig,
        tun: Arc<tokio::sync::Mutex<TunInterface>>,
        crypto: Arc<vpn_daemon::transport::frame::CryptoState>,
    ) -> (Self, BackendHandle) {
        let (cmd_tx, cmd_rx) = mpsc::channel(64);
        let (state_tx, state_rx) = watch::channel(BackendState::default());
        //event doesnt need rx, cuz it just sends events from tui. for communication used
        //cmd_rx,state_rx;
        let (event_tx, _) = broadcast::channel(256);
        let (metrics_tx, metrix_rx) = watch::channel(TrafficSnapshot::default());

        let backend = Self {
            cmd_rx: cmd_rx,
            state_tx: state_tx,
            event_tx: event_tx.clone(),
            state: BackendState::default(),
            server_state: std::sync::Arc::new(ServerState::new(cfg.subnet, cfg.pool_size)),
            metrics: std::sync::Arc::new(TrafficCounters::default()),
            active_cancel: Some(tokio_util::sync::CancellationToken::new()),
            tun,
            crypto,
        };
        let metrics_clone = backend.metrics.clone();
        let state_tx_clone = backend.state_tx.clone();
        let handle = BackendHandle::new(cmd_tx, state_rx, event_tx.subscribe(), metrix_rx);
        tokio::spawn(metrics_reporter(metrics_clone, state_tx_clone));
        (backend, handle)
    }
    pub async fn handle_command(&mut self, cmd: UiCommand) {
        match cmd {
            UiCommand::Connect(profile) => {
                tracing::info!("[BACKEND] Handling connection command.");
                //if self.active_cancel.is_some() {
                //    return;
                //}
                let server_addr = match profile.host.parse::<std::net::IpAddr>() {
                    Ok(ip) => {
                        tracing::info!("[BACKEND] applying constructed dns.");
                        std::net::SocketAddr::new(ip, profile.port)
                    }
                    Err(_) => {
                        self.report_error("DNS_INVALID", "Invalid host").await;
                        tracing::warn!("[DNS INVALID] Invalid host.");
                        return;
                    }
                };
                let cancel = tokio_util::sync::CancellationToken::new();
                self.active_cancel = Some(cancel.clone());

                let (tx_to_tun, rx_from_tun): (mpsc::Sender<Vec<u8>>, mpsc::Receiver<Vec<u8>>) =
                    tokio::sync::mpsc::channel(1024);

                let metrics = self.metrics.clone();
                let event_tx = self.event_tx.clone();
                let state_tx = self.state_tx.clone();
                let profile_clone = profile.clone();

                self.state.connections = ConnectionState::Connecting;
                self.state.active_profile = (Some(profile.tag.clone().unwrap_or_default()));
                self.state_tx.send_replace(self.state.clone());
                let _ = self.event_tx.send(BackendEvent::ConnectionStateChanged(
                    ConnectionState::Connecting,
                ));
                let _ = self.event_tx.send(BackendEvent::LogAdded {
                    level: LogLevel::Info,
                    message: format!(
                        "Connecting to {} profile. Host: {}",
                        profile.tag.unwrap_or_default(),
                        profile.host
                    ),
                    source: "backend".into(),
                });
                let crypto = self.crypto.clone();
                let tun = self.tun.clone();
                tracing::debug!("[BACKEND] Attempting to start connection");
                tokio::spawn(async move {
                    match establish_connection(
                        profile_clone,
                        server_addr,
                        metrics,
                        event_tx,
                        cancel,
                        tun,
                        crypto,
                    )
                    .await
                    {
                        Ok(_) => {
                            tracing::info!("Connection successfuly started.");
                            let _ = state_tx
                                .send_modify(|s| s.connections = ConnectionState::Connected);
                            let _ = state_tx.send_modify(|s| s.last_error = None);
                        }
                        Err(e) => {
                            let _ = state_tx.send_modify(|s| {
                                s.connections = ConnectionState::Failed;
                                s.last_error = Some(e.to_string());
                            });
                        }
                    }
                });
            }
            UiCommand::Disconnect => {
                if let Some(cancel) = self.active_cancel.take() {
                    cancel.cancel();
                }
                self.state.connections = ConnectionState::Disconnected;
                self.state_tx.send_replace(self.state.clone());
                let _ = self.event_tx.send(BackendEvent::ConnectionStateChanged(
                    ConnectionState::Disconnected,
                ));
                let _ = self.event_tx.send(BackendEvent::LogAdded {
                    level: LogLevel::Info,
                    message: format!(
                        "Disconnecting from {} profile.",
                        self.state.active_profile.clone().unwrap_or_default()
                    ),
                    source: "backend".into(),
                });
            }
            UiCommand::AddProfile(profile) => {
                let cloned_profile = profile.clone();
                self.state.active_profile = profile.clone().tag;
                self.state_tx.send_replace(self.state.clone());
                let _ = self
                    .event_tx
                    .send(BackendEvent::ProfileAdded(cloned_profile));
                let _ = self.event_tx.send(BackendEvent::LogAdded {
                    level: LogLevel::Info,
                    message: format!(
                        "Added profile. Tag: {}, Host: {}",
                        profile.tag.unwrap_or_default(),
                        profile.host
                    ),
                    source: "backend".into(),
                });
            }
            UiCommand::RemoveProfile(index) => {}
            UiCommand::RequestMetrics => {
                let snap = self.metrics.snapshot();
                let _ = self.event_tx.send(BackendEvent::MetricsUpdated {
                    rx_bytes: snap.bytes_rx,
                    tx_bytes: snap.bytes_tx,
                    peers: self.server_state.peer_count().await,
                });
            }
            UiCommand::RequestLogs => {}
        }
    }
    async fn report_error(&self, code: &str, msg: &str) {
        let _ = self.event_tx.send(BackendEvent::Error {
            code: code.into(),
            message: msg.into(),
        });
    }
}
async fn metrics_reporter(
    counters: std::sync::Arc<TrafficCounters>,
    state_tx: watch::Sender<BackendState>,
) {
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));
    loop {
        interval.tick().await;
        let snap = counters.snapshot();

        state_tx.send_if_modified(|state| {
            if state.rx_bytes == snap.bytes_rx && state.tx_bytes == snap.bytes_tx {
                return false;
            }
            state.rx_bytes = snap.bytes_rx;
            state.tx_bytes = snap.bytes_tx;
            state.rx_packets = snap.packets_rx;
            state.tx_packets = snap.packets_tx;
            true
        });
    }
}
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::Arc;
use vpn_daemon::transport::{frame::FrameKind, frame::decode_frame, frame::encode_frame};
pub struct ClientContext {
    pub profile: VpnProfile,
    pub socket: tokio::net::UdpSocket,
    pub session_id: u64,
    pub crypto: Arc<vpn_daemon::transport::frame::CryptoState>,
    pub server_addr: std::net::SocketAddr,
    pub tx_to_tun: tokio::sync::mpsc::Sender<Vec<u8>>,
    pub rx_from_tun: tokio::sync::mpsc::Receiver<Vec<u8>>,
    pub cancel: tokio_util::sync::CancellationToken,
}
async fn establish_connection(
    profile: VpnProfile,
    server_addr: SocketAddr,
    metrics: std::sync::Arc<TrafficCounters>,
    event_tx: broadcast::Sender<BackendEvent>,
    cancel: tokio_util::sync::CancellationToken,
    tun: Arc<tokio::sync::Mutex<TunInterface>>,
    crypto: Arc<vpn_daemon::transport::frame::CryptoState>,
) -> anyhow::Result<()> {
    let socket = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
    socket.connect(server_addr).await?;
    let session_id = generate_session_id();

    let (net_to_tun_tx, net_to_tun_rx) = mpsc::channel(1024);
    let (tun_to_net_tx, tun_to_net_rx) = mpsc::channel(1024);

    let ctx = ClientContext {
        profile: profile.clone(),
        socket: socket,
        session_id,
        crypto: crypto.clone(),
        server_addr,
        tx_to_tun: net_to_tun_tx,
        rx_from_tun: tun_to_net_rx,
        cancel: cancel.clone(),
    };
    client_handshake(&ctx).await?;
    let tunn = tun.clone();
    let guard = tunn.lock().await;
    let tun_name = guard.name();
    std::process::Command::new("ip")
        .args(["link", "set", "dev", tun_name, "up"])
        .status()?;
    std::process::Command::new("ip")
        .args(["addr", "add", "10.8.0.1/24", "dev", tun_name])
        .status()?;
    start_tun_bridge(tun, net_to_tun_rx, tun_to_net_tx);

    tracing::info!("Async TUN bridge started");
    tokio::spawn(client_data_loop(ctx));
    Ok(())
}
async fn client_handshake(ctx: &ClientContext) -> anyhow::Result<()> {
    let token = ctx.profile.uuid.as_bytes();
    let hello = encode_frame(FrameKind::HELLO, ctx.session_id, token);
    let encrypted = encrypt_frame(&hello, ctx.crypto.clone()).await?;

    ctx.socket.send(&encrypted).await?;

    let mut buf = vec![0u8; 2048];
    let len = tokio::time::timeout(std::time::Duration::from_secs(5), ctx.socket.recv(&mut buf))
        .await??;
    let plaintext = decrypt_frame(&buf[..len], ctx.crypto.clone()).await?;

    let frame = decode_frame(&plaintext).unwrap();
    if frame.kind != FrameKind::HELLOACK {
        anyhow::bail!("Expected HELLOACK, got {:?}", frame.kind);
    }
    if frame.session_id != ctx.session_id {
        anyhow::bail!("Session ID mismatch");
    }

    Ok(())
}
pub fn generate_session_id() -> u64 {
    getrandom::u64().unwrap()
}
use vpn_daemon::transport::server::{decrypt_frame, encrypt_frame};
async fn client_data_loop(ctx: ClientContext) {
    let mut buf = vec![0u8; 1500];
    let mut rx = ctx.rx_from_tun;
    let tx_to_tun = ctx.tx_to_tun;
    let socket = ctx.socket;
    let crypto = ctx.crypto;
    let cancel = ctx.cancel;

    loop {
        tokio::select! {
            res = socket.recv(&mut buf) => {
                match res {
                    Ok(len) => {
                        match decrypt_frame(&buf[..len], crypto.clone()).await {
                            Ok(frame_bytes) => {
                                if let Ok(frame) = decode_frame(&frame_bytes) {
                                    if frame.kind == FrameKind::DATA {
                                        let _ = tx_to_tun.send(frame.payload).await;
                                    }
                                }
                            }
                            Err(_) => continue,
                        }
                    }
                    Err(_) => break,
                }
            }
            Some(packet) = rx.recv() => {
                let frame = encode_frame(FrameKind::DATA, ctx.session_id, &packet);
                match encrypt_frame(&frame, crypto.clone()).await {
                    Ok(encrypted) => {
                        let _ = socket.send(&encrypted).await;
                    }
                    Err(_) => continue,
                }
            }
            _ = cancel.cancelled() => break,
        }
    }
}

use tokio::sync::Mutex; // ← Добавьте импорт

pub fn start_tun_bridge(
    tun: Arc<Mutex<TunInterface>>,
    mut rx_from_net: mpsc::Receiver<Vec<u8>>,
    tx_to_net: mpsc::Sender<Vec<u8>>,
) {
    let tun_reader = tun.clone();
    tokio::spawn(async move {
        let mut buf = vec![0u8; 1500];
        loop {
            let len = {
                let mut guard = tun_reader.lock().await;
                match guard.read_packet(&mut buf).await {
                    Ok(n) => n,
                    Err(e) => {
                        tracing::error!("Tun read error : {}", e);
                        break;
                    }
                }
            };

            if tx_to_net.send(buf[..len].to_vec()).await.is_err() {
                break;
            }
        }
    });

    let tun_writer = tun;
    tokio::spawn(async move {
        while let Some(mut packet) = rx_from_net.recv().await {
            {
                let mut guard = tun_writer.lock().await;
                guard.write_packet(&mut packet).await.ok();
            }
        }
    });
}
