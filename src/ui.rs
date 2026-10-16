use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    symbols::Marker,
    text::{Line, Span},
    widgets::{
        canvas::{Canvas, Line as CanvasLine},
        Block, BorderType, Clear, Gauge, Paragraph,
    },
    Frame,
};

use crate::app::{App, VizMode};
use crate::theme::blend;

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

    if app.show_help {
        render_help(f, app);
    }
}

fn render_top(f: &mut Frame, app: &App, area: Rect) {
    let th = app.theme();
    let state = app.state_str();
    let name = app.track_name();
    let vol = (app.audio.volume() * 100.0).round() as u32;
    let track_num = if app.tracks.is_empty() {
        String::new()
    } else {
        format!(" [{}/{}]", app.idx + 1, app.tracks.len())
    };

    let text = Line::from(vec![
        Span::styled(format!("[{state}] "), Style::default().fg(th.primary).add_modifier(Modifier::BOLD)),
        Span::styled(name, Style::default().fg(th.text)),
        Span::styled(track_num, Style::default().fg(th.dimmed)),
        Span::styled(" | ", Style::default().fg(th.dimmed)),
        Span::styled(format!("Vol: {vol}%"), Style::default().fg(th.text)),
        Span::styled(" | ", Style::default().fg(th.dimmed)),
        Span::styled(th.name, Style::default().fg(th.secondary)),
    ]);

    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(th.dimmed))
        .title("Player");
    f.render_widget(Paragraph::new(text).block(block), area);
}

fn render_middle(f: &mut Frame, app: &App, area: Rect) {
    if app.show_sidebar {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(28),
                Constraint::Percentage(44),
                Constraint::Percentage(28),
            ])
            .split(area);

        render_sidebar(f, app, chunks[0]);
        render_visualizer(f, app, chunks[1]);
        render_lyrics(f, app, chunks[2]);
    } else {
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
}

fn render_sidebar(f: &mut Frame, app: &App, area: Rect) {
    let th = app.theme();
    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(th.dimmed))
        .title(format!("Tracks [{}]", app.tracks.len()));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    if app.tracks.is_empty() {
        let msg = Line::from(Span::styled("No tracks", Style::default().fg(th.dimmed)));
        f.render_widget(Paragraph::new(vec![msg]), inner);
        return;
    }

    let height = inner.height as usize;
    let offset = if app.selected_idx >= height {
        app.selected_idx - height + 1
    } else {
        0
    };

    let end = (offset + height).min(app.tracks.len());
    let mut lines = Vec::with_capacity(end - offset);

    for i in offset..end {
        let path = &app.tracks[i];
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown");
        let is_playing = i == app.idx;
        let is_sel = i == app.selected_idx;

        let prefix = if is_playing { "> " } else { "  " };
        let style = if is_sel {
            Style::default().fg(th.primary).add_modifier(Modifier::REVERSED)
        } else if is_playing {
            Style::default().fg(th.secondary).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(th.text)
        };

        lines.push(Line::from(vec![
            Span::styled(prefix, style),
            Span::styled(name, style),
        ]));
    }

    f.render_widget(Paragraph::new(lines), inner);
}

