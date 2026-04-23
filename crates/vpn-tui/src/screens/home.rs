use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    widgets::{Block, Borders, Paragraph},
};

use crate::app::App;

const LOGO: &str = r#"
██   ██  █████  ███    ██  ██████  ███████ ████████
╚██ ██╝ ██   ██ ████   ██ ██    ██ ██         ██
 ╚███╝  ███████ ██ ██  ██ ██    ██ ███████    ██
 ██ ██  ██   ██ ██  ██ ██ ██    ██      ██    ██
██   ██ ██   ██ ██   ████  ██████  ███████    ██
"#;

pub fn render(frame: &mut Frame, _app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),
            Constraint::Length(15),
            Constraint::Min(3),
        ])
        .split(area);

    let logo = Paragraph::new(LOGO).block(Block::default().borders(Borders::ALL).title("Logo"));

    let welcome = Paragraph::new(
        "Welcome to Xanost VPN\n\nPress 'p' to open profiles\nPress 'l' to open logs\nPress y to use parser functions\nPress 'q' to quit"
    )
    .block(Block::default().borders(Borders::ALL).title("Welcome"));

    frame.render_widget(logo, chunks[0]);
    frame.render_widget(welcome, chunks[1]);
}
