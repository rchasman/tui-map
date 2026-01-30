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
        frame: app.frame,
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
    frame: u64,
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

        // Render fires with chaotic flickering RGB gradients
        for fire in &self.fires {
            let x = area.x + fire.x;
            let y = area.y + fire.y;
            if x < area.x + area.width && y < area.y + area.height {
                // Chaotic flickering: combine position, intensity, and frame for randomness
                // Use xorshift-style hash to break spatial patterns
                let mut seed = (fire.x as u64) * 2654435761 + (fire.y as u64) * 2246822519 + self.frame;
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let flicker = ((seed & 0x3F) as i16) - 32;  // -32 to +31 range
                let visual_intensity = (fire.intensity as i16 + flicker).clamp(0, 255) as u8;

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

            // Explosion expands quickly then billows - multi-stage expansion
            let progress = if exp.frame < 20 {
                // Fast initial expansion (0-20 frames)
                (exp.frame as f32 / 20.0).powf(0.7)
            } else if exp.frame < 40 {
                // Billowing expansion (20-40 frames) - slower, fuller
                1.0 + ((exp.frame - 20) as f32 / 20.0) * 0.3
            } else {
                // Full mushroom cloud (40-60 frames)
                1.3
            };
            let max_r = exp.radius as f32 * progress;

            // Mushroom cap rises and expands UPWARD
            let cap_height = (max_r * (1.8 + (exp.frame as f32 / 60.0) * 0.8)) as i16;  // Extends very high
            let cap_width = max_r;

            // Animation phases - extended for dramatic impact
            let flash_phase = exp.frame < 8;      // Extended white-hot flash (8 frames)
            let fireball_phase = exp.frame < 25;  // Long bright fireball phase (17 frames)
            let cooling_phase = exp.frame < 45;   // Extended cooling/billowing (20 frames)
            // Final smoke phase: 45-60 frames

            // Draw mushroom cloud - ONLY UPWARD, nothing below cursor
            let radius_i16 = exp.radius as i16;
            for dy in -cap_height..0 {
                let py = (y as i16 + dy) as u16;

                // Skip entire row if off-screen
                if py < area.y || py >= area.y + area.height {
                    continue;
                }

                let dy_sq = dy * dy;

                for dx in -(radius_i16)..=(radius_i16) {
                    let dist_sq = (dx * dx + dy_sq) as f32;

                    // Turbulent mushroom cap with chaotic asymmetry
                    let height_ratio = (-dy as f32) / cap_height as f32;  // 0.0 at base, 1.0 at top

                    // Add turbulence - use position and time for chaotic variation
                    let mut turb_seed = (dx as u64).wrapping_mul(2654435761)
                                       + (dy as u64).wrapping_mul(2246822519)
                                       + (self.frame + exp.frame as u64).wrapping_mul(1103515245);
                    turb_seed ^= turb_seed << 13;
                    turb_seed ^= turb_seed >> 7;
                    turb_seed ^= turb_seed << 17;
                    let turbulence = ((turb_seed & 0xFF) as f32 / 255.0) * 0.3;  // 0-30% variation

                    // Height-based width with turbulence
                    let height_factor = if height_ratio < 0.25 {
                        // Rising column (0-25% height) - narrow base with turbulence
                        0.6 + height_ratio * 0.8 + turbulence
                    } else if height_ratio < 0.6 {
                        // Transition zone (25-60%) - expanding
                        1.0 + (height_ratio - 0.25) * 0.6 + turbulence
                    } else {
                        // Mushroom cap (60-100%) - massive roiling top
                        1.2 + (height_ratio - 0.6) * 1.5 + turbulence * 1.5  // Extra turbulence at top
                    };

                    let effective_width_sq = (cap_width * height_factor) * (cap_width * height_factor);
                    let in_cloud = dist_sq <= effective_width_sq;

                    if in_cloud {
                        let px = (x as i16 + dx) as u16;

                        // Bounds check x-axis only
                        if px < area.x || px >= area.x + area.width {
                            continue;
                        }

                        // Calculate heat - combines radial and vertical position
                        // Hottest at base (blast site), cooler as you rise and spread out
                        let radial_dist = dist_sq.sqrt() / (cap_width * height_factor);
                        let vertical_factor = (-dy as f32) / cap_height as f32;  // 0.0 at base, 1.0 at top

                        // Heat calculation: hottest at base center, cooler at edges and top
                        let dist_norm = (radial_dist * 0.5 + vertical_factor * 0.5).min(1.0);

                        // Chaotic flickering for explosion
                        let mut seed = (px as u64) * 2654435761 + (py as u64) * 2246822519 + (self.frame + exp.frame as u64) * 1103515245;
                        seed ^= seed << 13;
                        seed ^= seed >> 7;
                        seed ^= seed << 17;
                        let flicker = ((seed & 0xFF) as f32) / 255.0;

                        // RGB gradient with phase-based coloring
                        let (r, g, b, ch) = if flash_phase {
                            // Extended flash phase: blinding white hot core
                            if dist_norm < 0.4 {
                                // Intense white core with slight blue tint (hotter than white)
                                (255, 255, 255, '█')
                            } else if dist_norm < 0.7 {
                                // Brilliant white-yellow transition
                                (255, 250, 220, '█')
                            } else {
                                // Bright yellow outer flash
                                (255, 240, 150, '▓')
                            }
                        } else if fireball_phase {
                            // Extended fireball: dramatic center white → yellow → orange → red
                            let phase_progress = (exp.frame - 8) as f32 / 17.0;  // 0.0 to 1.0 over fireball phase

                            // Core stays white-hot longer then transitions
                            let core_threshold = 0.3 - (phase_progress * 0.15);

                            if dist_norm < core_threshold {
                                // White-hot core that slowly shrinks
                                (255, 255, 250, '█')
                            } else if dist_norm < 0.4 {
                                // Bright yellow - transitions to orange over time
                                let r = 255;
                                let g = (250.0 - phase_progress * 70.0) as u8;
                                let b = (120.0 - phase_progress * 100.0) as u8;
                                (r, g, b, '▓')
                            } else if dist_norm < 0.6 {
                                // Orange - gets redder over time
                                let r = 255;
                                let g = (180.0 - phase_progress * 100.0) as u8;
                                let b = (20.0 * (1.0 - phase_progress)) as u8;
                                (r, g, b, '▓')
                            } else if dist_norm < 0.8 {
                                // Red-orange outer region
                                (255, 80, 0, '▒')
                            } else {
                                // Dark red billowing smoke
                                (200, 40, 0, '░')
                            }
                        } else if cooling_phase {
                            // Extended cooling: billowing orange/red → dark smoke with radioactive core
                            let cooling_progress = (exp.frame - 25) as f32 / 20.0;  // 0.0 to 1.0

                            if dist_norm < 0.15 {
                                // Pulsing radioactive core
                                let pulse_cycle = (exp.frame / 3) % 2;
                                let pulse = if pulse_cycle == 0 { 60 } else { 20 };
                                (255, pulse, 30, '☢')
                            } else if dist_norm < 0.4 {
                                // Flickering orange that darkens over time
                                let base = 220.0 - (cooling_progress * 80.0);
                                let r = (base - flicker * 40.0) as u8;
                                let g = (60.0 - cooling_progress * 20.0) as u8;
                                (r, g, 0, '▓')
                            } else if dist_norm < 0.7 {
                                // Dark orange transitioning to brown
                                let r = (160.0 - cooling_progress * 50.0) as u8;
                                let g = (40.0 - cooling_progress * 20.0) as u8;
                                (r, g, 0, '▒')
                            } else {
                                // Brown smoke billowing outward
                                let r = (100.0 - cooling_progress * 20.0) as u8;
                                let g = (20.0 - cooling_progress * 10.0) as u8;
                                (r, g, 0, '░')
                            }
                        } else {
                            // Final phase: dark dissipating smoke
                            let final_progress = (exp.frame - 45) as f32 / 15.0;
                            let r = (80.0 - final_progress * 30.0) as u8;
                            let g = (15.0 - final_progress * 10.0) as u8;
                            let alpha = if dist_norm > 0.5 { '░' } else { '▒' };
                            (r, g, 0, alpha)
                        };

                        buf[(px, py)].set_char(ch).set_fg(Color::Rgb(r, g, b));
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
