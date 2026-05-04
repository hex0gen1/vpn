use crate::app::App;
use crate::screens::constants;
use crate::ui::Hotkeys;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    widgets::{Block, Borders, Paragraph},
};

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
        .constraints([Constraint::Length(10), Constraint::Length(15)])
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
    frame.render_widget(logo, chunks[0]);
    frame.render_widget(navigation, chunks[1]);
}
