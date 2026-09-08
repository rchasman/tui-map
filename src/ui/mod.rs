pub mod weapons;
pub mod menu;
mod feeds;

use crate::app::{App, WeaponType};
use crate::hash::hash3;
use crate::map::{MapLayers, Projection, WRAP_OFFSETS};
use weapons::{ExplosionRender, GasCloudRender, weapon_color, gas_clouds};

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
    Frame,
};

/// Render the UI
pub fn render(frame: &mut Frame, app: &mut App) {
    render_in(frame, app, frame.area());
}

/// Render into a host-provided terminal region.
pub fn render_in(frame: &mut Frame, app: &mut App, area: Rect) {
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

fn render_map(frame: &mut Frame, app: &mut App, area: Rect) {
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

    // Braille gives 2x4 resolution per character
    app.projection.set_size(inner.width as usize * 2, inner.height as usize * 4);
    let projection = &app.projection;

    // Render map layers
    let layers = app.map_renderer.render(inner.width as usize, inner.height as usize, projection);

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

            explosions.push(ExplosionRender {
                x: cx, y: cy, frame: exp.frame, radius, weapon_type: exp.weapon_type,
                lon: exp.lon, lat: exp.lat, radius_km: exp.radius_km,
            });
        }
    }

    // Limit max visible explosions (sort by radius descending, show biggest)
    const MAX_VISIBLE_EXPLOSIONS: usize = 50;
    if explosions.len() > MAX_VISIBLE_EXPLOSIONS {
        explosions.sort_by_key(|e| std::cmp::Reverse(e.radius));
        explosions.truncate(MAX_VISIBLE_EXPLOSIONS);
    }

    // Cloud extents are sampled geographically, even when their centers are offscreen.
    let gas_clouds: Vec<GasCloudRender> = app.gas_clouds.iter().map(|cloud| GasCloudRender {
        intensity: cloud.intensity, weapon_type: cloud.weapon_type,
        lon: cloud.lon, lat: cloud.lat, radius_km: cloud.current_radius_km,
    }).collect();

    // Screen-space fire map: reuse buffers across frames to avoid per-frame allocation
    let fire_map_width = inner.width as usize;
    let fire_map_height = inner.height as usize;
    let fire_map_size = fire_map_width * fire_map_height;
    if app.fire_map_dims != (fire_map_width, fire_map_height) {
        app.fire_map_intensity = vec![0; fire_map_size];
        app.fire_map_weapon = vec![WeaponType::Nuke; fire_map_size];
        app.fire_map_dims = (fire_map_width, fire_map_height);
    } else {
        app.fire_map_intensity.fill(0);
        // fire_map_weapon doesn't need clearing — only read at indices
        // where fire_map_intensity > 0, and add_fire always writes both.
    }

    // Helper to merge fire into map (max intensity wins, keeps its weapon)
    let fmi = &mut app.fire_map_intensity;
    let fmw = &mut app.fire_map_weapon;
    let mut add_fire = |cx: usize, cy: usize, intensity: u8, weapon: WeaponType| {
        if cx < fire_map_width && cy < fire_map_height {
            let idx = cy * fire_map_width + cx;
            if intensity > fmi[idx] {
                fmi[idx] = intensity;
                fmw[idx] = weapon;
            }
        }
    };

    // Compute viewport bounds for fire culling
    let zoom = projection.effective_zoom();
    let (vp_min_lon, vp_min_lat, vp_max_lon, vp_max_lat) = if is_globe {
        if let Projection::Globe(ref g) = projection {
            let bounds = g.visible_bounds();
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

    if deg_per_char < 0.25 {
        for fire in &app.fires {
            if fire.lat < vp_min_lat || fire.lat > vp_max_lat {
                continue;
            }
            if let Some((px, py)) = projection.project_point(fire.lon, fire.lat) {
                let cx = px / 2;
                let cy = py / 4;
                if cx >= 0 && cy >= 0 {
                    let frac = app.map_renderer.land_fraction(fire.lon, fire.lat);
                    let intensity = (fire.intensity as f64 * frac) as u8;
                    if intensity > 0 {
                        add_fire(cx as usize, cy as usize, intensity, fire.weapon_type);
                    }
                }
            }
        }
    } else {
        let grid = if deg_per_char >= 1.0 { &app.fire_grid } else { &app.fire_grid_fine };
        let res = grid.resolution;

        let cell_dots_h = projection.deg_to_pixels(res);
        let pad_x = ((cell_dots_h / 2.0 - 1.0) / 2.0).max(0.0).ceil() as i32;

        let mid_lat = ((vp_min_lat + vp_max_lat) / 2.0).clamp(-85.0, 85.0);
        let mid_lon = (vp_min_lon + vp_max_lon) / 2.0;
        let pad_y = match (
            projection.project_point(mid_lon, mid_lat + res / 2.0),
            projection.project_point(mid_lon, mid_lat - res / 2.0),
        ) {
            (Some((_, y0)), Some((_, y1))) => {
                let cell_dots_v = (y1 - y0).unsigned_abs() as f64;
                ((cell_dots_v / 4.0 - 1.0) / 2.0).max(0.0).ceil() as i32
            }
            _ => 0,
        };

        app.fires_region_buf.clear();
        grid.fires_in_region_into(
            vp_min_lon.max(-180.0), vp_min_lat, vp_max_lon.min(180.0), vp_max_lat,
            &mut app.fires_region_buf,
        );
        if !is_globe {
            if vp_min_lon < -180.0 {
                grid.fires_in_region_into(vp_min_lon + 360.0, vp_min_lat, 180.0, vp_max_lat, &mut app.fires_region_buf);
            }
            if vp_max_lon > 180.0 {
                grid.fires_in_region_into(-180.0, vp_min_lat, vp_max_lon - 360.0, vp_max_lat, &mut app.fires_region_buf);
            }
        }

        for &(lon, lat, intensity, weapon) in &app.fires_region_buf {
            if let Some((px, py)) = projection.project_point(lon, lat) {
                let cx = (px / 2) as i32;
                let cy = (py / 4) as i32;
                for dy in -pad_y..=pad_y {
                    for dx in -pad_x..=pad_x {
                        let fx = cx + dx;
                        let fy = cy + dy;
                        if fx >= 0 && fy >= 0 {
                            add_fire(fx as usize, fy as usize, intensity, weapon);
                        }
                    }
                }
            }
        }
    }

    // Convert fire map to FireRender vec (only non-zero cells)
    let fires: Vec<FireRender> = app.fire_map_intensity
        .iter()
        .enumerate()
        .filter_map(|(idx, &intensity)| {
            if intensity > 0 {
                let x = (idx % fire_map_width) as u16;
                let y = (idx / fire_map_width) as u16;
                Some(FireRender { x, y, intensity, weapon_type: app.fire_map_weapon[idx] })
            } else {
                None
            }
        })
        .collect();

    // Cursor geographic position (for globe-aware reticle)
    let cursor_geo = cursor_pos.and_then(|(cx, cy)| {
        projection.unproject(cx as i32 * 2, cy as i32 * 4)
    });

    // Blast radius in km (EMP is 1.5× wider)
    let cursor_blast_km = {
        let base_radius = 50.0 + 700.0 / zoom;
        match app.active_weapon {
            WeaponType::Emp => base_radius * 1.5,
            _ => base_radius,
        }
    };

    // Resize reusable gas cloud density buffer if needed
    let gas_w = inner.width as usize;
    let gas_h = inner.height as usize;
    let gas_size = gas_w * gas_h;
    if app.gas_density_dims != (gas_w, gas_h) {
        app.gas_density_buf = vec![(0.0f32, 0.0f32); gas_size];
        app.gas_density_dims = (gas_w, gas_h);
    } else {
        app.gas_density_buf.fill((0.0, 0.0));
    }

    // Render braille map
    let map_widget = MapWidget {
        layers,
        cursor_pos,
        cursor_geo,
        cursor_blast_km,
        active_weapon: app.active_weapon,
        explosions,
        interactions: &app.interactions,
        compositor: &mut app.effect_compositor,
        fires,
        gas_clouds,
        density_buf: &mut app.gas_density_buf,
        inner_width: inner.width,
        inner_height: inner.height,
        frame: app.frame,
        projection,
    };
    frame.render_widget(map_widget, inner);
    feeds::render(frame, app, inner);
}

/// A fire cell to render
#[derive(Clone, Copy)]
struct FireRender {
    x: u16,
    y: u16,
    intensity: u8,
    weapon_type: WeaponType,
}

/// Custom widget that renders braille map with text labels overlaid
struct MapWidget<'a> {
    layers: MapLayers,
    cursor_pos: Option<(u16, u16)>,
    cursor_geo: Option<(f64, f64)>,
    cursor_blast_km: f64,
    active_weapon: WeaponType,
    explosions: Vec<ExplosionRender>,
    interactions: &'a crate::interactions::Interactions,
    compositor: &'a mut weapons::composite::Compositor,
    fires: Vec<FireRender>,
    gas_clouds: Vec<GasCloudRender>,
    density_buf: &'a mut [(f32, f32)],
    inner_width: u16,
    inner_height: u16,
    frame: u64,
    projection: &'a Projection,
}

