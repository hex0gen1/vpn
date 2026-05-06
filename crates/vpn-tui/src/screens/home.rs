use crate::app::App;
use crate::screens::constants;
use crate::ui::Hotkeys;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
};

const LOGO: &str = r#"
██   ██  █████  ███    ██  ██████  ███████ ████████
╚██ ██╝ ██   ██ ████   ██ ██    ██ ██         ██
 ╚███╝  ███████ ██ ██  ██ ██    ██ ███████    ██
 ██ ██  ██   ██ ██  ██ ██ ██    ██      ██    ██
██   ██ ██   ██ ██   ████  ██████  ███████    ██
"#;

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(10),
            Constraint::Length(15),
            Constraint::Max(30),
        ])
        .split(area);

    let logo = Paragraph::new(LOGO).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(constants::BORDER_STYLE)
            .padding(constants::PADDING)
            .title("Logo"),
    );

    let text_to_navigation = format!(
        "This tui contains multi-screen hovering | Hotkeys to navigate:\n\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n",
        Hotkeys::MvParser.as_str(),
        Hotkeys::MvProfiles.as_str(),
        Hotkeys::MvHome.as_str(),
        Hotkeys::ExpandDetails.as_str(),
        Hotkeys::MvLogs.as_str(),
        Hotkeys::Quit.as_str(),
        Hotkeys::ImportLink.as_str(),
        "i: enter input mode",
        "esc: leave input mode",
    );
    let navigation = Paragraph::new(text_to_navigation).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(constants::BORDER_STYLE)
            .padding(constants::PADDING),
    );
    let visible_logs = if app.logs.len() > 15 {
        &app.logs[app.logs.len() - 15..]
    } else {
        &app.logs[..]
    };
    let lines: Vec<Line> = visible_logs
        .iter()
        .map(|line| {
            if line.contains("ERROR") {
                Line::from(Span::styled(
                    line,
                    Style::default().fg(constants::COLOR_ERROR),
                ))
            } else if line.contains("WARN") {
                Line::from(Span::styled(
                    line,
                    Style::default().fg(constants::COLOR_WARN),
                ))
            } else if line.contains("DEBUG") {
                Line::from(Span::styled(
                    line,
                    Style::default().fg(constants::COLOR_DIM),
                ))
            } else if line.contains("FATAL") {
                Line::from(Span::styled(
                    line,
                    Style::default().fg(constants::COLOR_FATAL),
                ))
            } else {
                Line::from(Span::styled(
                    line,
                    Style::default().fg(constants::COLOR_DIM),
                ))
            }
        })
        .collect();

    let logs_widget = Paragraph::new(Text::from(lines))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(constants::BORDER_STYLE)
                .title(" SYSTEM LOGS "),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(logo, chunks[0]);
    frame.render_widget(navigation, chunks[1]);
    frame.render_widget(logs_widget, chunks[2]);
}
