use crate::app::App;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style, Stylize},
    widgets::{Block, Borders, Paragraph},
};

pub fn render(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let [header, body, description] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(5),
    ])
    .areas(area);
    let profile = &app.profiles[app.selected_profile];
    let metadata = app.build_metadata_group();
    let generaldata = App::build_general_group(profile);
    let securitydata = App::build_security_group(profile);
    let realitydata = App::build_reality_group(profile);
    let transportdata = App::build_transport_group(profile);

    let Groups = vec![
        metadata,
        generaldata,
        securitydata,
        realitydata,
        transportdata,
    ];
    let mut text = String::new();
    for group in &Groups {
        text.push_str(group.name.as_str().as_str());
        text.push('\n');

        for field in &group.data {
            text.push_str(&format!(" {} : {}", field.name.as_str(), field.data));
        }

        text.push('\n')
    }

    frame.render_widget(
        Paragraph::new(text).block(Block::default().borders(Borders::ALL)),
        body,
    );
}
