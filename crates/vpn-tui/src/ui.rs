use crate::app::{App, Popup, Screen};
use crate::screens::{home, logs, parser, pdetails, profiles};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::Span,
    widgets::{Block, Borders, Clear, Paragraph},
};
use vpn_types::Protocol;
#[derive(Debug)]
pub enum SK {
    Info,
    Warning,
    Error,
    Success,
}
enum Hotkeys {
    MvParser,
    MvHome,
    ExpandDetails,
    MvProfiles,
    MvLogs,
    Quit,
    ImportLink,
}
impl Hotkeys {
    fn as_str(hotkey: Hotkeys) -> String {
        match hotkey {
            Hotkeys::MvParser => "y: move to parser".to_string(),
            Hotkeys::MvProfiles => "p: move to profiles".to_string(),
            Hotkeys::MvHome => "h: move to homepage".to_string(),
            Hotkeys::ExpandDetails => "enter: expand details".to_string(),

            Hotkeys::MvLogs => "l: move to logs".to_string(),
            Hotkeys::Quit => "q: quit application".to_string(),
            Hotkeys::ImportLink => "ctrl+v: paste link from buffer".to_string(),
        }
    }
}
#[derive(Debug)]
pub struct SB {
    pub message: String,
    pub sk: SK,
}
pub fn render(frame: &mut Frame, app: &App) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(frame.area());

    render_header(frame, app, layout[0]);

    match app.screen {
        Screen::Home => home::render(frame, app, layout[1]),
        Screen::Profiles => profiles::render(frame, app, layout[1]),
        Screen::Logs => logs::render(frame, app, layout[1]),
        Screen::Parser => parser::render(frame, app, layout[1]),
        Screen::ProfilesDetail => pdetails::render(frame, app, layout[1]),
    }

    //render_footer(frame, app, layout[2]);
    render_status_bar(frame, app, layout[2]);
    if app.popup != Popup::None {
        match &app.popup {
            Popup::ConfirmQuit => render_exit_popup(frame, app, layout[1]),
            Popup::ConfirmDelete => render_confirmdel_popup(frame, app, layout[1]),
            Popup::None => (),
            Popup::ParserResult => render_parseres_popup(frame, app, layout[1]),
            Popup::Connect => render_connection_popup(frame, app, layout[1]),
            Popup::Error(text) => render_error_popup(frame, app, layout[1], text.as_str()),
        }
    }
}

fn render_header(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let title = match app.screen {
        Screen::Home => "Xanost VPN - Home",
        Screen::Profiles => "Xanost VPN - Profiles",
        Screen::Logs => "Xanost VPN - Logs",
        Screen::Parser => "Xanost VPN - Parser",
        Screen::ProfilesDetail => "Xanost VPN - Profile Details",
    };

    let header =
        Paragraph::new(title).block(Block::default().borders(Borders::ALL).title("Header"));

    frame.render_widget(header, area);
}

fn render_footer(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let conn = match app.connection_state {
        crate::app::ConnectionState::Disconnected => "Disconnected",
        crate::app::ConnectionState::Connecting => "Connecting",
        crate::app::ConnectionState::Connected => "Connected",
        crate::app::ConnectionState::Failed => "Failed",
    };

    let footer_text = format!("q: quit | h: home | p: profiles | l: logs | status: {conn}");

    let footer =
        Paragraph::new(footer_text).block(Block::default().borders(Borders::ALL).title("Status"));
    frame.render_widget(footer, area);
}

