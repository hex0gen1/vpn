use ratatui::{Frame, layout::Rect, widgets::Paragraph};

use crate::app::App;

pub fn render(frame: &mut Frame, _app: &App, area: Rect) {
    let widget = Paragraph::new("Logs screen");
    frame.render_widget(widget, area);
}
