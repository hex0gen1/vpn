use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::prelude::*;
use ratatui::{
    Frame, layout::Layout, widgets::Block, widgets::BorderType, widgets::Borders,
    widgets::Paragraph,
};
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;
use vpn_types::{Protocol, Security, VpnProfile};
mod app;
mod events;
mod screens;
mod ui;
use crate::app::{App, Mode, Popup};
use screens::{home, parser, pdetails, profiles};
use vpn_daemon::{
    daemon::runtime, linux::routing, linux::tun, transport::client, transport::frame,
    transport::packet, transport::server,
};
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

pub enum UiCommand {
    Connect(VpnProfile),
    Disconnect,
}

async fn run_engine(
    mut cmd_rx: mpsc::Receiver<UiCommand>,
    state_tx: watch::Sender<VpnState>,
    server_state: std::sync::Arc<server::ServerState>,
    tun: std::sync::Arc<std::sync::Mutex<tun::TunInterface>>,
) {
    let mut cancel = CancellationToken::new();
    loop {
        tokio::select! {
            Some(cmd) = cmd_rx.recv() => match cmd {
                UiCommand::Connect(profile) => {
                    state_tx.send_modify(|s| {
                        s.status = VpnStatus::Connecting;
                        s.push_log(format!("Connecting to {}:{}", profile.host, profile.port));
                    });
                    let child_cancel = CancellationToken::new();
                    let session_tx = state_tx.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        session_tx.send_modify(|s| {
                            s.status = VpnStatus::Connected { ip: "10.8.0.2".into(), peers: 1 };
                            s.push_log("Connected. TUN is UP.");
                        });
                        child_cancel.cancelled().await;
                    });
                    cancel = CancellationToken::new();
                }
                UiCommand::Disconnect => {
                    state_tx.send_modify(|s| {
                        s.status = VpnStatus::Disconnected;
                        s.push_log("Disconnecting...");
                    });
                    cancel.cancel();
                    cancel = CancellationToken::new();
                }
            },
            else => break,
        }
    }
}

fn handle_connect_action(
    state_rx: &watch::Receiver<VpnState>,
    cmd_tx: &mpsc::Sender<UiCommand>,
    profile: VpnProfile,
) {
    let status = state_rx.borrow().status.clone();
    match status {
        VpnStatus::Disconnected | VpnStatus::Connecting => {
            let _ = cmd_tx.try_send(UiCommand::Connect(profile));
        }
        VpnStatus::Connected { .. } => {
            let _ = cmd_tx.try_send(UiCommand::Disconnect);
        }
        VpnStatus::Error(_) => {}
    }
}

fn handle_normal_event(
    app: &mut app::App,
    event: Event,
    state_rx: &watch::Receiver<VpnState>,
    cmd_tx: &mpsc::Sender<UiCommand>,
) {
    if let Event::Key(key) = event {
        if key.kind != KeyEventKind::Press {
            return;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                app.mode = Mode::Popup(Popup::ConfirmQuit);
                app.popup = Popup::ConfirmQuit;
            }
            KeyCode::Char('h') => app.screen = app::Screen::Home,
            KeyCode::Char('p') => app.screen = app::Screen::Profiles,
            KeyCode::Char('l') => app.screen = app::Screen::Logs,
            KeyCode::Char('y') => {
                app.screen = app::Screen::Parser;
                app.mode = Mode::Input;
            }
            KeyCode::Char('j') => app.next_profile(),
            KeyCode::Char('k') => app.prev_profile(),
            KeyCode::Char('c') => {
                if app.screen == app::Screen::Profiles {
                    if let Some(profile) = app.profiles.get(app.selected_profile).cloned() {
                        handle_connect_action(state_rx, cmd_tx, profile);
                    }
                }
            }
            KeyCode::Enter => {
                if app.screen == app::Screen::Profiles {
                    app.screen = app::Screen::ProfilesDetail
                }
            }
            _ => {}
        }
    }
}

fn handle_input_event(app: &mut app::App, event: Event) {
    match event {
        Event::Paste(text) => {
            app.input_str.push_str(&text);
        }
        Event::Key(key) => {
            if key.kind != KeyEventKind::Press {
                return;
            }
            match key.code {
                KeyCode::Esc => app.mode = Mode::Normal,
                KeyCode::Enter => {
                    let input = std::mem::take(&mut app.input_str);
                    app.mode = Mode::Normal;
                    match vpn_daemon::parser::parse_vless::parse_vless_link(&input) {
                        Ok(profile) => {
                            app.profiles.push(profile.clone());
                            app.popup = Popup::PreviewAdd(profile.clone());
                            app.mode = Mode::Popup(Popup::PreviewAdd(profile.clone()));
                            app.selected_profile += 1;
                        }
                        Err(err) => {
                            eprintln!("parse error: {err}");
                        }
                    }
                }
                KeyCode::Delete => {
                    app.delete_selected_profile();
                }
                KeyCode::Char(c) => {
                    app.input_str.push(c);
                    app.cursor_pos += 1;
                }
                KeyCode::Left => {
                    if app.cursor_pos > 0 {
                        app.cursor_pos -= 1;
                    }
                }
                KeyCode::Right => {
                    if app.cursor_pos < app.input_str.len() {
                        app.cursor_pos += 1;
                    }
                }
                KeyCode::Backspace => {
                    if app.cursor_pos > 0 {
                        app.cursor_pos -= 1;
                        app.input_str.remove(app.cursor_pos);
                    }
                }
                KeyCode::Home => {
                    app.cursor_pos = 0;
                }
                KeyCode::End => {
                    app.cursor_pos = app.input_str.len();
                }
                _ => {}
            }
        }
        _ => {}
    }
}