fn render_visualizer(f: &mut Frame, app: &App, area: Rect) {
    let th = app.theme();
    let border_color = blend(th.dimmed, th.accent, app.pulse);
    let title = format!("Visualizer [{}]", app.viz_mode.name());
    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(border_color))
        .title(title);

    match app.viz_mode {
        VizMode::Spectrum => {
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

                let mut row_spans = Vec::new();
                let mut curr_str = String::new();
                let mut curr_fg = th.primary;

                for x in 0..width {
                    let t = (x as f32 / width as f32) * (app.bins.len() - 1) as f32;
                    let i = t.floor() as usize;
                    let frac = t - i as f32;
                    let v = if i + 1 < app.bins.len() {
                        app.bins[i] * (1.0 - frac) + app.bins[i + 1] * frac
                    } else {
                        app.bins[i]
                    };
                    let p_val = if i + 1 < app.peaks.len() {
                        app.peaks[i] * (1.0 - frac) + app.peaks[i + 1] * frac
                    } else {
                        app.peaks[i]
                    };

                    let col_eighths = (v * (height * 8) as f32).round() as usize;
                    let peak_eighths = (p_val * (height * 8) as f32).round() as usize;
                    let peak_row = peak_eighths / 8;

                    let (ch, fg) = if col_eighths >= max_level {
                        ('█', th.primary)
                    } else if col_eighths > min_level {
                        (BLOCKS[col_eighths - min_level], th.primary)
                    } else if peak_row == row_from_bottom && peak_eighths > 0 {
                        ('─', th.secondary)
                    } else {
                        (' ', th.primary)
                    };

                    if fg != curr_fg && !curr_str.is_empty() {
                        row_spans.push(Span::styled(curr_str, Style::default().fg(curr_fg)));
                        curr_str = String::new();
                        curr_fg = fg;
                    }
                    curr_str.push(ch);
                }
                if !curr_str.is_empty() {
                    row_spans.push(Span::styled(curr_str, Style::default().fg(curr_fg)));
                }
                lines.push(Line::from(row_spans));
            }
            f.render_widget(Paragraph::new(lines), inner);
        }
        VizMode::Oscilloscope => {
            let samples = app.wave_samples(area.width as usize * 2);
            let canvas = Canvas::default()
                .block(block)
                .marker(Marker::Braille)
                .x_bounds([0.0, (samples.len().max(2) - 1) as f64])
                .y_bounds([-1.0, 1.0])
                .paint(|ctx| {
                    ctx.draw(&CanvasLine::new(0.0, 0.0, samples.len() as f64, 0.0, th.dimmed));
                    for i in 0..samples.len().saturating_sub(1) {
                        ctx.draw(&CanvasLine::new(
                            i as f64,
                            samples[i] as f64,
                            (i + 1) as f64,
                            samples[i + 1] as f64,
                            th.primary,
                        ));
                    }
                });
            f.render_widget(canvas, area);
        }
        VizMode::CenterWave => {
            let (left, right) = app.stereo_samples(area.width as usize * 2);
            let canvas = Canvas::default()
                .block(block)
                .marker(Marker::Braille)
                .x_bounds([0.0, (left.len().max(2) - 1) as f64])
                .y_bounds([-1.0, 1.0])
                .paint(|ctx| {
                    ctx.draw(&CanvasLine::new(0.0, 0.0, left.len() as f64, 0.0, th.dimmed));
                    for i in 0..left.len().saturating_sub(1) {
                        let l1 = (left[i].abs() as f64).clamp(0.0, 1.0);
                        let l2 = (left[i + 1].abs() as f64).clamp(0.0, 1.0);
                        ctx.draw(&CanvasLine::new(i as f64, l1, (i + 1) as f64, l2, th.primary));

                        let r1 = -(right[i].abs() as f64).clamp(0.0, 1.0);
                        let r2 = -(right[i + 1].abs() as f64).clamp(0.0, 1.0);
                        ctx.draw(&CanvasLine::new(i as f64, r1, (i + 1) as f64, r2, th.secondary));
                    }
                });
            f.render_widget(canvas, area);
        }
    }
}

fn render_lyrics(f: &mut Frame, app: &App, area: Rect) {
    let th = app.theme();
    let title = if app.lyrics.offset_ms != 0 {
        format!("Lyrics [{:+}]", app.lyrics.offset_ms)
    } else {
        "Lyrics".to_string()
    };

    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(th.dimmed))
        .title(title);
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    if app.lyrics.lines.is_empty() {
        let msg = Line::from(Span::styled("No lyrics available", Style::default().fg(th.dimmed))).centered();
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
                Style::default().fg(th.secondary).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(th.dimmed)
            };
            lines.push(Line::from(Span::styled(&l.text, style)).centered());
        } else {
            lines.push(Line::raw(""));
        }
    }

    f.render_widget(Paragraph::new(lines), inner);
}

fn render_bottom(f: &mut Frame, app: &App, area: Rect) {
    let th = app.theme();
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
        .block(
            Block::bordered()
                .border_type(BorderType::Plain)
                .border_style(Style::default().fg(th.dimmed))
                .title("Progress"),
        )
        .gauge_style(Style::default().fg(th.primary))
        .ratio(ratio)
        .label(Span::styled(label, Style::default().fg(th.text)));

    f.render_widget(gauge, area);
}

fn render_help(f: &mut Frame, app: &App) {
    let th = app.theme();
    let area = f.area();
    let w = 46.min(area.width);
    let h = 15.min(area.height);
    let rect = Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    };

    f.render_widget(Clear, rect);

    let binds = [
        ("Space", "Play / Pause"),
        ("s", "Restart / Skip"),
        ("+ / -", "Volume Up / Down"),
        ("[ / ]", "Lyrics Offset (-/+100ms)"),
        ("v", "Visualizer Mode"),
        ("t", "Color Theme"),
        ("Tab / p", "Toggle Track Drawer"),
        ("j / k (Up/Down)", "Select Track"),
        ("Enter", "Play Selected Track"),
        ("?", "Toggle Keybindings"),
        ("q / Esc", "Quit / Close"),
    ];

    let mut lines = Vec::with_capacity(binds.len());
    for (k, desc) in binds {
        lines.push(Line::from(vec![
            Span::styled(format!("  {:<18}", k), Style::default().fg(th.primary).add_modifier(Modifier::BOLD)),
            Span::styled(desc, Style::default().fg(th.text)),
        ]));
    }

    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(th.accent))
        .title("Keybindings");

    f.render_widget(Paragraph::new(lines).block(block), rect);
}
