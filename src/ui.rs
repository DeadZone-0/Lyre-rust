use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Gauge, Paragraph},
    Frame,
};

use crate::app::App;

pub fn render(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(6),
            Constraint::Length(3),
        ])
        .split(f.area());

    render_top(f, app, chunks[0]);
    render_middle(f, chunks[1]);
    render_bottom(f, app, chunks[2]);
}

fn render_top(f: &mut Frame, app: &App, area: Rect) {
    let state = app.state_str();
    let name = app.track_name();
    let vol = (app.audio.volume() * 100.0).round() as u32;
    let track_num = if app.tracks.is_empty() {
        String::new()
    } else {
        format!(" [{}/{}]", app.idx + 1, app.tracks.len())
    };

    let text = Line::from(vec![
        Span::styled(format!("[{state}] "), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw(name),
        Span::styled(track_num, Style::default().fg(Color::DarkGray)),
        Span::raw(" | "),
        Span::styled(format!("Vol: {vol}%"), Style::default().fg(Color::Gray)),
    ]);

    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .title("Player");
    f.render_widget(Paragraph::new(text).block(block), area);
}

fn render_middle(f: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(60),
            Constraint::Percentage(40),
        ])
        .split(area);

    let viz_block = Block::bordered()
        .border_type(BorderType::Plain)
        .title("Visualizer");
    f.render_widget(viz_block, chunks[0]);

    let lyrics_block = Block::bordered()
        .border_type(BorderType::Plain)
        .title("Lyrics");
    f.render_widget(lyrics_block, chunks[1]);
}

fn render_bottom(f: &mut Frame, app: &App, area: Rect) {
    let pos = app.audio.pos();
    let dur = app.audio.duration;

    let (ratio, label) = match dur {
        Some(d) if d.as_secs() > 0 => {
            let r = (pos.as_secs_f64() / d.as_secs_f64()).clamp(0.0, 1.0);
            let lbl = format!(
                "{:02}:{:02} / {:02}:{:02}",
                pos.as_secs() / 60,
                pos.as_secs() % 60,
                d.as_secs() / 60,
                d.as_secs() % 60
            );
            (r, lbl)
        }
        _ => {
            let lbl = format!("{:02}:{:02} / --:--", pos.as_secs() / 60, pos.as_secs() % 60);
            (0.0, lbl)
        }
    };

    let gauge = Gauge::default()
        .block(Block::bordered().border_type(BorderType::Plain).title("Progress"))
        .gauge_style(Style::default().fg(Color::Cyan))
        .ratio(ratio)
        .label(label);

    f.render_widget(gauge, area);
}
