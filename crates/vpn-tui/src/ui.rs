use crate::app::{App, Popup, Screen};
use crate::screens::constants;
use crate::screens::{home, logs, parser, pdetails, profiles};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph},
};
use vpn_types::Protocol;
#[derive(Debug)]
pub enum SK {
    Info,
    Warning,
    Error,
    Success,
}
impl SK {
    pub fn style(&self) -> Style {
        match self {
            SK::Info => Style::default().fg(constants::COLOR_DIM),
            SK::Warning => Style::default().fg(constants::COLOR_WARN),
            SK::Error => Style::default().fg(constants::COLOR_ERROR),
            SK::Success => Style::default().fg(constants::COLOR_SUCCESS),
        }
    }
    pub fn prefix(&self) -> &'static str {
        match self {
            SK::Info => "ℹ",
            SK::Warning => "⚠",
            SK::Error => "✗",
            SK::Success => "✓",
        }
    }
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
            Popup::PreviewAdd(profile) => {
                let text = format!(
                    "Add new profile?\n\n\
                Tag: {}\n\
                Protocol: {}\n\
                Host: {}:{}\n\
                \n\
                [Enter] Confirm  [Esc] Cancel",
                    profile.tag.as_deref().unwrap_or("untitled"),
                    profile.protocol.as_str(),
                    profile.host,
                    profile.port
                );
                render_popup_with_footer(frame, layout[1], "Confirm Add", &text, "");
            }
        }
    }
}
pub fn render_popup_with_footer(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    message: &str,
    footer_hints: &str,
) {
    let popup_area = centered_popup(area, 50, 40);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(1)])
        .split(popup_area);

    frame.render_widget(Clear, popup_area);

    let content_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            title,
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Cyan),
        ));

    let content = Paragraph::new(message)
        .block(content_block)
        .wrap(ratatui::widgets::Wrap { trim: true });

    frame.render_widget(content, chunks[0]);

    if !footer_hints.is_empty() {
        let footer = Paragraph::new(Line::from(Span::styled(
            footer_hints,
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM),
        )))
        .alignment(ratatui::prelude::Alignment::Center);

        frame.render_widget(footer, chunks[1]);
    }
}

pub fn render_simple_popup(frame: &mut Frame, area: Rect, title: &str, message: &str) {
    let popup_area = centered_popup(area, 50, 30);
    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red))
        .title(Span::styled(
            title,
            Style::default().add_modifier(Modifier::BOLD).fg(Color::Red),
        ));

    let paragraph = Paragraph::new(message)
        .block(block)
        .wrap(ratatui::widgets::Wrap { trim: true })
        .alignment(ratatui::prelude::Alignment::Center);

    frame.render_widget(paragraph, popup_area);
}

pub fn render_confirm_popup(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    message: &str,
    yes_hint: &str,
    no_hint: &str,
) {
    let popup_area = centered_popup(area, 50, 40);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(1)])
        .split(popup_area);

    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(Span::styled(
            title,
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Yellow),
        ));

    frame.render_widget(
        Paragraph::new(message)
            .block(block)
            .wrap(ratatui::widgets::Wrap { trim: true }),
        chunks[0],
    );

    let footer = format!(
        "[{}] {}   [{}] {}",
        Span::styled("Enter", Style::default().fg(Color::Green)),
        yes_hint,
        Span::styled("Esc", Style::default().fg(Color::Red)),
        no_hint
    );

    frame.render_widget(
        Paragraph::new(footer)
            .style(Style::default().fg(Color::DarkGray))
            .alignment(ratatui::prelude::Alignment::Center),
        chunks[1],
    );
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

    let footer = Paragraph::new(footer_text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(constants::BORDER_STYLE)
            .padding(constants::PADDING)
            .title("Status"),
    );
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
    let style = app.status.sk.style();
    let prefix = app.status.sk.prefix();
    let final_output =
        format!("type: {input_type} | {prefix}{status_kind} | message: {status_msg}");
    let status_bar = Span::styled(
        format!(
            "{} | {} {} | {} ",
            input_type, prefix, status_kind, status_msg
        ),
        style,
    );

    frame.render_widget(status_bar, area);
}
fn render_exit_popup(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let popup_area = centered_popup(area, 50, 40);

    let popup_text = String::from("You sure you wanna exit?\n[y][n]");
    frame.render_widget(Clear, popup_area);
    frame.render_widget(
        Paragraph::new(popup_text).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(constants::BORDER_STYLE)
                .padding(constants::PADDING)
                .title("Confirm Exit"),
        ),
        popup_area,
    );
}
fn render_confirmdel_popup(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let popup_area = centered_popup(area, 50, 40);

    let popup_text = String::from(
        "Confirm delete? Profile data will be deleted from memory.\n Enter - yes, Esc - no",
    );
    frame.render_widget(Clear, popup_area);
    frame.render_widget(
        Paragraph::new(popup_text).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(constants::BORDER_STYLE)
                .padding(constants::PADDING)
                .title("Confirm Delete"),
        ),
        popup_area,
    );
}
fn render_parseres_popup(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let popup_area = centered_popup(area, 50, 40);

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

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(constants::BORDER_STYLE)
        .padding(constants::PADDING)
        .title(Span::styled(
            "Parse Result",
            Style::default().add_modifier(Modifier::BOLD),
        ));

    frame.render_widget(Clear, popup_area);
    frame.render_widget(Paragraph::new(popup_text).block(block), popup_area);
}

fn render_popup_overlay(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {}
fn render_error_popup(frame: &mut Frame, app: &App, area: Rect, message: &str) {
    let mut popup_area = centered_popup(area, 50, 40);
    let mut par = String::from("Error: ");
    par.push_str(message);
    let widget = Paragraph::new(par).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(constants::BORDER_STYLE)
            .padding(constants::PADDING)
            .title("Error!"),
    );
    frame.render_widget(Clear, popup_area);
    frame.render_widget(widget, popup_area);
}
fn render_connection_popup(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let popup_area = centered_popup(area, 50, 40);
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
        Paragraph::new(popup_text).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(constants::BORDER_STYLE)
                .padding(constants::PADDING)
                .title("Connection"),
        ),
        popup_area,
    );
}
pub fn centered_popup(area: Rect, width_percent: u16, height_percent: u16) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - height_percent) / 2),
            Constraint::Percentage(height_percent),
            Constraint::Percentage((100 - height_percent) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - width_percent) / 2),
            Constraint::Percentage(width_percent),
            Constraint::Percentage((100 - width_percent) / 2),
        ])
        .split(popup_layout[1])[1]
}
