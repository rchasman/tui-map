mod app;
mod braille;
mod data;
mod map;
mod ui;

use anyhow::Result;
use app::App;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseButton,
    MouseEvent, MouseEventKind,
};
use crossterm::execute;
use ratatui::DefaultTerminal;
use std::path::Path;
use std::time::Duration;

fn main() -> Result<()> {
    // Initialize terminal
    let mut terminal = ratatui::init();
    terminal.clear()?;

    // Enable mouse capture
    execute!(std::io::stdout(), EnableMouseCapture)?;

    // Run the app
    let result = run(&mut terminal);

    // Disable mouse capture and restore terminal
    let _ = execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();

    result
}

/// Handle mouse events for panning and zooming
fn handle_mouse(app: &mut App, mouse: MouseEvent) {
    // Always track mouse position for cursor marker
    app.set_mouse_pos(mouse.column, mouse.row);

    match mouse.kind {
        // Scroll wheel for zooming towards mouse position
        MouseEventKind::ScrollUp => app.zoom_in_at(mouse.column, mouse.row),
        MouseEventKind::ScrollDown => app.zoom_out_at(mouse.column, mouse.row),
        // Horizontal scroll for panning (trackpad two-finger swipe)
        MouseEventKind::ScrollLeft => app.pan(-15, 0),
        MouseEventKind::ScrollRight => app.pan(15, 0),
        // Click and drag to pan
        MouseEventKind::Down(MouseButton::Left) => {
            app.last_mouse = Some((mouse.column, mouse.row));
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            app.handle_drag(mouse.column, mouse.row);
        }
        MouseEventKind::Up(MouseButton::Left) => {
            app.end_drag();
        }
        // Right click to launch nuke
        MouseEventKind::Down(MouseButton::Right) => {
            app.launch_nuke(mouse.column, mouse.row);
        }
        _ => {}
    }
}

fn run(terminal: &mut DefaultTerminal) -> Result<()> {
    let size = terminal.size()?;
    let mut app = App::new(size.width as usize, size.height as usize);

    // Load all available GeoJSON data at different resolutions
    let data_dir = Path::new("data");
    if data_dir.exists() {
        let _ = data::load_all_geojson(&mut app.map_renderer, data_dir);
    }

    // Fall back to simple world if no data loaded
    if !app.map_renderer.has_data() {
        data::generate_simple_world(&mut app.map_renderer);
    }

    // Main loop
    loop {
        // Draw
        terminal.draw(|frame| ui::render(frame, &app))?;

        // Handle events with ~60fps target
        if event::poll(Duration::from_millis(16))? {
            match event::read()? {
                Event::Key(key) => {
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

                            // Layer toggles
                            KeyCode::Char('b') | KeyCode::Char('B') => {
                                app.map_renderer.toggle_borders();
                            }
                            KeyCode::Char('s') | KeyCode::Char('S') => {
                                app.map_renderer.toggle_states();
                            }
                            KeyCode::Char('c') | KeyCode::Char('C') => {
                                app.map_renderer.toggle_cities();
                            }
                            KeyCode::Char('y') | KeyCode::Char('Y') => {
                                app.map_renderer.toggle_counties();
                            }
                            KeyCode::Char('L') => {
                                app.map_renderer.toggle_labels();
                            }
                            KeyCode::Char('p') | KeyCode::Char('P') => {
                                app.map_renderer.toggle_population();
                            }

                            // Reset view
                            KeyCode::Char('r') | KeyCode::Char('0') => {
                                let size = terminal.size()?;
                                app = App::new(size.width as usize, size.height as usize);
                                let _ = data::load_all_geojson(&mut app.map_renderer, data_dir);
                                if !app.map_renderer.has_data() {
                                    data::generate_simple_world(&mut app.map_renderer);
                                }
                            }

                            _ => {}
                        }
                    }
                }
                Event::Mouse(mouse) => {
                    handle_mouse(&mut app, mouse);
                }
                Event::Resize(width, height) => {
                    app.resize(width as usize, height as usize);
                }
                _ => {}
            }
        }

        // Update explosion animations
        app.update_explosions();

        if app.should_quit {
            break;
        }
    }

    Ok(())
}
