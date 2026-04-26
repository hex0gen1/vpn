use crate::app::App;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

/*pub fn render(frame: &mut Frame, app: &App, area: Rect) {
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
}*/
pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(area);

    render_header(frame, app, chunks[0]);
    render_input_field(frame, app, chunks[1]);
    render_format_hint(frame, chunks[2]);
    render_hints(frame, app, chunks[3]);
}

fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let title = Span::styled(
        " Add Profile (VLESS) ",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );

    let desc = if app.profiles.is_empty() {
        "No profiles yet. Paste a vless:// link below to get started. To enter input mode press 'y'."
    } else {
        "Add another profile by pasting a vless:// link. To enter input mode press 'y'."
    };

    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(Color::DarkGray));

    let text = Line::from(vec![
        title,
        Span::raw(" — "),
        Span::styled(desc, Style::default().fg(Color::DarkGray)),
    ]);

    frame.render_widget(Paragraph::new(text).block(block), area);
}

fn render_input_field(frame: &mut Frame, app: &App, area: Rect) {
    let is_focused = matches!(app.mode, crate::app::Mode::Input);
    let input = &app.input_str;

    let content = if input.is_empty() {
        Line::from(Span::styled(
            "Paste vless:// link here...",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        ))
    } else {
        let mut spans = vec![Span::raw(input)];
        if is_focused {
            spans.push(Span::styled(
                "█",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::RAPID_BLINK),
            ));
        }
        Line::from(spans)
    };

    let border_style = if is_focused {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(Span::styled(
            if is_focused {
                " INPUT FOCUSED "
            } else {
                " Input "
            },
            Style::default().fg(if is_focused {
                Color::Green
            } else {
                Color::DarkGray
            }),
        ));

    let paragraph = Paragraph::new(content)
        .block(block)
        .wrap(Wrap { trim: false })
        .style(Style::default().fg(Color::White));

    frame.render_widget(paragraph, area);
}

fn render_format_hint(frame: &mut Frame, area: Rect) {
    let hint = Line::from(vec![
        Span::styled(
            "Format: ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("vless://"),
        Span::styled(
            "<uuid>",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::UNDERLINED),
        ),
        Span::raw("@"),
        Span::styled(
            "<host>",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::UNDERLINED),
        ),
        Span::raw(":"),
        Span::styled(
            "<port>",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::UNDERLINED),
        ),
        Span::raw("?security=none&type=tcp#"),
        Span::styled(
            "<tag>",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::UNDERLINED),
        ),
    ]);

    frame.render_widget(
        Paragraph::new(hint)
            .style(Style::default().fg(Color::DarkGray))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_hints(frame: &mut Frame, app: &App, area: Rect) {
    let status = if !app.input_str.is_empty() {
        Span::styled(
            format!("{} chars ready", app.input_str.len()),
            Style::default().fg(Color::Green),
        )
    } else {
        Span::raw("")
    };

    let hints = Line::from(vec![
        Span::styled(
            "[Ctrl+V]",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" paste  "),
        Span::styled(
            "[Enter]",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" parse  "),
        Span::styled(
            "[Esc]",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" cancel  "),
        status,
    ]);

    frame.render_widget(
        Paragraph::new(hints)
            .style(Style::default().fg(Color::DarkGray))
            .alignment(ratatui::prelude::Alignment::Right),
        area,
    );
}
