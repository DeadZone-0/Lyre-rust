use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Gauge, Paragraph},
    Frame,
};

use crate::app::App;

const BLOCKS: [char; 9] = [' ', ' ', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

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
    render_middle(f, app, chunks[1]);
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

fn render_middle(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(60),
            Constraint::Percentage(40),
        ])
        .split(area);

    render_visualizer(f, app, chunks[0]);
    render_lyrics(f, app, chunks[1]);
}

fn render_visualizer(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .title("Visualizer");
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 || app.bins.is_empty() {
        return;
    }

    let width = inner.width as usize;
    let height = inner.height as usize;
    let mut lines = Vec::with_capacity(height);

    for y in 0..height {
        let row_from_bottom = height - 1 - y;
        let min_level = row_from_bottom * 8;
        let max_level = min_level + 8;

        let mut row_chars = String::with_capacity(width);
        for x in 0..width {
            let t = (x as f32 / width as f32) * (app.bins.len() - 1) as f32;
            let i = t.floor() as usize;
            let frac = t - i as f32;
            let v = if i + 1 < app.bins.len() {
                app.bins[i] * (1.0 - frac) + app.bins[i + 1] * frac
            } else {
                app.bins[i]
            };

            let col_eighths = (v * (height * 8) as f32).round() as usize;
            let ch = if col_eighths >= max_level {
                '█'
            } else if col_eighths <= min_level {
                ' '
            } else {
                BLOCKS[col_eighths - min_level]
            };
            row_chars.push(ch);
        }
        lines.push(Line::from(Span::styled(row_chars, Style::default().fg(Color::Cyan))));
    }

    f.render_widget(Paragraph::new(lines), inner);
}

fn render_lyrics(f: &mut Frame, app: &App, area: Rect) {
    let title = if app.lyrics.offset_ms != 0 {
        format!("Lyrics [{:+}ms]", app.lyrics.offset_ms)
    } else {
        "Lyrics".to_string()
    };

    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .title(title);
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    if app.lyrics.lines.is_empty() {
        let msg = Line::from(Span::styled("No lyrics available", Style::default().fg(Color::DarkGray))).centered();
        let mid = inner.height / 2;
        let mut lines = vec![Line::raw(""); mid as usize];
        lines.push(msg);
        f.render_widget(Paragraph::new(lines), inner);
        return;
    }

    let pos = app.audio.pos();
    let cur = app.lyrics.current_index(pos);
    let center_idx = cur.unwrap_or(0) as i32;
    let mid_y = inner.height as i32 / 2;
    let mut lines = Vec::with_capacity(inner.height as usize);

    for y in 0..inner.height as i32 {
        let line_idx = center_idx + (y - mid_y);
        if line_idx >= 0 && (line_idx as usize) < app.lyrics.lines.len() {
            let l = &app.lyrics.lines[line_idx as usize];
            let is_current = cur == Some(line_idx as usize);
            let style = if is_current {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            lines.push(Line::from(Span::styled(&l.text, style)).centered());
        } else {
            lines.push(Line::raw(""));
        }
    }

    f.render_widget(Paragraph::new(lines), inner);
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
