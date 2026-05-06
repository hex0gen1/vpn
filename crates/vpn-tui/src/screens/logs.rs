use ratatui::{Frame, layout::Rect, widgets::Paragraph};

use crate::app::App;

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let widget = Paragraph::new("Logs");
    frame.render_widget(widget, area);
}
