use crate::app::App;
use crate::map::MapLayers;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
    Frame,
};

/// Render the UI
pub fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // Split into map area and status bar
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),    // Map
            Constraint::Length(1), // Status bar
        ])
        .split(area);

    render_map(frame, app, chunks[0]);
    render_status_bar(frame, app, chunks[1]);
}

fn render_map(frame: &mut Frame, app: &App, area: Rect) {
    // Create a block with border
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(Span::styled(
            " World Map ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Update viewport size for rendering
    let mut viewport = app.viewport.clone();
    // Braille gives 2x4 resolution per character
    viewport.width = inner.width as usize * 2;
    viewport.height = inner.height as usize * 4;

    // Render map layers
    let layers = app.map_renderer.render(inner.width as usize, inner.height as usize, &viewport);

    // Get mouse cursor position for marker
    let cursor_pos = app.mouse_pixel_pos().and_then(|(px, py)| {
        // Convert braille pixels to character position
        let cx = (px / 2) as u16;
        let cy = (py / 4) as u16;
        if cx < inner.width && cy < inner.height {
            Some((cx, cy))
        } else {
            None
        }
    });

    // Convert explosions to screen coordinates
    let explosions: Vec<ExplosionRender> = app.explosions.iter().filter_map(|exp| {
        let (px, py) = viewport.project(exp.lon, exp.lat);
        let cx = (px / 2) as u16;
        let cy = (py / 4) as u16;
        if cx < inner.width && cy < inner.height {
            // Convert radius_km to screen chars (rough: 1 degree ~= 111km at equator)
            let degrees = exp.radius_km / 111.0;
            let pixels = (degrees * viewport.zoom * inner.width as f64 / 360.0) as u16;
            let radius = (pixels / 2).max(3).min(15); // Clamp to reasonable range
            Some(ExplosionRender { x: cx, y: cy, frame: exp.frame, radius })
        } else {
            None
        }
    }).collect();

    // Convert fires to screen coordinates
    let fires: Vec<FireRender> = app.fires.iter().filter_map(|fire| {
        let (px, py) = viewport.project(fire.lon, fire.lat);
        let cx = (px / 2) as u16;
        let cy = (py / 4) as u16;
        if cx < inner.width && cy < inner.height && px >= 0 && py >= 0 {
            Some(FireRender { x: cx, y: cy, intensity: fire.intensity })
        } else {
            None
        }
    }).collect();

    // Render braille map
    let map_widget = MapWidget {
        layers,
        cursor_pos,
        explosions,
        fires,
        has_states: app.map_renderer.settings.show_states,
        zoom: viewport.zoom,
        inner_width: inner.width,
        inner_height: inner.height,
    };
    frame.render_widget(map_widget, inner);
}

/// An explosion to render
struct ExplosionRender {
    x: u16,
    y: u16,
    frame: u8,
    radius: u16, // Visual radius in chars
}

/// A fire to render
struct FireRender {
    x: u16,
    y: u16,
    intensity: u8,
}

/// Custom widget that renders braille map with text labels overlaid
struct MapWidget {
    layers: MapLayers,
    cursor_pos: Option<(u16, u16)>,
    explosions: Vec<ExplosionRender>,
    fires: Vec<FireRender>,
    has_states: bool,
    zoom: f64,
    inner_width: u16,
    inner_height: u16,
}

impl MapWidget {
    /// Render a braille canvas layer with a specific color
    fn render_layer(&self, canvas: &crate::braille::BrailleCanvas, color: Color, area: Rect, buf: &mut Buffer) {
        for (row_idx, row_str) in canvas.rows().enumerate() {
            if row_idx >= area.height as usize {
                break;
            }
            let y = area.y + row_idx as u16;

            for (col_idx, ch) in row_str.chars().enumerate() {
                if col_idx >= area.width as usize {
                    break;
                }
                // Skip empty braille characters (U+2800)
                if ch == '\u{2800}' {
                    continue;
                }
                let x = area.x + col_idx as u16;
                buf[(x, y)].set_char(ch).set_fg(color);
            }
        }
    }
}

impl Widget for MapWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Render layers from back to front:
        // 1. Coastlines (Cyan - at back)
        self.render_layer(&self.layers.coastlines, Color::Cyan, area, buf);

        // 2. County borders (DarkGray)
        self.render_layer(&self.layers.counties, Color::DarkGray, area, buf);

        // 3. Country borders (Yellow if states visible at this zoom, else Cyan)
        let states_visible = self.has_states && self.zoom >= 4.0;
        let border_color = if states_visible { Color::Yellow } else { Color::Cyan };
        self.render_layer(&self.layers.borders, border_color, area, buf);

        // 4. State borders (Yellow - on top)
        self.render_layer(&self.layers.states, Color::Yellow, area, buf);

        // Then overlay city markers and labels
        let marker_style = Style::default().fg(Color::White);
        let label_style = Style::default().fg(Color::White);
        let dead_marker_style = Style::default().fg(Color::DarkGray);
        let dead_label_style = Style::default().fg(Color::DarkGray).add_modifier(Modifier::CROSSED_OUT);

        for (lx, ly, text) in &self.layers.labels {
            // Check bounds
            if *ly >= self.inner_height || *lx >= self.inner_width {
                continue;
            }

            let x = area.x + *lx;
            let y = area.y + *ly;

            // Check for dead city (~ prefix) or skull marker
            let is_dead = text.starts_with('~') || text.starts_with('☠');
            let display_text_raw = if text.starts_with('~') { &text[1..] } else { text.as_str() };

            // Check if this is a marker glyph (single char) or a label
            let is_marker = text.len() <= 3 && matches!(text.chars().next(), Some('⚜' | '★' | '◆' | '■' | '●' | '○' | '◦' | '·' | '☠'));
            let style = match (is_marker, is_dead) {
                (true, true) => dead_marker_style,
                (true, false) => marker_style,
                (false, true) => dead_label_style,
                (false, false) => label_style,
            };

            // Truncate label to fit screen, allow longer labels for population
            let max_len = (self.inner_width.saturating_sub(*lx)) as usize;
            let max_display = if is_marker { 1 } else { 24 };
            let display_text: String = display_text_raw.chars().take(max_len.min(max_display)).collect();

            for (i, ch) in display_text.chars().enumerate() {
                let px = x + i as u16;
                if px < area.x + area.width {
                    buf[(px, y)].set_char(ch).set_style(style);
                }
            }
        }

        // Render fires
        for fire in &self.fires {
            let x = area.x + fire.x;
            let y = area.y + fire.y;
            if x < area.x + area.width && y < area.y + area.height {
                let ch = if fire.intensity > 150 { '▓' } else if fire.intensity > 75 { '▒' } else { '░' };
                let color = if fire.intensity > 100 { Color::Yellow } else { Color::Red };
                buf[(x, y)].set_char(ch).set_fg(color);
            }
        }

        // Render explosions
        for exp in &self.explosions {
            let x = area.x + exp.x;
            let y = area.y + exp.y;

            // Explosion expands based on frame, up to actual blast radius
            let progress = (exp.frame as f32 / 15.0).min(1.0);
            let max_r = exp.radius as f32 * progress;

            // Draw mushroom cloud shape - wider at top, stem below
            for dy in -(exp.radius as i16 + 2)..=(exp.radius as i16) {
                for dx in -(exp.radius as i16)..=(exp.radius as i16) {
                    // Euclidean distance for circular shape
                    let dist = ((dx * dx + dy * dy) as f32).sqrt();

                    // Mushroom cap (top half, wider)
                    let in_cap = dy <= 0 && dist <= max_r;
                    // Stem (bottom, narrower)
                    let in_stem = dy > 0 && dy <= (max_r * 0.6) as i16 && dx.abs() <= (max_r * 0.3) as i16;

                    if in_cap || in_stem {
                        let px = (x as i16 + dx) as u16;
                        let py = (y as i16 + dy) as u16;
                        if px >= area.x && px < area.x + area.width &&
                           py >= area.y && py < area.y + area.height {
                            // Color based on distance from center and frame
                            let (ch, color) = if dist < max_r * 0.3 {
                                if exp.frame < 8 { ('*', Color::White) } else { ('☢', Color::Red) }
                            } else if dist < max_r * 0.6 {
                                ('█', Color::Yellow)
                            } else {
                                ('░', Color::Red)
                            };
                            buf[(px, py)].set_char(ch).set_fg(color);
                        }
                    }
                }
            }
        }

        // Render cursor marker
        if let Some((cx, cy)) = self.cursor_pos {
            let x = area.x + cx;
            let y = area.y + cy;
            if x < area.x + area.width && y < area.y + area.height {
                buf[(x, y)].set_char('╋').set_fg(Color::Red);
            }
        }
    }
}

