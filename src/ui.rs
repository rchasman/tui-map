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

    // Convert explosions to screen coordinates with aggressive culling
    // Note: Damage calculations still run in app.update_explosions() regardless of viewport
    // This only culls *rendering* to avoid expensive O(r²) nested loops for off-screen effects
    let mut explosions: Vec<ExplosionRender> = Vec::new();
    for exp in &app.explosions {
        // Try normal position and wrapped positions
        for &offset in &[0.0, -360.0, 360.0] {
            let ((px, py), _) = viewport.project_wrapped(exp.lon, exp.lat, offset);

            // Early rejection: negative coords mean off-screen left/top
            if px < 0 || py < 0 {
                continue;
            }

            let cx = (px / 2) as u16;
            let cy = (py / 4) as u16;

            // Convert radius_km to screen chars (rough: 1 degree ~= 111km at equator)
            let degrees = exp.radius_km / 111.0;
            let pixels = (degrees * viewport.zoom * inner.width as f64 / 360.0) as u16;
            let radius = (pixels / 2).max(3).min(15); // Clamp to reasonable range

            // Cull if too small to see when zoomed out (< 2 chars radius)
            if radius < 2 {
                continue;
            }

            // Cull if entirely off-screen (center + radius outside bounds)
            if cx >= inner.width + radius || cy >= inner.height + radius {
                continue;
            }

            // Cull if center too far off-screen (even if edge might be visible)
            if cx < inner.width && cy < inner.height {
                explosions.push(ExplosionRender { x: cx, y: cy, frame: exp.frame, radius });
            }
        }
    }

    // Limit max visible explosions (sort by radius descending, show biggest)
    const MAX_VISIBLE_EXPLOSIONS: usize = 50;
    if explosions.len() > MAX_VISIBLE_EXPLOSIONS {
        explosions.sort_by_key(|e| std::cmp::Reverse(e.radius));
        explosions.truncate(MAX_VISIBLE_EXPLOSIONS);
    }

    // Convert fires to screen coordinates with culling and wrapping
    let mut fires: Vec<FireRender> = Vec::new();
    for fire in &app.fires {
        // Cull very faint fires (intensity < 20 is barely visible)
        if fire.intensity < 20 {
            continue;
        }

        // Try normal position and wrapped positions
        for &offset in &[0.0, -360.0, 360.0] {
            let ((px, py), _) = viewport.project_wrapped(fire.lon, fire.lat, offset);

            // Early rejection: negative coords or out of bounds
            if px < 0 || py < 0 {
                continue;
            }

            let cx = (px / 2) as u16;
            let cy = (py / 4) as u16;

            if cx < inner.width && cy < inner.height {
                fires.push(FireRender { x: cx, y: cy, intensity: fire.intensity });
            }
        }
    }

    // Limit max visible fires (keep only the most intense)
    const MAX_VISIBLE_FIRES: usize = 200;
    if fires.len() > MAX_VISIBLE_FIRES {
        fires.sort_by_key(|f| std::cmp::Reverse(f.intensity));
        fires.truncate(MAX_VISIBLE_FIRES);
    }

    // Convert debris to screen coordinates with culling and wrapping
    let mut debris_particles: Vec<DebrisRender> = Vec::new();
    for particle in &app.debris {
        // Try normal position and wrapped positions
        for &offset in &[0.0, -360.0, 360.0] {
            let ((px, py), _) = viewport.project_wrapped(particle.lon, particle.lat, offset);

            // Early rejection: negative coords or out of bounds
            if px < 0 || py < 0 {
                continue;
            }

            let cx = (px / 2) as u16;
            let cy = (py / 4) as u16;

            if cx < inner.width && cy < inner.height {
                debris_particles.push(DebrisRender { x: cx, y: cy, life: particle.life });
            }
        }
    }

    // Limit debris
    const MAX_VISIBLE_DEBRIS: usize = 300;
    if debris_particles.len() > MAX_VISIBLE_DEBRIS {
        debris_particles.truncate(MAX_VISIBLE_DEBRIS);
    }

    // Render braille map
    let map_widget = MapWidget {
        layers,
        cursor_pos,
        explosions,
        fires,
        debris: debris_particles,
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

/// A debris particle to render
struct DebrisRender {
    x: u16,
    y: u16,
    life: u8,
}

/// Custom widget that renders braille map with text labels overlaid
struct MapWidget {
    layers: MapLayers,
    cursor_pos: Option<(u16, u16)>,
    explosions: Vec<ExplosionRender>,
    fires: Vec<FireRender>,
    debris: Vec<DebrisRender>,
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
        // 1. County borders (DarkGray - at back)
        self.render_layer(&self.layers.counties, Color::DarkGray, area, buf);

        // 2. Country borders (Yellow if states visible at this zoom, else Cyan)
        let states_visible = self.has_states && self.zoom >= 4.0;
        let border_color = if states_visible { Color::Yellow } else { Color::Cyan };
        self.render_layer(&self.layers.borders, border_color, area, buf);

        // 3. State borders (Yellow)
        self.render_layer(&self.layers.states, Color::Yellow, area, buf);

        // 4. Coastlines (Cyan - on top of borders)
        self.render_layer(&self.layers.coastlines, Color::Cyan, area, buf);

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

        // Render debris particles
        for particle in &self.debris {
            let x = area.x + particle.x;
            let y = area.y + particle.y;
            if x < area.x + area.width && y < area.y + area.height {
                // Debris fades as it loses life
                let fade = (particle.life as f32 / 50.0).min(1.0);
                let r = (180.0 * fade) as u8;
                let g = (160.0 * fade) as u8;
                let b = (140.0 * fade) as u8;

                // Different characters based on remaining life
                let ch = if particle.life > 40 {
                    '▪'  // Solid block
                } else if particle.life > 25 {
                    '•'  // Bullet
                } else if particle.life > 10 {
                    '·'  // Small dot
                } else {
                    ','  // Tiny speck
                };

                buf[(x, y)].set_char(ch).set_fg(Color::Rgb(r, g, b));
            }
        }

        // Render fires with flickering RGB gradients
        for fire in &self.fires {
            let x = area.x + fire.x;
            let y = area.y + fire.y;
            if x < area.x + area.width && y < area.y + area.height {
                // Flickering: add pseudo-random variation to intensity
                let flicker = ((fire.x as u32 * 97 + fire.y as u32 * 31) % 20) as u8;
                let visual_intensity = fire.intensity.saturating_add(flicker).saturating_sub(10);

                // RGB fire gradient: white (hottest) → yellow → orange → red → dark red
                let (r, g, b, ch) = if visual_intensity > 220 {
                    // White hot core
                    (255, 255, 200, '█')
                } else if visual_intensity > 180 {
                    // Bright yellow flames
                    (255, 220, 0, '▓')
                } else if visual_intensity > 140 {
                    // Orange flames
                    (255, 140, 0, '▓')
                } else if visual_intensity > 100 {
                    // Orange-red
                    (255, 80, 0, '▒')
                } else if visual_intensity > 60 {
                    // Red
                    (220, 20, 0, '▒')
                } else if visual_intensity > 30 {
                    // Dark red embers
                    (160, 10, 0, '░')
                } else {
                    // Dying embers
                    (100, 5, 0, '·')
                };

                buf[(x, y)].set_char(ch).set_fg(Color::Rgb(r, g, b));
            }
        }

        // Render explosions with RGB gradient mushroom cloud and shockwave
        for exp in &self.explosions {
            let x = area.x + exp.x;
            let y = area.y + exp.y;

            // Explosion expands based on frame, up to actual blast radius
            let progress = (exp.frame as f32 / 15.0).min(1.0);
            let max_r = exp.radius as f32 * progress;
            let max_r_sq = max_r * max_r;

            // Precompute stem boundaries
            let stem_height = (max_r * 0.6) as i16;
            let stem_width = (max_r * 0.3) as i16;

            // Animation phases
            let flash_phase = exp.frame < 3;     // Initial white flash
            let fireball_phase = exp.frame < 8;  // Bright fireball
            let cooling_phase = exp.frame < 15;  // Cooling smoke

            // Draw mushroom cloud shape
            let radius_i16 = exp.radius as i16;
            for dy in -(radius_i16 + 2)..=(radius_i16) {
                let py = (y as i16 + dy) as u16;

                // Skip entire row if off-screen
                if py < area.y || py >= area.y + area.height {
                    continue;
                }

                let dy_sq = dy * dy;

                for dx in -(radius_i16)..=(radius_i16) {
                    let dist_sq = (dx * dx + dy_sq) as f32;

                    // Mushroom cap (top half, wider)
                    let in_cap = dy <= 0 && dist_sq <= max_r_sq;
                    // Stem (bottom, narrower)
                    let in_stem = dy > 0 && dy <= stem_height && dx.abs() <= stem_width;

                    if in_cap || in_stem {
                        let px = (x as i16 + dx) as u16;

                        // Bounds check x-axis only
                        if px < area.x || px >= area.x + area.width {
                            continue;
                        }

                        // Calculate normalized distance from center (0.0 = center, 1.0 = edge)
                        let dist_norm = if in_stem {
                            (dx.abs() as f32 / stem_width.max(1) as f32).min(1.0)
                        } else {
                            (dist_sq.sqrt() / max_r).min(1.0)
                        };

                        // Flickering based on position (deterministic)
                        let flicker = ((px as u32 * 97 + py as u32 * 31 + exp.frame as u32 * 13) % 20) as f32 / 20.0;

                        // RGB gradient with phase-based coloring
                        let (r, g, b, ch) = if flash_phase {
                            // Initial flash: white hot
                            if dist_norm < 0.5 {
                                (255, 255, 255, '█')
                            } else {
                                (255, 240, 200, '▓')
                            }
                        } else if fireball_phase {
                            // Fireball: center white → yellow → orange → red
                            if dist_norm < 0.2 {
                                (255, 255, 240, '█')  // White core
                            } else if dist_norm < 0.4 {
                                (255, 250, 100, '▓')  // Bright yellow
                            } else if dist_norm < 0.6 {
                                (255, 180, 20, '▓')   // Orange
                            } else if dist_norm < 0.8 {
                                (255, 80, 0, '▒')     // Red-orange
                            } else {
                                (200, 40, 0, '░')     // Dark red smoke
                            }
                        } else if cooling_phase {
                            // Cooling: orange/red → dark smoke with radioactive core
                            if dist_norm < 0.15 {
                                // Radioactive core
                                let pulse = if exp.frame % 2 == 0 { 30 } else { 0 };
                                (255, pulse, 30, '☢')
                            } else if dist_norm < 0.4 {
                                (220 - (flicker * 40.0) as u8, 60, 0, '▓')  // Flickering orange
                            } else if dist_norm < 0.7 {
                                (160, 40, 0, '▒')     // Dark orange
                            } else {
                                (100, 20, 0, '░')     // Brown smoke
                            }
                        } else {
                            // Final phase: dark smoke
                            (80, 15, 0, '░')
                        };

                        buf[(px, py)].set_char(ch).set_fg(Color::Rgb(r, g, b));
                    }
                }
            }

            // Add expanding shockwave ring (first few frames only)
            if exp.frame < 6 {
                let ring_radius = (exp.frame as f32 * max_r / 5.0) as i16;

                for angle_step in 0..24 {
                    let angle = (angle_step as f32 / 24.0) * std::f32::consts::TAU;
                    let ring_x = (x as i16 + (ring_radius as f32 * angle.cos()) as i16) as u16;
                    let ring_y = (y as i16 + (ring_radius as f32 * angle.sin()) as i16) as u16;

                    if ring_x >= area.x && ring_x < area.x + area.width &&
                       ring_y >= area.y && ring_y < area.y + area.height {
                        // Shockwave: bright yellow-white
                        let fade = 1.0 - (exp.frame as f32 / 6.0);
                        let r = (255.0 * fade) as u8;
                        let g = (240.0 * fade) as u8;
                        let b = (200.0 * fade) as u8;
                        buf[(ring_x, ring_y)].set_char('○').set_fg(Color::Rgb(r, g, b));
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
