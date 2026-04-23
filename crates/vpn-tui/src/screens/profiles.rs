use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Block, Borders, Paragraph},
};

use crate::app::App;

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let mut text = String::from("Profiles:\n\n");

    for (idx, profile) in app.profiles.iter().enumerate() {
        if idx == app.selected_profile {
            text.push_str(&format!("> {}\n", profile.display_name()));
        } else {
            text.push_str(&format!("  {}\n", profile.display_name()));
        }
    }

    let widget =
        Paragraph::new(text).block(Block::default().title("Profiles").borders(Borders::ALL));

    frame.render_widget(widget, area);
}
