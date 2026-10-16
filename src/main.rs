mod app;
mod audio;
mod cover;
mod lyrics;
mod mpris_sync;
mod theme;
mod ui;

use std::env;
use std::time::Duration;
use anyhow::Result;
use crossterm::event::{self, Event, KeyEventKind};

use crate::app::App;

fn main() -> Result<()> {
    let target = env::args().nth(1);
    let mut app = App::new(target.as_deref())?;

    let mut term = ratatui::init();
    let res = run(&mut term, &mut app);
    ratatui::restore();
    if app.is_kitty {
        cover::clear_kitty_image();
    }

    res
}

fn run(term: &mut ratatui::DefaultTerminal, app: &mut App) -> Result<()> {
    while !app.quit {
        term.draw(|f| ui::render(f, app))?;

        if app.is_kitty && app.use_kitty_art && app.kitty_needs_draw.get() {
            if app.art_rect.get().is_some() && app.cover.png.is_some() {
                let rect = app.art_rect.get().unwrap();
                let png = app.cover.png.as_ref().unwrap();
                cover::send_kitty_image(png, rect.x, rect.y, rect.width, rect.height);
                app.kitty_needs_draw.set(false);
            }
        }

        if event::poll(Duration::from_millis(16))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    app.handle_key(key.code)?;
                }
                _ => {}
            }
        }
        app.tick()?;
    }
    if app.is_kitty {
        cover::clear_kitty_image();
    }
    Ok(())
}
