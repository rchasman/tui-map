mod app;
mod braille;
mod data;
mod map;
mod ui;

use anyhow::Result;
use app::App;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::DefaultTerminal;
use std::path::Path;
use std::time::Duration;

fn main() -> Result<()> {
    // Initialize terminal
    let mut terminal = ratatui::init();
    terminal.clear()?;

    // Run the app
    let result = run(&mut terminal);

    // Restore terminal
    ratatui::restore();

    result
}

fn run(terminal: &mut DefaultTerminal) -> Result<()> {
    let size = terminal.size()?;
    let mut app = App::new(size.width as usize, size.height as usize);

    // Try to load GeoJSON data, fall back to simple world
    let data_path = Path::new("data/natural-earth.json");
    if data_path.exists() {
        if let Err(e) = data::load_geojson(&mut app.map_renderer, data_path) {
            eprintln!("Warning: Failed to load GeoJSON: {}", e);
            data::generate_simple_world(&mut app.map_renderer);
        }
    } else {
        data::generate_simple_world(&mut app.map_renderer);
    }

    // Main loop
    loop {
        // Draw
        terminal.draw(|frame| ui::render(frame, &app))?;

        // Handle events with ~60fps target
        if event::poll(Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
                // Only handle key press events (not release)
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.quit(),

                        // Pan with hjkl or arrow keys
                        KeyCode::Left | KeyCode::Char('h') => app.pan(-10, 0),
                        KeyCode::Right | KeyCode::Char('l') => app.pan(10, 0),
                        KeyCode::Up | KeyCode::Char('k') => app.pan(0, -6),
                        KeyCode::Down | KeyCode::Char('j') => app.pan(0, 6),

                        // Zoom
                        KeyCode::Char('+') | KeyCode::Char('=') => app.zoom_in(),
                        KeyCode::Char('-') | KeyCode::Char('_') => app.zoom_out(),

                        // Reset view
                        KeyCode::Char('r') | KeyCode::Char('0') => {
                            let size = terminal.size()?;
                            app = App::new(size.width as usize, size.height as usize);
                            data::generate_simple_world(&mut app.map_renderer);
                        }

                        _ => {}
                    }
                }
            } else if let Event::Resize(width, height) = event::read()? {
                app.resize(width as usize, height as usize);
            }
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}
