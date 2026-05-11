use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Block, Borders, Paragraph},
};

use crate::app::App;
use crate::screens::constants;
/*pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let mut text = String::from("Profiles:\n\n");
    let profiles = vpn_daemon::parser::parse_vless::list_profiles();
    for (idx,profile) in profiles.iter() {

        vpn_daemon::parser::parse_vless::load_profile(profile.tag.unwrap());
    }
    for (profile, profiles) in profiles.iter().enumerate() {
        if profile == app.selected_profile {
            text.push_str(&format!("> {}\n", profiles.display_name()));
        } else {
            text.push_str(&format!("  {}\n", profiles.display_name()));
        }
    }

    let widget = Paragraph::new(text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(constants::BORDER_STYLE)
            .padding(constants::PADDING),
    );

    frame.render_widget(widget, area);
}*/
pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ratatui::widgets::ListItem> = app
        .profiles
        .iter()
        .enumerate()
        .map(|(i, tag)| {
            let style = if i == app.selected_profile {
                ratatui::style::Style::default()
                    .fg(ratatui::style::Color::Yellow)
                    .add_modifier(ratatui::style::Modifier::BOLD)
            } else {
                ratatui::style::Style::default()
            };
            ratatui::widgets::ListItem::new(format!(
                " {} {:?}",
                if i == app.selected_profile {
                    "▶"
                } else {
                    " "
                },
                tag
            ))
            .style(style)
        })
        .collect();

    let list = ratatui::widgets::List::new(items).block(
        Block::default()
            .title("Select profile (↑↓/Enter)")
            .borders(Borders::ALL),
    );
    frame.render_widget(list, area);
}
