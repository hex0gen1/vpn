use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Block, Borders, Paragraph},
};

use crate::app::App;
use crate::screens::constants;
pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let mut text = String::from("Profiles:\n\n");

    for (idx, profile) in app.profiles.iter().enumerate() {
        if idx == app.selected_profile {
            text.push_str(&format!("> {}\n", profile.display_name()));
        } else {
            text.push_str(&format!("  {}\n", profile.display_name()));
        }
    }

    let widget = Paragraph::new(text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(constants::BORDER_STYLE)
            .padding(constants::PADDING),
    );

    frame.render_widget(widget, area);
}
