mod app;
mod audio;
mod ui;

use std::env;
use std::path::PathBuf;
use std::time::Duration;
use anyhow::Result;
use crossterm::event::{self, Event, KeyEventKind};

use crate::app::App;

fn main() -> Result<()> {
    let target = env::args().nth(1).map(PathBuf::from);
    let mut app = App::new(target)?;

    let mut term = ratatui::init();
    let res = run(&mut term, &mut app);
    ratatui::restore();

    res
}

fn run(term: &mut ratatui::DefaultTerminal, app: &mut App) -> Result<()> {
    while !app.quit {
        term.draw(|f| ui::render(f, app))?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    app.handle_key(key.code)?;
                }
            }
        }
        app.tick()?;
    }
    Ok(())
}
