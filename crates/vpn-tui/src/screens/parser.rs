use crate::app::App;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    widgets::{Block, Borders, Paragraph},
};

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    //let chunks = Layout::default()
    //    .direction(Direction::Horizontal)
    //    .constraints([
    //        Constraint::Length(8),
    //        Constraint::Length(30),
    //        Constraint::Min(5),
    //    ])
    //    .split(area);
    let note = Paragraph::new(
        "Here you can use parser-function to add VPN profile.
        at the current state of development only vless config is available, so be patient. 
        You can get config-link from distributor XANOST[tg-bot].",
    )
    .block(Block::default().borders(Borders::ALL));
    let mut sec_rect = Rect::from(area);
    sec_rect.y += 10;
    sec_rect.x += 1;
    frame.render_widget(&app.input_str, sec_rect);
    frame.render_widget(note, area);
}
