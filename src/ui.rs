use crate::app::App;
use crate::hash::{hash2, hash3};
use crate::map::{MapLayers, Projection, WRAP_OFFSETS};
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

    // Update projection size for rendering
    let mut projection = app.projection.clone();
    // Braille gives 2x4 resolution per character
    projection.set_size(inner.width as usize * 2, inner.height as usize * 4);

    // Render map layers
    let layers = app.map_renderer.render(inner.width as usize, inner.height as usize, &projection);

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
    let mut explosions: Vec<ExplosionRender> = Vec::with_capacity(50);
    let is_globe = matches!(projection, Projection::Globe(_));
    for exp in &app.explosions {
        // Globe: single project call (no wrapping needed)
        // Mercator: try wrap offsets
        let screen_positions: Vec<(i32, i32)> = if is_globe {
            projection.project_point(exp.lon, exp.lat).into_iter().collect()
        } else {
            if let Projection::Mercator(ref vp) = projection {
                WRAP_OFFSETS.iter().filter_map(|&offset| {
                    let ((px, py), _) = vp.project_wrapped(exp.lon, exp.lat, offset);
                    (px >= 0 && py >= 0 && px <= 30000 && py <= 30000).then_some((px, py))
                }).collect()
            } else {
                Vec::new()
            }
        };

        for (px, py) in screen_positions {
            let cx = (px / 2) as u16;
            let cy = (py / 4) as u16;

            let degrees = exp.radius_km / 111.0;
            let pixels = projection.deg_to_pixels(degrees) as u16;
            let radius = (pixels / 2).max(3);

            if radius < 2 {
                continue;
            }

            let left_edge = cx.saturating_sub(radius);
            let top_edge = cy.saturating_sub(radius);
            let right_edge = cx.saturating_add(radius);
            let bottom_edge = cy.saturating_add(radius);

            if right_edge < 1 || bottom_edge < 1 || left_edge >= inner.width || top_edge >= inner.height {
                continue;
            }

            explosions.push(ExplosionRender { x: cx, y: cy, frame: exp.frame, radius });
        }
    }

    // Limit max visible explosions (sort by radius descending, show biggest)
    const MAX_VISIBLE_EXPLOSIONS: usize = 50;
    if explosions.len() > MAX_VISIBLE_EXPLOSIONS {
        explosions.sort_by_key(|e| std::cmp::Reverse(e.radius));
        explosions.truncate(MAX_VISIBLE_EXPLOSIONS);
    }

    // Screen-space fire map: merge overlapping fires by tracking max intensity per cell
    // This is O(1) lookup and avoids duplicate rendering
    let fire_map_width = inner.width as usize;
    let fire_map_height = inner.height as usize;
    let mut fire_map: Vec<u8> = vec![0; fire_map_width * fire_map_height];

    // Helper to merge fire into map (max intensity wins)
    let mut add_fire = |cx: usize, cy: usize, intensity: u8| {
        if cx < fire_map_width && cy < fire_map_height {
            let idx = cy * fire_map_width + cx;
            fire_map[idx] = fire_map[idx].max(intensity);
        }
    };

    // Compute viewport bounds for fire culling
    let zoom = projection.effective_zoom();
    let (vp_min_lon, vp_min_lat, vp_max_lon, vp_max_lat) = if is_globe {
        if let Projection::Globe(ref g) = projection {
            let bounds = g.visible_bounds();
            // Add padding for fire rendering
            ((bounds.0 - 5.0).max(-180.0), (bounds.1 - 5.0).max(-90.0),
             (bounds.2 + 5.0).min(180.0), (bounds.3 + 5.0).min(90.0))
        } else {
            unreachable!()
        }
    } else {
        if let Projection::Mercator(ref vp) = projection {
            let half_width_deg = 180.0 / vp.zoom;
            let min_lon = vp.center_lon - half_width_deg * 1.5;
            let max_lon = vp.center_lon + half_width_deg * 1.5;
            let (_, top_lat) = vp.unproject(0, 0);
            let (_, bottom_lat) = vp.unproject(0, vp.height as i32);
            let lat_pad = (top_lat - bottom_lat).abs() * 0.25;
            ((min_lon), (bottom_lat - lat_pad).max(-90.0),
             (max_lon), (top_lat + lat_pad).min(90.0))
        } else {
            unreachable!()
        }
    };

    // Hierarchical fire rendering based on zoom
    let deg_per_char = 360.0 / (zoom * inner.width as f64);

    {
        let grid = if deg_per_char >= 1.0 { &app.fire_grid } else { &app.fire_grid_fine };
        let mut fires_data = grid.fires_in_region(
            vp_min_lon.max(-180.0), vp_min_lat, vp_max_lon.min(180.0), vp_max_lat,
        );
        if !is_globe {
            if vp_min_lon < -180.0 {
                fires_data.extend(grid.fires_in_region(vp_min_lon + 360.0, vp_min_lat, 180.0, vp_max_lat));
            }
            if vp_max_lon > 180.0 {
                fires_data.extend(grid.fires_in_region(-180.0, vp_min_lat, vp_max_lon - 360.0, vp_max_lat));
            }
        }

        for (lon, lat, intensity) in fires_data {
            if let Some((px, py)) = projection.project_point(lon, lat) {
                add_fire((px / 2) as usize, (py / 4) as usize, intensity);
            }
        }
    }

    // Convert fire map to FireRender vec (only non-zero cells)
    let fires: Vec<FireRender> = fire_map
        .iter()
        .enumerate()
        .filter_map(|(idx, &intensity)| {
            if intensity > 0 {
                let x = (idx % fire_map_width) as u16;
                let y = (idx / fire_map_width) as u16;
                Some(FireRender { x, y, intensity })
            } else {
                None
            }
        })
        .collect();

    // Calculate blast radius for cursor reticle
    let cursor_blast_radius = if cursor_pos.is_some() {
        let radius_km = 50.0 + 700.0 / zoom;
        let degrees = radius_km / 111.0;
        let pixels = projection.deg_to_pixels(degrees) as u16;
        Some((pixels / 2).max(3))
    } else {
        None
    };

    // Render braille map
    let map_widget = MapWidget {
        layers,
        cursor_pos,
        cursor_blast_radius,
        explosions,
        fires,
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
#[derive(Clone, Copy)]
struct FireRender {
    x: u16,
    y: u16,
    intensity: u8,
}

/// Custom widget that renders braille map with text labels overlaid
struct MapWidget {
    layers: MapLayers,
    cursor_pos: Option<(u16, u16)>,
    cursor_blast_radius: Option<u16>,
    explosions: Vec<ExplosionRender>,
    fires: Vec<FireRender>,
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

        // 2. State borders (Yellow)
        self.render_layer(&self.layers.states, Color::Yellow, area, buf);

        // 3. Coastlines (Cyan)
        self.render_layer(&self.layers.coastlines, Color::Cyan, area, buf);

        // 4. Country borders (Cyan - on top so always visible above states)
        self.render_layer(&self.layers.borders, Color::Cyan, area, buf);

        // Render fires - simple density-based blocks, color does the work
        for fire in &self.fires {
            let x = area.x + fire.x;
            let y = area.y + fire.y;
            if x < area.x + area.width && y < area.y + area.height {
                // Subtle flicker in intensity only
                let seed = hash3(fire.x as u64, fire.y as u64, self.frame);
                let flicker = ((seed & 0x1F) as i16) - 16;  // -16 to +15 range (subtle)
                let vi = (fire.intensity as i16 + flicker).clamp(0, 255) as u8;

                // Density blocks + color gradient - let color convey heat
                let (r, g, b, ch) = if vi > 220 {
                    (255, 255, 240, '█')  // White-hot
                } else if vi > 180 {
                    (255, 240, 100, '█')  // Bright yellow
                } else if vi > 140 {
                    (255, 180, 30, '▓')   // Yellow-orange
                } else if vi > 100 {
                    (255, 120, 0, '▓')    // Orange
                } else if vi > 60 {
                    (255, 60, 0, '▒')     // Orange-red
                } else if vi > 30 {
                    (200, 30, 0, '▒')     // Red
                } else if vi > 15 {
                    (140, 20, 0, '░')     // Dark red
                } else {
                    (90, 10, 0, '░')      // Embers
                };

                buf[(x, y)].set_char(ch).set_fg(Color::Rgb(r, g, b));
            }
        }

        // City markers and labels — rendered ON TOP of fires so population
        // damage is visible through the flames
        for (lx, ly, text, health) in &self.layers.labels {
            if *ly >= self.inner_height || *lx >= self.inner_width {
                continue;
            }

            let x = area.x + *lx;
            let y = area.y + *ly;

            let is_dead = *health == 0.0;
            let display_text_raw = text.as_str();

            let is_marker = text.len() <= 3 && matches!(text.chars().next(), Some('⚜' | '★' | '◆' | '■' | '●' | '○' | '◦' | '·' | '☠'));

            // Style dims with damage: White at full health → DarkGray at death
            // bg(Reset) makes spaces opaque over fires
            let style = if is_dead {
                if is_marker {
                    Style::default().fg(Color::DarkGray).bg(Color::Reset)
                } else {
                    Style::default().fg(Color::DarkGray).bg(Color::Reset).add_modifier(Modifier::CROSSED_OUT)
                }
            } else {
                let brightness = (health * 200.0 + 55.0) as u8; // 55..255
                Style::default().fg(Color::Rgb(brightness, brightness, brightness)).bg(Color::Reset)
            };

            // Label truncates as population diminishes (markers always 1 char)
            let max_len = (self.inner_width.saturating_sub(*lx)) as usize;
            let max_display = if is_marker {
                1
            } else {
                // 8 chars at near-death, 24 at full health
                (8.0 + 16.0 * health) as usize
            };
            let display_text: String = display_text_raw.chars().take(max_len.min(max_display)).collect();

            for (i, ch) in display_text.chars().enumerate() {
                let px = x + i as u16;
                if px < area.x + area.width {
                    buf[(px, y)].set_char(ch).set_style(style);
                }
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

            // Mushroom cap rises and expands UPWARD with massive roiling top
            let cap_height = (max_r * (2.0 + (exp.frame as f32 / 60.0) * 1.2)) as i16;  // Extends VERY high
            let cap_width = max_r;

            // Animation phases - extended for dramatic impact
            let flash_phase = exp.frame < 8;      // Extended white-hot flash (8 frames)
            let fireball_phase = exp.frame < 25;  // Long bright fireball phase (17 frames)
            let cooling_phase = exp.frame < 45;   // Extended cooling/billowing (20 frames)
            // Final smoke phase: 45-60 frames

            // Draw mushroom cloud - ONLY UPWARD, nothing below cursor
            let radius_i16 = exp.radius as i16;
            let cap_height_f32 = cap_height as f32;

            // Precompute frame-based seed components (hoist out of inner loop)
            let frame_seed_component = self.frame + exp.frame as u64;

            for dy in -cap_height..0 {
                // Safe signed addition with bounds check BEFORE casting to u16
                let py_signed = (y as i16) + dy;
                if py_signed < 0 || py_signed >= (area.y + area.height) as i16 {
                    continue; // Skip if off-screen (handles negative overflow)
                }
                let py = py_signed as u16;

                let dy_sq = dy * dy;
                let dy_f32 = dy as f32;
                let height_ratio = -dy_f32 / cap_height_f32;

                // Precompute height tier (hoist branching out of x loop)
                let (base_width, height_mult, large_mult, fine_mult) = if height_ratio < 0.2 {
                    (0.5, 0.4, 0.0, 0.5)  // Rising column
                } else if height_ratio < 0.5 {
                    (0.9, 1.5, 0.7, 0.3)  // Transition
                } else if height_ratio < 0.75 {
                    (1.4, 2.0, 1.2, 0.4)  // Lower cap
                } else {
                    (1.9, 2.5, 2.0, 0.8)  // Roiling top
                };

                let height_component = if height_ratio < 0.2 {
                    height_ratio * height_mult
                } else if height_ratio < 0.5 {
                    (height_ratio - 0.2) * height_mult
                } else if height_ratio < 0.75 {
                    (height_ratio - 0.5) * height_mult
                } else {
                    (height_ratio - 0.75) * height_mult
                };

                for dx in -(radius_i16)..=(radius_i16) {
                    let dist_sq = (dx * dx + dy_sq) as f32;

                    // Multi-scale turbulence (compute both at once)
                    let dx_f32 = dx as f32;
                    let angle = dx_f32.atan2(dy_f32);
                    let large_turb_seed = hash2((angle * 1000.0) as u64, self.frame / 5);
                    let large_turbulence = ((large_turb_seed & 0xFF) as f32 / 255.0 - 0.5) * 0.6;

                    let fine_turb_seed = hash3(dx as u64, dy as u64, frame_seed_component);
                    let fine_turbulence = ((fine_turb_seed & 0xFF) as f32 / 255.0 - 0.5) * 0.4;

                    // Height-based width calculation (branchless tier system)
                    let height_factor = base_width + height_component +
                                       large_turbulence * large_mult +
                                       fine_turbulence * fine_mult;

                    let effective_width_sq = (cap_width * height_factor) * (cap_width * height_factor);
                    let in_cloud = dist_sq <= effective_width_sq;

                    if in_cloud {
                        // Safe signed addition with bounds check BEFORE casting to u16
                        let px_signed = (x as i16) + dx;
                        if px_signed < 0 || px_signed >= (area.x + area.width) as i16 {
                            continue; // Skip if off-screen (handles negative overflow)
                        }
                        let px = px_signed as u16;

                        // Calculate heat - combines radial and vertical position
                        // Hottest at base (blast site), cooler as you rise and spread out
                        let radial_dist = dist_sq.sqrt() / (cap_width * height_factor);
                        let vertical_factor = (-dy as f32) / cap_height as f32;  // 0.0 at base, 1.0 at top

                        // Heat calculation: hottest at base center, cooler at edges and top
                        let dist_norm = (radial_dist * 0.5 + vertical_factor * 0.5).min(1.0);

                        // Chaotic flickering for explosion
                        let seed = hash3(px as u64, py as u64, self.frame + exp.frame as u64);
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

        // Render cursor blast radius circle
        if let Some((cx, cy)) = self.cursor_pos {
            if let Some(radius) = self.cursor_blast_radius {
                let center_x = area.x as i32 + cx as i32;
                let center_y = area.y as i32 + cy as i32;
                let r = radius as i32;

                // Compute bounds for circle drawing (optimize by not checking every point)
                let min_x = (center_x - r).max(area.x as i32);
                let max_x = (center_x + r).min((area.x + area.width) as i32 - 1);
                let min_y = (center_y - r).max(area.y as i32);
                let max_y = (center_y + r).min((area.y + area.height) as i32 - 1);

                let r_sq = r * r;
                let inner_r_sq = (r - 1).max(0) * (r - 1).max(0);

                // Draw circle outline using distance check
                for y in min_y..=max_y {
                    let dy = y - center_y;
                    let dy_sq = dy * dy;

                    for x in min_x..=max_x {
                        let dx = x - center_x;
                        let dist_sq = dx * dx + dy_sq;

                        // Only render points near the circle perimeter (not filled)
                        if dist_sq >= inner_r_sq && dist_sq <= r_sq {
                            buf[(x as u16, y as u16)]
                                .set_char('·')
                                .set_fg(Color::Red);
                        }
                    }
                }

                // Draw center crosshair
                if center_x >= area.x as i32 && center_x < (area.x + area.width) as i32 &&
                   center_y >= area.y as i32 && center_y < (area.y + area.height) as i32 {
                    buf[(center_x as u16, center_y as u16)]
                        .set_char('✕')
                        .set_fg(Color::Red);
                }
            }
        }
    }
}

fn render_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let settings = &app.map_renderer.settings;

    let status = Line::from(vec![
        Span::styled(
            if app.is_globe() { "[G]lobe " } else { "[M]ap " },
            Style::default().fg(if app.is_globe() { Color::Magenta } else { Color::Cyan }),
        ),
        Span::styled("Zoom: ", Style::default().fg(Color::DarkGray)),
        Span::styled(app.zoom_level(), Style::default().fg(Color::Yellow)),
        Span::styled(" (", Style::default().fg(Color::DarkGray)),
        Span::styled(app.lod_level(), Style::default().fg(Color::Magenta)),
        Span::styled(") ", Style::default().fg(Color::DarkGray)),
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