fn handle_details_event(app: &mut app::App, event: Event) {
    match event {
        Event::Key(key) => {
            if key.kind != KeyEventKind::Press {
                return;
            }
            match key.code {
                KeyCode::Enter | KeyCode::Char('l') => {
                    app.screen = app::Screen::ProfilesDetail;
                }
                _ => {}
            }
        }
        _ => {}
    }
}

fn handle_popup_event(
    app: &mut app::App,
    event: Event,
    state_rx: &watch::Receiver<VpnState>,
    cmd_tx: &mpsc::Sender<UiCommand>,
) {
    match &app.popup {
        Popup::ConfirmDelete => match event {
            Event::Key(key) => {
                if key.kind != KeyEventKind::Press {
                    return;
                }
                match key.code {
                    KeyCode::Enter => {
                        app.mode = Mode::Normal;
                        app.delete_selected_profile();
                        app.popup = Popup::None;
                    }
                    KeyCode::Esc | KeyCode::Char('q') => {
                        app.mode = Mode::Normal;
                        app.popup = Popup::None;
                    }
                    _ => {}
                }
            }
            _ => {}
        },
        Popup::None => {}
        Popup::ParserResult => match event {
            Event::Key(key) => {
                if key.kind != KeyEventKind::Press {
                    return;
                }
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => {
                        app.mode = Mode::Normal;
                        app.popup = Popup::None;
                    }
                    _ => {}
                }
            }
            _ => {}
        },
        Popup::Connect => match event {
            Event::Key(key) => {
                if key.kind != KeyEventKind::Press {
                    return;
                }
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => {
                        app.mode = Mode::Normal;
                        app.popup = Popup::None;
                    }
                    KeyCode::Enter => {
                        if let Some(profile) = app.profiles.get(app.selected_profile).cloned() {
                            handle_connect_action(state_rx, cmd_tx, profile);
                        }
                        app.mode = Mode::Normal;
                        app.popup = Popup::None;
                    }
                    _ => {}
                }
            }
            _ => {}
        },
        Popup::ConfirmQuit => match event {
            Event::Key(key) => {
                if key.kind != KeyEventKind::Press {
                    return;
                }
                match key.code {
                    KeyCode::Char('y') => {
                        app.should_quit = true;
                        app.mode = Mode::Normal;
                        app.popup = Popup::None;
                    }
                    KeyCode::Char('n') => {
                        app.mode = Mode::Normal;
                        app.popup = Popup::None;
                    }
                    _ => {}
                }
            }
            _ => {}
        },
        Popup::Error(_) => match event {
            Event::Key(key) => {
                if key.kind != KeyEventKind::Press {
                    return;
                }
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => {
                        app.mode = Mode::Normal;
                        app.popup = Popup::None;
                    }
                    _ => {}
                }
            }
            _ => {}
        },
        Popup::PreviewAdd(profile) => match event {
            Event::Key(key) => {
                if key.kind != KeyEventKind::Press {
                    return;
                }
                match key.code {
                    KeyCode::Enter => {
                        app.mode = Mode::Normal;
                        app.popup = Popup::None;
                    }
                    KeyCode::Esc => {
                        app.mode = Mode::Normal;
                        app.popup = Popup::None;
                        app.profiles.remove(app.selected_profile);
                    }
                    _ => {}
                }
            }
            _ => {}
        },
    }
}

async fn run(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut app::App,
    state_rx: watch::Receiver<VpnState>,
    cmd_tx: mpsc::Sender<UiCommand>,
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
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt::init();

    let mut terminal = ratatui::init();
    terminal.clear()?;

    let mut app = app::App::new();
    let (state_tx, state_rx) = watch::channel(VpnState::new());
    let (cmd_tx, cmd_rx) = mpsc::channel(32);

    let (fd, name) = tun::create_interface("tun1")?;
    let tun = std::sync::Arc::new(std::sync::Mutex::new(tun::TunInterface::new(fd, name)?));
    let subnet = std::net::Ipv4Addr::new(10, 8, 0, 8);
    let server_state = std::sync::Arc::new(server::ServerState::new(subnet, 254));

    tokio::spawn(run_engine(cmd_rx, state_tx, server_state, tun));

    let result = run(&mut terminal, &mut app, state_rx, cmd_tx).await;

    ratatui::restore();
    result
}