fn render_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let settings = &app.map_renderer.settings;

    let status = Line::from(vec![
        Span::styled(" Zoom: ", Style::default().fg(Color::DarkGray)),
        Span::styled(app.zoom_level(), Style::default().fg(Color::Yellow)),
        Span::styled(" (", Style::default().fg(Color::DarkGray)),
        Span::styled(app.lod_level(), Style::default().fg(Color::Magenta)),
        Span::styled(") ", Style::default().fg(Color::DarkGray)),
        // Toggle indicators
        Span::styled(
            if settings.show_borders { "[B]order " } else { "[b]order " },
            Style::default().fg(if settings.show_borders { Color::Green } else { Color::DarkGray }),
        ),
        Span::styled(
            if settings.show_states { "[S]tate " } else { "[s]tate " },
            Style::default().fg(if settings.show_states { Color::Green } else { Color::DarkGray }),
        ),
        Span::styled(
            if settings.show_counties { "[Y]county " } else { "[y]county " },
            Style::default().fg(if settings.show_counties { Color::Green } else { Color::DarkGray }),
        ),
        Span::styled(
            if settings.show_cities { "[C]ities " } else { "[c]ities " },
            Style::default().fg(if settings.show_cities { Color::Green } else { Color::DarkGray }),
        ),
        Span::styled(
            if settings.show_labels { "[L]abels " } else { "[l]abels " },
            Style::default().fg(if settings.show_labels { Color::Green } else { Color::DarkGray }),
        ),
        Span::styled(
            if settings.show_population { "[P]op " } else { "[p]op " },
            Style::default().fg(if settings.show_population { Color::Green } else { Color::DarkGray }),
        ),
        Span::styled("| ", Style::default().fg(Color::DarkGray)),
        Span::styled(app.center_coords(), Style::default().fg(Color::Cyan)),
        if app.casualties > 0 {
            Span::styled(
                format!(" | CASUALTIES: {}", format_casualties(app.casualties)),
                Style::default().fg(Color::Red),
            )
        } else {
            Span::styled(
                " | SPACE to nuke",
                Style::default().fg(Color::DarkGray),
            )
        },
    ]);

    let paragraph = Paragraph::new(status);
    frame.render_widget(paragraph, area);
}

/// Format casualties with suffix (K, M, B)
fn format_casualties(n: u64) -> String {
    if n >= 1_000_000_000 {
        format!("{:.1}B", n as f64 / 1_000_000_000.0)
    } else if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.0}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
