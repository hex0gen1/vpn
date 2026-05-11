use color_eyre::Result;
use crossterm::event::{Event, KeyCode, KeyEventKind};
use ratatui::prelude::*;
use tokio::sync::mpsc;
mod app;
mod backend;
mod events;
mod screens;
mod ui;
use crate::app::{App, Mode, Popup};
use vpn_daemon::transport::frame::{FrameKind, decode_frame, encode_frame};
use vpn_daemon::transport::server::encrypt_frame;
use vpn_daemon::{linux::tun, transport::server};
#[derive(Debug, Clone, PartialEq)]
pub enum VpnStatus {
    Disconnected,
    Connecting,
    Connected { ip: String, peers: usize },
    Error(String),
}

#[derive(Debug, Clone)]
pub struct VpnState {
    pub status: VpnStatus,
    pub logs: Vec<String>,
}
impl Default for VpnState {
    fn default() -> Self {
        Self::new()
    }
}
impl VpnState {
    pub fn new() -> Self {
        Self {
            status: VpnStatus::Disconnected,
            logs: Vec::new(),
        }
    }
    pub fn push_log(&mut self, msg: impl Into<String>) {
        self.logs.push(msg.into());
        if self.logs.len() > 60 {
            self.logs.drain(..10);
        }
    }
}
use app::Screen;
use vpn_daemon::parser::parse_vless::parse_vless_link;
async fn handle_key_event(app: &mut App, key: crossterm::event::KeyEvent, handle: &BackendHandle) {
    if key.kind != KeyEventKind::Press {
        return;
    }
    match (&app.mode, app.screen, key.code) {
        (Mode::Normal, _, KeyCode::Char('h')) => app.screen = Screen::Home,
        (Mode::Normal, _, KeyCode::Char('p')) => app.screen = Screen::Profiles,
        (Mode::Normal, _, KeyCode::Char('l')) => app.screen = Screen::Logs,
        (Mode::Normal, _, KeyCode::Char('i')) => app.mode = Mode::Input,
        (Mode::Normal, _, KeyCode::Char('y')) => {
            app.screen = Screen::Parser;
            app.mode = Mode::Input;
        }
        (Mode::Normal, _, KeyCode::Char('q')) => {
            app.popup = Popup::ConfirmQuit;
            app.mode = Mode::Popup(Popup::ConfirmQuit);
        }
        (Mode::Input, _, KeyCode::Esc) => {
            app.mode = Mode::Normal;
            app.input_str.clear();
        }
        (Mode::Input, _, KeyCode::Backspace) => {
            app.input_str.pop();
        }
        (Mode::Input, _, KeyCode::Enter) => {
            let input = std::mem::take(&mut app.input_str);
            match parse_vless_link(&input) {
                Ok(profile) => {
                    let _ = handle
                        .cmd_tx
                        .try_send(UiCommand::AddProfile(profile.clone()));
                    app.profiles.push(profile.clone());
                    let _ = vpn_daemon::parser::parse_vless::save_profile(
                        &profile,
                        &profile.clone().tag.unwrap(),
                    )
                    .map_err(|e| {
                        format!(
                            "Failed to load profile: {:?} with error {} ",
                            profile.tag, e
                        )
                    });
                    app.mode = Mode::Normal;
                    tracing::info!("Added profile : {:?}", Some(profile.tag))
                }
                Err(e) => {
                    app.popup = Popup::Error(e.to_string());
                    app.mode = Mode::Popup(Popup::Error(String::new()));
                }
            }
        }
        (Mode::Normal, Screen::Profiles, KeyCode::Char('j')) => app.prev_profile(),
        (Mode::Normal, Screen::Profiles, KeyCode::Char('k')) => app.next_profile(),
        (Mode::Input, _, KeyCode::Char(c)) => app.input_str.push(c),
        (Mode::Normal, Screen::Profiles | Screen::ProfilesDetail, KeyCode::Tab) => {
            if let Some(profile) = app.current_profile().cloned() {
                match handle.try_send_cmd(UiCommand::Connect(profile)) {
                    Ok(_) => {
                        app.logs.push(("Connect cmd sent to backend.").into());
                        tracing::debug!("Connect cmd sent to backend.");
                    }
                    Err(_) => app.logs.push(("Backend channel closed/full.").into()),
                }
            } else {
                tracing::warn!("Connect attempted but no profile selected.");
            }
        }
        (Mode::Normal, Screen::Profiles, KeyCode::Delete) => {
            let idx = app.selected_profile;
            let _ = handle.cmd_tx.try_send(UiCommand::RemoveProfile(idx));
        }
        (Mode::Normal, Screen::Profiles, KeyCode::Enter) => {
            app.screen = Screen::ProfilesDetail;
            app.mode = Mode::Normal;
        }
        (Mode::Popup(Popup::ConfirmQuit), _, KeyCode::Char('y')) => app.should_quit = true,
        (Mode::Popup(_), _, KeyCode::Esc)
        | (Mode::Popup(Popup::ConfirmQuit), _, KeyCode::Char('n')) => {
            app.popup = Popup::None;
            app.mode = Mode::Normal;
        }
        _ => {}
    }
}
/*async fn run(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut app::App,
    handle: BackendHandle,
) -> Result<()> {
    loop {
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(3)])
                .split(f.area());

            let state = state_rx.borrow().clone();
            ui::render(f, app);
        })?;

        if event::poll(std::time::Duration::from_millis(100))? {
            let event = event::read()?;
            match app.mode {
                Mode::Normal => handle_normal_event(app, event, &state_rx, &cmd_tx),
                Mode::Input => handle_input_event(app, event),
                Mode::Details => handle_details_event(app, event),
                Mode::Popup(_) => handle_popup_event(app, event, &state_rx, &cmd_tx),
            }
        }

        if app.should_quit {
            return Ok(());
        }
    }
}*/
use futures::StreamExt;
pub async fn run(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
    mut handle: BackendHandle,
) -> Result<()> {
    let mut events = crossterm::event::EventStream::new();
    let mut state_change = handle.state_rx.clone();
    let mut event_rx = handle.event_rx.resubscribe();

    loop {
        tokio::select! {
                Some(Ok(event)) = events.next() => {
                    if let Event::Key(key) = event && key.kind == KeyEventKind::Press {
                            handle_key_event(app, key, &handle).await;
                        }
                }
            _ = state_change.changed() => {
                let state = state_change.borrow().clone();
                app.apply_backend_state(state);
            }
            Ok(backend_event) = event_rx.recv() => {
                app.handle_backend_event(backend_event);
            }
            _ = handle.metrics_rx.changed() => {
                let metrics = handle.metrics_rx.borrow().clone();
                app.update_metrics(metrics);
            }
        }
        terminal.draw(|f| {
            let state = handle.state_rx.borrow();
            ui::render(f, app, &state);
        })?;

        if app.should_quit {
            break;
        }
    }

    Ok(())
}
use backend::backend::{Backend, BackendConfig, BackendHandle, UiCommand};
#[tokio::main]
async fn main() -> Result<()> {
    initialize_loggingg()?;
    color_eyre::install()?;
    let mut terminal = ratatui::init();
    terminal.clear()?;
    let mut app = app::App::new();
    let (fd, name) = tun::create_interface("xtvpn0")?;
    let subnet = std::net::Ipv4Addr::new(10, 8, 0, 0);
    std::process::Command::new("ip")
        .args(["link", "set", "dev", &name, "up"])
        .status()?;
    std::process::Command::new("ip")
        .args(["addr", "add", "10.8.0.1/24", "dev", &name])
        .status()?;

    let cfg = BackendConfig {
        subnet,
        pool_size: 254,
        tun_name: name.clone(),
    };
    let tun = std::sync::Arc::new(tokio::sync::Mutex::new(
        vpn_daemon::linux::tun::TunInterface::new(fd, name)?,
    ));
    let crypto = vpn_daemon::transport::server::generate_crypto_state()?;
    let cryptoo = crypto.clone();
    //tokio::spawn(async move {
    //    if let Err(e) = run_local_server("127.0.0.1:11949", tun_tun, crypto).await {
    //        tracing::error!("Test server crashed: {}", e);
    //    }
    //});
    tracing::info!("Local test server spawned on 127.0.0.1:11949");
    let _server_state = std::sync::Arc::new(server::ServerState::new(subnet, 254));
    let (backend, handle) = Backend::new(cfg, tun, cryptoo);
    //tokio::spawn(run_engine(cmd_rx, state_tx, server_state, tun));
    tokio::spawn(async move {
        backend.run().await;
    });
    let result = run(&mut terminal, &mut app, handle).await;

    ratatui::restore();
    result
}
pub async fn run_local_server(
    listen_addr: &str,
    tun: std::sync::Arc<tokio::sync::Mutex<vpn_daemon::linux::tun::TunInterface>>,
    crypto: std::sync::Arc<vpn_daemon::transport::frame::CryptoState>,
) -> anyhow::Result<()> {
    let socket = tokio::net::UdpSocket::bind(listen_addr).await?;
    tracing::info!("Server listening on {}", listen_addr);

    let (udp_to_tun_tx, udp_to_tun_rx) = mpsc::channel::<Vec<u8>>(1024);
    let (_tun_to_udp_tx, _tun_to_udp_rx) = mpsc::channel::<Vec<u8>>(1024);
    let mut from_udp = udp_to_tun_rx;
    let _tun_r = tun.clone();
    let _tun_rr = tun.clone();
    let _tun_w = tun.clone();
    tokio::spawn(async move {
        while let Some(mut packet) = from_udp.recv().await {
            let _ = tun.lock().await.write_packet(&mut packet).await;
        }
    });
    /*tokio::spawn(async move {
        let mut buf = [0u8; 1500];
        loop {
            let len = {
                let mut guard = tun_rr.lock().await;
                match guard.read_packet(&mut buf).await {
                    Ok(n) => n,
                    Err(_) => break,
                }
            };
            if tun_to_udp_tx.send(buf[..len].to_vec()).await.is_err() {
                break;
            }
        }
    });*/
    let mut buf = vec![0u8; 2048];
    let mut last_client_addr: Option<std::net::SocketAddr> = None;
    loop {
        let (len, client_addr) = match socket.recv_from(&mut buf).await {
            Ok(tup) => tup,
            Err(e) => {
                tracing::warn!("Udp recv_from error {}", e);
                continue;
            }
        };
        if last_client_addr.is_none() {
            last_client_addr = Some(client_addr);
            tracing::debug!("First client added. {}", client_addr);
        }
        let raw = buf[..len].to_vec();
        let plain_text =
            match vpn_daemon::transport::server::decrypt_frame(&raw, crypto.clone()).await {
                Ok(n) => n,
                Err(e) => {
                    tracing::debug!("Decrypt failed: {}", e);
                    continue;
                }
            };
        if let Ok(frame) = decode_frame(&plain_text) {
            match frame.kind {
                FrameKind::HELLO => {
                    let ack = encode_frame(FrameKind::HELLOACK, frame.session_id, &[]);
                    let encrypted = match encrypt_frame(&ack, crypto.clone()).await {
                        Ok(n) => n,
                        Err(e) => {
                            tracing::debug!("Encrypt failed: {}", e);
                            continue;
                        }
                    };
                    let _ = socket.send_to(&encrypted, client_addr).await;
                    tracing::debug!("Handshake OK for session {}", frame.session_id);
                }
                FrameKind::DATA => {
                    if udp_to_tun_tx.send(frame.payload.clone()).await.is_err() {
                        break;
                    }
                    if let Some(reply) = emulate_icmp_reply(&frame.payload.clone()) {
                        let resp = encode_frame(FrameKind::DATA, frame.session_id, &reply);
                        let encrypted = match encrypt_frame(&resp, crypto.clone()).await {
                            Ok(n) => n,
                            Err(e) => {
                                tracing::debug!("Encrypt failed {}", e);
                                continue;
                            }
                        };
                        let _ = socket.send_to(&encrypted, client_addr).await;
                    }
                    //if let Ok(len) = tun_r.lock().await.read_packet(&mut buf).await {
                    //    let resp = encode_frame(FrameKind::DATA, frame.session_id, &buf[..len]);
                    //    let _ = socket.send_to(&resp, client_addr).await;
                    //}
                }
                _ => {}
            }
        }
    }
    Ok(())
}
fn emulate_icmp_reply(payload: &[u8]) -> Option<Vec<u8>> {
    if payload.len() < 28 {
        tracing::debug!("ICMP: packet too short ({})", payload.len());
        return None;
    }

    if (payload[0] >> 4) != 4 {
        tracing::debug!("ICMP: not IPv4 (version={})", payload[0] >> 4);
        return None;
    }
    if payload[9] != 1 {
        tracing::debug!("ICMP: not ICMP protocol (proto={})", payload[9]);
        return None;
    }

    let ip_header_len = ((payload[0] & 0x0F) * 4) as usize;
    if payload.len() < ip_header_len + 8 {
        tracing::debug!("ICMP: packet too short for ICMP header");
        return None;
    }

    if payload[ip_header_len] != 8 {
        tracing::debug!("ICMP: not Echo Request (type={})", payload[ip_header_len]);
        return None;
    }

    let mut reply = payload.to_vec();

    reply[ip_header_len] = 0;

    reply[ip_header_len + 2] = 0;
    reply[ip_header_len + 3] = 0;
    let icmp_sum = calculate_checksum(&reply[ip_header_len..]);
    reply[ip_header_len + 2] = (icmp_sum >> 8) as u8;
    reply[ip_header_len + 3] = icmp_sum as u8;

    reply[12..16].copy_from_slice(&payload[16..20]);
    reply[16..20].copy_from_slice(&payload[12..16]);

    reply[10] = 0;
    reply[11] = 0;
    let ip_sum = calculate_checksum(&reply[0..ip_header_len]);
    reply[10] = (ip_sum >> 8) as u8;
    reply[11] = ip_sum as u8;

    tracing::debug!("ICMP reply generated, len={}", reply.len());
    Some(reply)
}

use color_eyre::eyre::Result as CResult;
pub fn initialize_loggingg() -> CResult<()> {
    let log_path = std::env::current_dir()?.join("vpn-debug.log");

    eprintln!("Log file path: {}", log_path.display());

    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;

    let subscriber = tracing_subscriber::fmt()
        .with_writer(log_file)
        .with_ansi(false)
        .with_target(false)
        .with_file(true)
        .with_line_number(true)
        .with_env_filter(tracing_subscriber::EnvFilter::builder().parse_lossy("debug"))
        .finish();

    tracing::subscriber::set_global_default(subscriber).ok();

    tracing::info!("Logging initialized. File: {}", log_path.display());
    tracing::debug!("DEBUG test message");

    Ok(())
}

fn calculate_checksum(buf: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    for chunk in buf.chunks(2) {
        let val = if chunk.len() == 2 {
            u16::from_be_bytes([chunk[0], chunk[1]]) as u32
        } else {
            (chunk[0] as u32) << 8
        };
        sum += val;
    }
    while sum > 0xffff {
        sum = (sum >> 16) + (sum & 0xffff);
    }
    !(sum as u16)
}