fn render_status_bar(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let input_type = match &app.mode {
        crate::app::Mode::Input => "Input mode".to_string(),
        crate::app::Mode::Normal => "Normal mode".to_string(),
        crate::app::Mode::Popup(popup) => format!("Popup mode{}", popup.as_str()),
        crate::app::Mode::Details => "Details mode".to_string(),
    };
    let status_msg = app.status.message.as_str();
    let status_kind = match app.status.sk {
        SK::Error => String::from("!ERROR!"),
        SK::Info => String::from("INFO"),
        SK::Success => String::from("_SUCCESS_"),
        SK::Warning => String::from("WARNING!"),
    };
    let final_output =
        format!("type: {input_type} | status: {status_kind} | message: {status_msg}");
    let status_bar = Paragraph::new(final_output)
        .block(Block::default().borders(Borders::ALL).title("StatusBar"));

    frame.render_widget(status_bar, area);
}
fn render_exit_popup(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let mut popup_area = Rect::from(area);
    popup_area.height /= 2;
    popup_area.width /= 2;
    popup_area.x += area.width / 4;
    popup_area.y += area.height / 4;
    let popup_text = String::from("You sure you wanna exit?\n[y][n]");
    frame.render_widget(Clear, popup_area);
    frame.render_widget(
        Paragraph::new(popup_text).block(Block::default().title("Confirm Exit")),
        popup_area,
    );
}
fn render_confirmdel_popup(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let mut popup_area = Rect::from(area);
    popup_area.width /= 2;
    popup_area.height /= 2;
    popup_area.x += area.width / 4;
    popup_area.y += area.height / 4;
    let popup_text = String::from(
        "Confirm delete? Profile data will be deleted from memory.\n Enter - yes, Esc - no",
    );
    frame.render_widget(Clear, popup_area);
    frame.render_widget(
        Paragraph::new(popup_text).block(Block::default().title("Confirm Delete")),
        popup_area,
    );
}
fn render_parseres_popup(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let mut popup_area = Rect::from(area);
    popup_area.width /= 2;
    popup_area.height /= 2;
    popup_area.x += area.width / 4;
    popup_area.y += area.height / 4;

    let popup_text = match app.profiles.get(app.selected_profile) {
        Some(profile) => match &profile.protocol {
            Protocol::Vless => {
                let protocol = profile.protocol.as_str();
                format!("Successfully loaded new profile! Protocol: {protocol}")
            }
            Protocol::Unknown(name) => {
                format!("Failed to parse config: unsupported protocol `{name}`.")
            }
            Protocol::None => "Failed to parse config! Protocol is missing.".to_string(),
        },
        None => "Failed to parse config! No profile selected.".to_string(),
    };

    let block = Block::default().title(Span::styled(
        "Parse Result",
        Style::default().add_modifier(Modifier::BOLD),
    ));

    frame.render_widget(Clear, popup_area);
    frame.render_widget(Paragraph::new(popup_text).block(block), popup_area);
}

fn render_popup_overlay(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {}
fn render_error_popup(frame: &mut Frame, app: &App, area: Rect, message: &str) {
    let mut popup_area = Rect::from(area);
    popup_area.width /= 2;
    popup_area.height /= 2;
    popup_area.x += area.width / 4;
    popup_area.y += area.height / 4;
    let mut par = String::from("Error: ");
    par.push_str(message);
    let widget = Paragraph::new(par).block(Block::default().borders(Borders::ALL).title("Error!"));
    frame.render_widget(Clear, popup_area);
    frame.render_widget(widget, popup_area);
}
fn render_connection_popup(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let mut popup_area = Rect::from(area);
    popup_area.width /= 2;
    popup_area.height /= 2;
    popup_area.x += area.width / 4;
    popup_area.y += area.height / 4;

    let popup_text = match app.profiles.get(app.selected_profile) {
        None => {
            "No profile selected.".to_string()
        }
        Some(profile) => {
            match &profile.protocol {
                Protocol::None => {
                    "You need to select a profile before connecting. You can do it by hovering over an available profile."
                        .to_string()
                }
                Protocol::Unknown(text) => {
                    format!("Cannot connect: unsupported protocol `{text}`.")
                }
                Protocol::Vless => {
                    let name = profile.tag.as_deref().unwrap_or("no tag");
                    let protocol = profile.protocol.as_str();

                    format!(
                        "Applying connection with profile {name}, protocol: {protocol}. To view more details, press Enter while hovering over the profile."
                    )
                }
            }
        }
    };

    frame.render_widget(Clear, popup_area);
    frame.render_widget(
        Paragraph::new(popup_text)
            .block(Block::default().borders(Borders::ALL).title("Connection")),
        popup_area,
    );
}
