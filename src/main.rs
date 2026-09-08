use tui_map::{app, data, ui};

use anyhow::Result;
use app::{App, WeaponType};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseButton,
    MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate};
use ratatui::DefaultTerminal;
use std::path::Path;
use std::time::{Duration, Instant};

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
            app.start_drag(mouse.column, mouse.row);
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

    // Build spatial indexes: land grid for fire filtering, feature grids for viewport queries
    app.map_renderer.build_land_grid();
    app.map_renderer.build_spatial_indexes();

    draw_frame(terminal, &mut app)?;
    let mut next_frame = Instant::now() + FRAME_INTERVAL;
    loop {
        let now = Instant::now();
        if now >= next_frame {
            // Input frequency must not change the animation rate. Drawing uses
            // part of this frame's budget rather than adding to a fixed sleep.
            next_frame = advance_frame_deadline(next_frame, now);
            app.update_explosions();
            draw_frame(terminal, &mut app)?;
        }

        // Handle queued input until the next frame is due. Even an input flood
        // returns to the deadline check after each event, so rendering cannot starve.
        if event::poll(next_frame.saturating_duration_since(Instant::now()))? {
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

                            // Toggle globe/mercator
                            KeyCode::Char('g') | KeyCode::Char('G') => {
                                app.toggle_projection();
                            }

                            // Weapon selection
                            KeyCode::Char('1') => app.select_weapon(WeaponType::Nuke),
                            KeyCode::Char('2') => app.select_weapon(WeaponType::Bio),
                            KeyCode::Char('3') => app.select_weapon(WeaponType::Emp),
                            KeyCode::Char('4') => app.select_weapon(WeaponType::Chem),
                            KeyCode::Char('5') => app.select_weapon(WeaponType::Water),
                            KeyCode::Char('6') => app.select_weapon(WeaponType::Life),

                            // Launch weapon at cursor
                            KeyCode::Char(' ') => {
                                if let Some((col, row)) = app.mouse_pos {
                                    app.launch_nuke(col, row);
                                }
                            }

                            // Reset view
                            KeyCode::Char('r') | KeyCode::Char('0') => {
                                let size = terminal.size()?;
                                app = App::new(size.width as usize, size.height as usize);
                                let _ = data::load_all_geojson(&mut app.map_renderer, data_dir);
                                if !app.map_renderer.has_data() {
                                    data::generate_simple_world(&mut app.map_renderer);
                                }
                                app.map_renderer.build_land_grid();
                                app.map_renderer.build_spatial_indexes();
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

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

// Supporting terminals display each diff atomically instead of painting it
// partway through transmission. Always close the frame, including on draw errors.
fn draw_frame(terminal: &mut DefaultTerminal, app: &mut App) -> Result<()> {
    execute!(terminal.backend_mut(), BeginSynchronizedUpdate)?;
    let drawn = terminal.draw(|frame| ui::render(frame, app)).map(|_| ());
    let ended = execute!(terminal.backend_mut(), EndSynchronizedUpdate);
    drawn?;
    ended?;
    Ok(())
}

const FRAME_INTERVAL: Duration = Duration::from_nanos(1_000_000_000 / 60);

fn advance_frame_deadline(previous: Instant, now: Instant) -> Instant {
    let next = previous + FRAME_INTERVAL;
    if next <= now {
        now + FRAME_INTERVAL
    } else {
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drawing_consumes_the_frame_budget() {
        let now = Instant::now();
        let next = advance_frame_deadline(now, now);
        let draw_time = Duration::from_millis(4);
        assert_eq!(
            next.duration_since(now + draw_time),
            FRAME_INTERVAL - draw_time
        );
    }

    #[test]
    fn delayed_frames_do_not_accumulate_catch_up_draws() {
        let previous = Instant::now();
        let now = previous + Duration::from_secs(1);
        assert_eq!(advance_frame_deadline(previous, now), now + FRAME_INTERVAL);
    }

    #[test]
    fn input_frequency_does_not_accelerate_animation() {
        let start = Instant::now();
        for event_interval in [Duration::from_micros(100), Duration::from_millis(1)] {
            let mut now = start;
            let mut next = start + FRAME_INTERVAL;
            let mut frames = 0;
            while now < start + Duration::from_secs(1) {
                now += event_interval.min(next.saturating_duration_since(now));
                if now >= next {
                    frames += 1;
                    next = advance_frame_deadline(next, now);
                }
            }
            assert_eq!(frames, 60);
        }
    }
}