impl<'a> MapWidget<'a> {
    /// Render a braille canvas layer with a specific color.
    /// Reads raw bytes directly — zero String allocations per frame.
    fn render_layer(&self, canvas: &crate::braille::BrailleCanvas, color: Color, area: Rect, buf: &mut Buffer) {
        let rows = canvas.char_height().min(area.height as usize);
        for row_idx in 0..rows {
            let y = area.y + row_idx as u16;
            for (col_idx, &b) in canvas.row_raw(row_idx).iter().enumerate() {
                if col_idx >= area.width as usize {
                    break;
                }
                if b == 0 { continue; }
                let ch = unsafe { char::from_u32_unchecked(0x2800 + b as u32) };
                let x = area.x + col_idx as u16;
                buf[(x, y)].set_char(ch).set_fg(color);
            }
        }
    }
}

impl<'a> Widget for MapWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Render layers from back to front:
        // 0. Globe outline (very faint, behind everything)
        if let Some(ref outline) = self.layers.globe_outline {
            self.render_layer(outline, Color::Rgb(50, 50, 50), area, buf);
        }

        // 1. County borders (DarkGray - at back)
        self.render_layer(&self.layers.counties, Color::DarkGray, area, buf);

        // 2. State borders (Yellow)
        self.render_layer(&self.layers.states, Color::Yellow, area, buf);

        // 3. Coastlines (Cyan)
        self.render_layer(&self.layers.coastlines, Color::Cyan, area, buf);

        // 4. Country borders (Cyan - on top so always visible above states)
        self.render_layer(&self.layers.borders, Color::Cyan, area, buf);

        // Render fires — weapon-tinted color gradients
        for fire in &self.fires {
            let x = area.x + fire.x;
            let y = area.y + fire.y;
            if x < area.x + area.width && y < area.y + area.height {
                let seed = hash3(fire.x as u64, fire.y as u64, self.frame);
                let flicker = ((seed & 0x1F) as i16) - 16;
                let vi = (fire.intensity as i16 + flicker).clamp(0, 255) as u8;

                let (r, g, b, ch) = match fire.weapon_type {
                    WeaponType::Chem => {
                        if vi > 220      { (255, 220, 255, '█') }
                        else if vi > 180 { (240, 140, 255, '█') }
                        else if vi > 140 { (200, 80, 220, '▓') }
                        else if vi > 100 { (180, 40, 180, '▓') }
                        else if vi > 60  { (140, 20, 140, '▒') }
                        else if vi > 30  { (100, 10, 100, '▒') }
                        else if vi > 15  { (70, 5, 70, '░') }
                        else             { (45, 0, 45, '░') }
                    }
                    _ => {
                        if vi > 220      { (255, 255, 240, '█') }
                        else if vi > 180 { (255, 240, 100, '█') }
                        else if vi > 140 { (255, 180, 30, '▓') }
                        else if vi > 100 { (255, 120, 0, '▓') }
                        else if vi > 60  { (255, 60, 0, '▒') }
                        else if vi > 30  { (200, 30, 0, '▒') }
                        else if vi > 15  { (140, 20, 0, '░') }
                        else             { (90, 10, 0, '░') }
                    }
                };

                buf[(x, y)].set_char(ch).set_fg(Color::Rgb(r, g, b));
            }
        }

        // Render gas clouds — merged density so overlapping clouds blend
        gas_clouds::render_interacting(&self.gas_clouds, self.density_buf, area, self.frame, buf, self.projection, self.interactions);

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

            let style = if is_dead {
                if is_marker {
                    Style::default().fg(Color::DarkGray).bg(Color::Reset)
                } else {
                    Style::default().fg(Color::DarkGray).bg(Color::Reset).add_modifier(Modifier::CROSSED_OUT)
                }
            } else {
                let brightness = (health * 200.0 + 55.0) as u8;
                Style::default().fg(Color::Rgb(brightness, brightness, brightness)).bg(Color::Reset)
            };

            let max_len = (self.inner_width.saturating_sub(*lx)) as usize;
            let display_text: String = if is_marker {
                display_text_raw.chars().take(1).collect()
            } else {
                display_text_raw.chars().take(max_len).collect()
            };

            for (i, ch) in display_text.chars().enumerate() {
                let px = x + i as u16;
                if px < area.x + area.width {
                    buf[(px, y)].set_char(ch).set_style(style);
                }
            }
        }

        // Render explosions — dispatch per weapon type
        self.compositor.render(&self.explosions, self.interactions, self.projection,
            area, self.frame, buf);

        // Render cursor targeting reticle — color from active weapon
        let reticle_color = weapon_color(self.active_weapon);
        if let Some((cx, cy)) = self.cursor_pos {
            let center_x = area.x as i32 + cx as i32;
            let center_y = area.y as i32 + cy as i32;

            if let Projection::Globe(ref globe) = self.projection {
                if let Some((cursor_lon, cursor_lat)) = self.cursor_geo {
                    let radius_deg = self.cursor_blast_km / 111.0;
                    let cos_lat = cursor_lat.to_radians().cos().max(0.1);

                    for i in 0..128u32 {
                        let angle = (i as f64 / 128.0) * std::f64::consts::TAU;
                        let dlat = radius_deg * angle.sin();
                        let dlon = (radius_deg * angle.cos()) / cos_lat;

                        if let Some((px, py)) = globe.project(cursor_lon + dlon, cursor_lat + dlat) {
                            let scx = px / 2;
                            let scy = py / 4;

                            if scx >= 0 && scx < self.inner_width as i32
                                && scy >= 0 && scy < self.inner_height as i32 {
                                buf[(area.x + scx as u16, area.y + scy as u16)]
                                    .set_char('·')
                                    .set_fg(reticle_color);
                            }
                        }
                    }
                }
            } else {
                let degrees = self.cursor_blast_km / 111.0;
                let pixels = self.projection.deg_to_pixels(degrees) as u16;
                let radius = (pixels / 2).max(3);
                let r = radius as i32;

                let min_x = (center_x - r).max(area.x as i32);
                let max_x = (center_x + r).min((area.x + area.width) as i32 - 1);
                let min_y = (center_y - r).max(area.y as i32);
                let max_y = (center_y + r).min((area.y + area.height) as i32 - 1);

                let r_sq = r * r;
                let inner_r_sq = (r - 1).max(0) * (r - 1).max(0);

                for y in min_y..=max_y {
                    let dy = y - center_y;
                    let dy_sq = dy * dy;

                    for x in min_x..=max_x {
                        let dx = x - center_x;
                        let dist_sq = dx * dx + dy_sq;

                        if dist_sq >= inner_r_sq && dist_sq <= r_sq {
                            buf[(x as u16, y as u16)]
                                .set_char('·')
                                .set_fg(reticle_color);
                        }
                    }
                }
            }

            // Center crosshair
            if center_x >= area.x as i32 && center_x < (area.x + area.width) as i32 &&
               center_y >= area.y as i32 && center_y < (area.y + area.height) as i32 {
                buf[(center_x as u16, center_y as u16)]
                    .set_char('✕')
                    .set_fg(reticle_color);
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
        Span::styled("| ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{} {}", app.active_weapon.symbol(), app.active_weapon.label()),
            Style::default().fg(weapon_color(app.active_weapon)),
        ),
        if app.casualties > 0 {
            Span::styled(
                format!(" | CASUALTIES: {}", format_casualties(app.casualties)),
                Style::default().fg(Color::Red),
            )
        } else {
            Span::raw("")
        },
    ]);

    let paragraph = Paragraph::new(status);
    frame.render_widget(paragraph, area);
}

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
