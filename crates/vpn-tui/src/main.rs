use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::DefaultTerminal;
mod app;
mod backend;
mod events;
mod screens;
mod ui;

use crate::backend::orchestrator::{CommandEnvelope, Orchestrator};
use crate::backend::state::Event as BackendEvent;
use crate::backend::state::{Command, DaemonInit, IpcResponse, State};
use anyhow::{Result, anyhow};
use app::{App, ConnectionState};
use tokio::sync::{broadcast, mpsc, oneshot, watch};
pub struct BackendHandle {
    pub cmd_tx: mpsc::Sender<CommandEnvelope>,
    pub state_rx: watch::Receiver<State>,
    pub event_rx: broadcast::Receiver<BackendEvent>,
}
pub async fn send_command(
    cmd_tx: &mpsc::Sender<CommandEnvelope>,
    command: Command,
) -> Result<IpcResponse> {
    let (reply_tx, reply_rx) = oneshot::channel::<IpcResponse>();

    cmd_tx
        .send(CommandEnvelope {
            command,
            reply: Some(reply_tx),
        })
        .await
        .map_err(|_| anyhow!("failed to send command to orchestrator"))?;

    let response = reply_rx
        .await
        .map_err(|_| anyhow!("orchestrator dropped reply"))?;

    Ok(response)
}

pub async fn send_connection_request(
    cmd_tx: &mpsc::Sender<CommandEnvelope>,
) -> Result<IpcResponse> {
    send_command(cmd_tx, Command::Connect).await
}

pub async fn send_disconnect_request(
    cmd_tx: &mpsc::Sender<CommandEnvelope>,
) -> Result<IpcResponse> {
    send_command(cmd_tx, Command::Disconnect).await
}
use app::{Mode, Popup};
use vpn_parse::parser::parse_vless_link;

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt::init();

    let mut app = app::App::new();
    let mut terminal = ratatui::init();

    let init = DaemonInit::new(&app);

    let (cmd_tx, cmd_rx) = mpsc::channel::<CommandEnvelope>(64);
    let (state_tx, state_rx) = watch::channel(State::new(&init));
    let (event_tx, _) = broadcast::channel::<BackendEvent>(256);

    let orchestrator = Orchestrator::new(event_tx.clone(), state_tx, cmd_rx, init);

    tokio::spawn(async move {
        orchestrator.run().await;
    });

    let backend = BackendHandle {
        cmd_tx,
        state_rx,
        event_rx: event_tx.subscribe(),
    };

    let result = run(&mut terminal, &mut app, &backend).await;

    ratatui::restore();
    result?;
    Ok(())
}

async fn run(
    terminal: &mut DefaultTerminal,
    app: &mut app::App,
    backend: &BackendHandle,
) -> std::io::Result<()> {
    loop {
        terminal.draw(|frame| ui::render(frame, app))?;
        if app.should_quit {
            break;
        }
        let event = event::read()?;

        match app.mode {
            Mode::Normal => handle_normal_event(app, event),
            Mode::Input => handle_input_event(app, event),
            Mode::Popup(_) => handle_popup_event(app, event, backend),
        }
    }
    Ok(())
}

fn handle_normal_event(app: &mut app::App, event: Event) {
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
                    app.popup = Popup::Connect;
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
                KeyCode::Backspace => {
                    app.input_str.pop();
                }
                KeyCode::Enter => {
                    let input = std::mem::take(&mut app.input_str);
                    app.mode = Mode::Normal;

                    match parse_vless_link(&input) {
                        Ok(profile) => app.profiles.push(profile),
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
                }
                _ => {}
            }
        }
        _ => {}
    }
}

async fn handle_connect(app: &App, backend: &BackendHandle) -> Result<IpcResponse> {
    if app.profiles.is_empty() {
        return Err(anyhow!("no profiles available"));
    }

    let profile = &app.profiles[app.selected_profile];

    if profile.protocol.as_str().is_empty() {
        return Err(anyhow!("selected profile has empty protocol"));
    }

    send_connection_request(&backend.cmd_tx).await
}

async fn handle_disconnect(_app: &App, backend: &BackendHandle) -> Result<IpcResponse> {
    send_disconnect_request(&backend.cmd_tx).await
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
fn handle_popup_event(app: &mut app::App, event: Event, back: &BackendHandle) {
    match &app.popup {
        Popup::ConfirmDelete => match event {
            Event::Key(key) => {
                if key.kind != KeyEventKind::Press {
                    return;
                }
                match key.code {
                    KeyCode::Enter => {
                        app.mode = Mode::Normal;
                        app.delete_profile();
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
                        handle_connect_popup_enter(app, back);
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
        Popup::Error(String) => match event {
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
    }
}
async fn handle_connect_popup_enter(app: &mut app::App, backend: &BackendHandle) {
    use crate::backend::state::VpnStatus;
    let backend_state = backend.state_rx.borrow().clone();
    match backend_state.status {
        VpnStatus::Disconnected | VpnStatus::Idle => {
            match send_connection_request(&backend.cmd_tx).await {
                Ok(IpcResponse::Ok) => {
                    app.popup = Popup::None;
                    app.mode = Mode::Normal;
                }
                Ok(IpcResponse::Error(err)) => {
                    app.popup = Popup::Error(err);
                }
                Ok(IpcResponse::State(_)) => {}
                Err(err) => {
                    app.popup = Popup::Error(err.to_string());
                }
                Ok(IpcResponse::Event(event)) => {}
            }
        }

        VpnStatus::Connected => match send_disconnect_request(&backend.cmd_tx).await {
            Ok(IpcResponse::Ok) => {
                app.popup = Popup::None;
                app.mode = Mode::Normal;
            }
            Ok(IpcResponse::Error(err)) => {
                app.popup = Popup::Error(err);
            }
            Ok(IpcResponse::State(_)) => {}
            Err(err) => {
                app.popup = Popup::Error(err.to_string());
            }
            Ok(IpcResponse::Event(Event)) => {}
        },

        VpnStatus::Connecting | VpnStatus::Disconnecting => {}

        VpnStatus::Error(error) => {
            app.popup = Popup::None;
            app.mode = Mode::Normal;
        }
    }
}
