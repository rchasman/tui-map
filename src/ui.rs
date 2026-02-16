use crate::app::{App, WeaponType};
use crate::hash::{hash2, hash3};
use crate::map::{GlobeViewport, MapLayers, Projection, WRAP_OFFSETS};
use crate::map::globe::lonlat_to_vec3;
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

    // Project gas clouds to screen coordinates
    let mut gas_clouds: Vec<GasCloudRender> = Vec::new();
    for cloud in &app.gas_clouds {
        let screen_positions: Vec<(i32, i32)> = if is_globe {
            projection.project_point(cloud.lon, cloud.lat).into_iter().collect()
        } else {
            if let Projection::Mercator(ref vp) = projection {
                WRAP_OFFSETS.iter().filter_map(|&offset| {
                    let ((px, py), _) = vp.project_wrapped(cloud.lon, cloud.lat, offset);
                    (px >= 0 && py >= 0 && px <= 30000 && py <= 30000).then_some((px, py))
                }).collect()
            } else {
                Vec::new()
            }
        };

        for (px, py) in screen_positions {
            let cx = (px / 2) as u16;
            let cy = (py / 4) as u16;

            let degrees = cloud.current_radius_km / 111.0;
            let pixels = projection.deg_to_pixels(degrees) as u16;
            let radius = (pixels / 2).max(3);

            if radius < 2 { continue; }

            let left_edge = cx.saturating_sub(radius);
            let top_edge = cy.saturating_sub(radius);
            let right_edge = cx.saturating_add(radius);
            let bottom_edge = cy.saturating_add(radius);

            if right_edge < 1 || bottom_edge < 1 || left_edge >= inner.width || top_edge >= inner.height {
                continue;
            }

            gas_clouds.push(GasCloudRender {
                x: cx, y: cy, radius, intensity: cloud.intensity, weapon_type: cloud.weapon_type,
                lon: cloud.lon, lat: cloud.lat, radius_km: cloud.current_radius_km,
            });
        }
    }

    // Screen-space fire map: merge overlapping fires by tracking max intensity + weapon per cell
    let fire_map_width = inner.width as usize;
    let fire_map_height = inner.height as usize;
    let fire_map_size = fire_map_width * fire_map_height;
    let mut fire_map_intensity: Vec<u8> = vec![0; fire_map_size];
    let mut fire_map_weapon: Vec<WeaponType> = vec![WeaponType::Nuke; fire_map_size];

    // Helper to merge fire into map (max intensity wins, keeps its weapon)
    let mut add_fire = |cx: usize, cy: usize, intensity: u8, weapon: WeaponType| {
        if cx < fire_map_width && cy < fire_map_height {
            let idx = cy * fire_map_width + cx;
            if intensity > fire_map_intensity[idx] {
                fire_map_intensity[idx] = intensity;
                fire_map_weapon[idx] = weapon;
            }
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
        let res = grid.resolution;

        // Compute cell fill padding ONCE per frame to prevent gaps at high zoom.
        // When grid cells span > 1 char, we fill a rect around each center.
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

        for (lon, lat, intensity, weapon) in fires_data {
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
    let fires: Vec<FireRender> = fire_map_intensity
        .iter()
        .enumerate()
        .filter_map(|(idx, &intensity)| {
            if intensity > 0 {
                let x = (idx % fire_map_width) as u16;
                let y = (idx / fire_map_width) as u16;
                Some(FireRender { x, y, intensity, weapon_type: fire_map_weapon[idx] })
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

    // Render braille map
    let map_widget = MapWidget {
        layers,
        cursor_pos,
        cursor_geo,
        cursor_blast_km,
        active_weapon: app.active_weapon,
        explosions,
        fires,
        gas_clouds,
        inner_width: inner.width,
        inner_height: inner.height,
        frame: app.frame,
        projection,
    };
    frame.render_widget(map_widget, inner);
}

/// An explosion to render
struct ExplosionRender {
    x: u16,
    y: u16,
    frame: u8,
    radius: u16, // Visual radius in chars
    weapon_type: WeaponType,
    lon: f64,
    lat: f64,
    radius_km: f64,
}

/// A fire to render
#[derive(Clone, Copy)]
struct FireRender {
    x: u16,
    y: u16,
    intensity: u8,
    weapon_type: WeaponType,
}

/// A gas cloud to render
struct GasCloudRender {
    x: u16,
    y: u16,
    radius: u16,
    intensity: u16,
    weapon_type: WeaponType,
    lon: f64,
    lat: f64,
    radius_km: f64,
}

/// Custom widget that renders braille map with text labels overlaid
struct MapWidget {
    layers: MapLayers,
    cursor_pos: Option<(u16, u16)>,
    cursor_geo: Option<(f64, f64)>,
    cursor_blast_km: f64,
    active_weapon: WeaponType,
    explosions: Vec<ExplosionRender>,
    fires: Vec<FireRender>,
    gas_clouds: Vec<GasCloudRender>,
    inner_width: u16,
    inner_height: u16,
    frame: u64,
    projection: Projection,
}

impl MapWidget {
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
                if b == 0 { continue; } // skip empty
                let ch = unsafe { char::from_u32_unchecked(0x2800 + b as u32) };
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
                        // Purple-tinted fire: white → magenta → purple → dark plum
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
                        // Nuke (and any other): standard orange/red heat palette
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

        // Render gas clouds — noxious fog that expands as it decays
        for cloud in &self.gas_clouds {
            render_gas_cloud(cloud, area, self.frame, buf, &self.projection);
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
        let globe_ref = match &self.projection {
            Projection::Globe(g) => Some(g),
            _ => None,
        };
        for exp in &self.explosions {
            let x = area.x + exp.x;
            let y = area.y + exp.y;

            match exp.weapon_type {
                WeaponType::Nuke => render_nuke_explosion(exp, x, y, area, self.frame, buf, globe_ref),
                WeaponType::Bio => render_bio_explosion(exp, x, y, area, self.frame, buf, globe_ref),
                WeaponType::Emp => render_emp_explosion(exp, x, y, area, self.frame, buf, globe_ref),
                WeaponType::Chem => render_chem_explosion(exp, x, y, area, self.frame, buf, globe_ref),
            }
        }

        // Render cursor targeting reticle — color from active weapon
        let reticle_color = weapon_color(self.active_weapon);
        if let Some((cx, cy)) = self.cursor_pos {
            let center_x = area.x as i32 + cx as i32;
            let center_y = area.y as i32 + cy as i32;

            if let Projection::Globe(ref globe) = self.projection {
                // Globe: project geographic circle onto sphere surface
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
                // Mercator: screen-space circle
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

/// Map weapon type to its signature color
fn weapon_color(weapon: WeaponType) -> Color {
    match weapon {
        WeaponType::Nuke => Color::Red,
        WeaponType::Bio => Color::Rgb(0, 255, 50),
        WeaponType::Emp => Color::Rgb(0, 200, 255),
        WeaponType::Chem => Color::Rgb(200, 0, 200),
    }
}

// ── Per-weapon explosion renderers ──────────────────────────────────────────

/// Nuke: mushroom cloud rising UPWARD — white → yellow → orange → red → smoke
fn render_nuke_explosion(exp: &ExplosionRender, x: u16, y: u16, area: Rect, global_frame: u64, buf: &mut Buffer, globe: Option<&GlobeViewport>) {
    let progress = if exp.frame < 20 {
        (exp.frame as f32 / 20.0).powf(0.7)
    } else if exp.frame < 40 {
        1.0 + ((exp.frame - 20) as f32 / 20.0) * 0.3
    } else {
        1.3
    };
    let max_r = exp.radius as f32 * progress;
    let cap_height = (max_r * (2.0 + (exp.frame as f32 / 60.0) * 1.2)) as i16;
    let cap_width = max_r;

    let flash_phase = exp.frame < 8;
    let fireball_phase = exp.frame < 25;
    let cooling_phase = exp.frame < 45;

    let radius_i16 = exp.radius as i16;
    let cap_height_f32 = cap_height as f32;
    let frame_seed_component = global_frame + exp.frame as u64;

    for dy in -cap_height..0 {
        let py_signed = (y as i16) + dy;
        if py_signed < 0 || py_signed >= (area.y + area.height) as i16 { continue; }
        let py = py_signed as u16;

        let dy_sq = dy * dy;
        let dy_f32 = dy as f32;
        let height_ratio = -dy_f32 / cap_height_f32;

        let (base_width, height_mult, large_mult, fine_mult) = if height_ratio < 0.2 {
            (0.5, 0.4, 0.0, 0.5)
        } else if height_ratio < 0.5 {
            (0.9, 1.5, 0.7, 0.3)
        } else if height_ratio < 0.75 {
            (1.4, 2.0, 1.2, 0.4)
        } else {
            (1.9, 2.5, 2.0, 0.8)
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
            let dx_f32 = dx as f32;
            let angle = dx_f32.atan2(dy_f32);
            let large_turb_seed = hash2((angle * 1000.0) as u64, global_frame / 5);
            let large_turbulence = ((large_turb_seed & 0xFF) as f32 / 255.0 - 0.5) * 0.6;
            let fine_turb_seed = hash3(dx as u64, dy as u64, frame_seed_component);
            let fine_turbulence = ((fine_turb_seed & 0xFF) as f32 / 255.0 - 0.5) * 0.4;

            let height_factor = base_width + height_component +
                               large_turbulence * large_mult +
                               fine_turbulence * fine_mult;
            let effective_width_sq = (cap_width * height_factor) * (cap_width * height_factor);

            if dist_sq <= effective_width_sq {
                let px_signed = (x as i16) + dx;
                if px_signed < 0 || px_signed >= (area.x + area.width) as i16 { continue; }
                let px = px_signed as u16;

                if let Some(g) = globe {
                    let bx = (px as i32 - area.x as i32) * 2;
                    let by = (py as i32 - area.y as i32) * 4;
                    if g.pixel_to_sphere_point(bx, by).is_none() { continue; }
                }

                let radial_dist = dist_sq.sqrt() / (cap_width * height_factor);
                let vertical_factor = (-dy as f32) / cap_height as f32;
                let dist_norm = (radial_dist * 0.5 + vertical_factor * 0.5).min(1.0);

                let seed = hash3(px as u64, py as u64, global_frame + exp.frame as u64);
                let flicker = ((seed & 0xFF) as f32) / 255.0;

                let (r, g, b, ch) = if flash_phase {
                    if dist_norm < 0.4 { (255, 255, 255, '█') }
                    else if dist_norm < 0.7 { (255, 250, 220, '█') }
                    else { (255, 240, 150, '▓') }
                } else if fireball_phase {
                    let phase_progress = (exp.frame - 8) as f32 / 17.0;
                    let core_threshold = 0.3 - (phase_progress * 0.15);
                    if dist_norm < core_threshold { (255, 255, 250, '█') }
                    else if dist_norm < 0.4 {
                        (255, (250.0 - phase_progress * 70.0) as u8, (120.0 - phase_progress * 100.0) as u8, '▓')
                    } else if dist_norm < 0.6 {
                        (255, (180.0 - phase_progress * 100.0) as u8, (20.0 * (1.0 - phase_progress)) as u8, '▓')
                    } else if dist_norm < 0.8 { (255, 80, 0, '▒') }
                    else { (200, 40, 0, '░') }
                } else if cooling_phase {
                    let cooling_progress = (exp.frame - 25) as f32 / 20.0;
                    if dist_norm < 0.15 {
                        let pulse = if (exp.frame / 3) % 2 == 0 { 60 } else { 20 };
                        (255, pulse, 30, '☢')
                    } else if dist_norm < 0.4 {
                        ((220.0 - cooling_progress * 80.0 - flicker * 40.0) as u8, (60.0 - cooling_progress * 20.0) as u8, 0, '▓')
                    } else if dist_norm < 0.7 {
                        ((160.0 - cooling_progress * 50.0) as u8, (40.0 - cooling_progress * 20.0) as u8, 0, '▒')
                    } else {
                        ((100.0 - cooling_progress * 20.0) as u8, (20.0 - cooling_progress * 10.0) as u8, 0, '░')
                    }
                } else {
                    let final_progress = (exp.frame - 45) as f32 / 15.0;
                    let ch = if dist_norm > 0.5 { '░' } else { '▒' };
                    ((80.0 - final_progress * 30.0) as u8, (15.0 - final_progress * 10.0) as u8, 0, ch)
                };

                buf[(px, py)].set_char(ch).set_fg(Color::Rgb(r, g, b));
            }
        }
    }
}

/// Bio: low creeping fog — wide but stays low, neon green palette, irregular tendrils
fn render_bio_explosion(exp: &ExplosionRender, x: u16, y: u16, area: Rect, global_frame: u64, buf: &mut Buffer, globe: Option<&GlobeViewport>) {
    let progress = if exp.frame < 20 {
        (exp.frame as f32 / 20.0).powf(0.5) // Faster initial spread
    } else if exp.frame < 40 {
        1.0 + ((exp.frame - 20) as f32 / 20.0) * 0.4
    } else {
        1.4
    };
    let max_r = exp.radius as f32 * progress;

    // Low fog: 40% of nuke height, 1.8× width
    let cap_height = (max_r * 0.4 * (1.5 + (exp.frame as f32 / 60.0) * 0.5)) as i16;
    let cap_width = max_r * 1.8;

    let flash_phase = exp.frame < 5;
    let spread_phase = exp.frame < 20;
    let creep_phase = exp.frame < 45;

    let radius_i16 = (exp.radius as f32 * 1.8) as i16;
    let cap_height_f32 = cap_height.max(1) as f32;
    let frame_seed_component = global_frame + exp.frame as u64;

    // Fog extends both slightly above AND below cursor (hugs ground)
    let dy_min = -cap_height;
    let dy_max = (cap_height / 3).max(2); // Small drip below

    for dy in dy_min..=dy_max {
        let py_signed = (y as i16) + dy;
        if py_signed < 0 || py_signed >= (area.y + area.height) as i16 { continue; }
        let py = py_signed as u16;

        let dy_sq = dy * dy;
        let dy_f32 = dy as f32;
        let height_ratio = dy_f32.abs() / cap_height_f32;

        for dx in -(radius_i16)..=(radius_i16) {
            let dist_sq = (dx * dx + dy_sq) as f32;
            let dx_f32 = dx as f32;
            let angle = dx_f32.atan2(dy_f32);

            // Higher fine turbulence for irregular tendrils
            let large_turb_seed = hash2((angle * 800.0) as u64, global_frame / 4);
            let large_turbulence = ((large_turb_seed & 0xFF) as f32 / 255.0 - 0.5) * 0.8;
            let fine_turb_seed = hash3(dx as u64, dy as u64, frame_seed_component);
            let fine_turbulence = ((fine_turb_seed & 0xFF) as f32 / 255.0 - 0.5) * 0.7; // High fine turbulence

            // Width-dominant shape (wide, low)
            let height_factor = 1.0 + large_turbulence * 0.6 + fine_turbulence * 0.5;
            let effective_width_sq = (cap_width * height_factor) * (cap_width * height_factor);

            // Vertical falloff: fog thins rapidly with height
            let vert_falloff = 1.0 - (height_ratio * height_ratio);
            let in_fog = dist_sq <= effective_width_sq * vert_falloff.max(0.0);

            if in_fog {
                let px_signed = (x as i16) + dx;
                if px_signed < 0 || px_signed >= (area.x + area.width) as i16 { continue; }
                let px = px_signed as u16;

                if let Some(g) = globe {
                    let bx = (px as i32 - area.x as i32) * 2;
                    let by = (py as i32 - area.y as i32) * 4;
                    if g.pixel_to_sphere_point(bx, by).is_none() { continue; }
                }

                let radial_dist = dist_sq.sqrt() / (cap_width * height_factor).max(1.0);
                let dist_norm = (radial_dist * 0.6 + height_ratio * 0.4).min(1.0);

                let seed = hash3(px as u64, py as u64, global_frame + exp.frame as u64);
                let flicker = ((seed & 0xFF) as f32) / 255.0;

                let (r, g, b, ch) = if flash_phase {
                    if dist_norm < 0.4 { (200, 255, 200, '█') }
                    else if dist_norm < 0.7 { (100, 255, 80, '█') }
                    else { (50, 200, 40, '▓') }
                } else if spread_phase {
                    let p = (exp.frame - 5) as f32 / 15.0;
                    if dist_norm < 0.3 { (0, 255, 50, '█') }
                    else if dist_norm < 0.5 { ((40.0 * p) as u8, (255.0 - p * 55.0) as u8, (50.0 - p * 30.0) as u8, '▓') }
                    else if dist_norm < 0.7 { (80, (200.0 - p * 60.0) as u8, 0, '▒') }
                    else { (40, (120.0 - p * 40.0) as u8, 0, '░') }
                } else if creep_phase {
                    let p = (exp.frame - 20) as f32 / 25.0;
                    if dist_norm < 0.15 {
                        let pulse = if (exp.frame / 4) % 2 == 0 { 255 } else { 180 };
                        (0, pulse, 30, '☣')
                    } else if dist_norm < 0.4 {
                        ((40.0 + flicker * 20.0) as u8, (180.0 - p * 60.0) as u8, (20.0 - p * 10.0) as u8, '▓')
                    } else if dist_norm < 0.7 {
                        ((50.0 - p * 15.0) as u8, (100.0 - p * 30.0) as u8, (10.0 - p * 5.0) as u8, '▒')
                    } else {
                        ((40.0 - p * 10.0) as u8, (60.0 - p * 20.0) as u8, (10.0 - p * 5.0) as u8, '░')
                    }
                } else {
                    let p = (exp.frame - 45) as f32 / 15.0;
                    let ch = if dist_norm > 0.5 { '░' } else { '▒' };
                    ((30.0 - p * 15.0) as u8, (40.0 - p * 20.0) as u8, (20.0 - p * 10.0) as u8, ch)
                };

                buf[(px, py)].set_char(ch).set_fg(Color::Rgb(r, g, b));
            }
        }
    }
}

/// EMP: expanding concentric rings — electric blue/cyan, fast, short duration
fn render_emp_explosion(exp: &ExplosionRender, x: u16, y: u16, area: Rect, global_frame: u64, buf: &mut Buffer, globe: Option<&GlobeViewport>) {
    // 3 rings expanding at staggered speeds, fills radius by frame 15
    let progress = (exp.frame as f32 / 15.0).min(1.0); // Full expansion by frame 15
    let fade = if exp.frame > 15 { (exp.frame - 15) as f32 / 15.0 } else { 0.0 };

    let max_r = exp.radius as f32 * progress;

    // 3 ring radii at different expansion speeds
    let ring_radii = [
        max_r * 1.0,            // Outer ring (fastest)
        max_r * 0.65,           // Middle ring
        max_r * 0.35,           // Inner ring
    ];
    let ring_thickness = 2.0_f32; // ~2 chars thick

    // Globe: geographic → screen distance mapping (angular distance × scale factor)
    let center_vec = lonlat_to_vec3(exp.lon, exp.lat);
    let geo_scale = {
        let max_angle = exp.radius_km / 6371.0;
        exp.radius as f64 / max_angle
    };

    // Scan area covers full circle (above AND below cursor)
    let scan_r = (max_r as i16) + 3;

    for dy in -scan_r..=scan_r {
        let py_signed = (y as i16) + dy;
        if py_signed < 0 || py_signed >= (area.y + area.height) as i16 { continue; }
        let py = py_signed as u16;

        for dx in -scan_r..=scan_r {
            let px_signed = (x as i16) + dx;
            if px_signed < 0 || px_signed >= (area.x + area.width) as i16 { continue; }
            let px = px_signed as u16;

            // Distance: geographic on globe (conforms to curvature), screen-space on Mercator
            let dist: f32 = if let Some(g) = globe {
                let bx = (px as i32 - area.x as i32) * 2;
                let by = (py as i32 - area.y as i32) * 4;
                match g.pixel_to_sphere_point(bx, by) {
                    None => continue, // outside globe disk
                    Some(p) => {
                        let dot = p.dot(center_vec).clamp(-1.0, 1.0);
                        (dot.acos() * geo_scale) as f32
                    }
                }
            } else {
                ((dx * dx + dy * dy) as f32).sqrt()
            };

            // Check if this pixel is near any ring
            let mut best_ring: Option<(f32, usize)> = None; // (proximity to ring, ring_index)
            for (i, &ring_r) in ring_radii.iter().enumerate() {
                if ring_r < 1.0 { continue; }
                let proximity = (dist - ring_r).abs();
                if proximity <= ring_thickness {
                    if best_ring.is_none() || proximity < best_ring.unwrap().0 {
                        best_ring = Some((proximity, i));
                    }
                }
            }

            // Also add flickering arc sparks between rings
            let spark_seed = hash3(dx as u64, dy as u64, global_frame + exp.frame as u64);
            let is_spark = (spark_seed & 0x1F) == 0 && dist < max_r && dist > ring_radii[2] * 0.5;

            if let Some((proximity, ring_idx)) = best_ring {
                let ring_fade = proximity / ring_thickness; // 0 at center, 1 at edge
                let age_fade = 1.0 - fade;

                // Rapid pulse/flicker (frame-by-frame jitter)
                let jitter = ((spark_seed & 0x3) as f32) / 3.0;
                let brightness = ((1.0 - ring_fade) * age_fade * (0.7 + jitter * 0.3)).min(1.0);

                if brightness < 0.05 { continue; }

                // Color: inner rings brighter cyan, outer rings deeper blue
                let (r, g, b, ch) = match ring_idx {
                    0 => { // Outer ring — deep blue fading
                        let b_val = (200.0 * brightness) as u8;
                        (0, (80.0 * brightness) as u8, b_val, if brightness > 0.5 { '▓' } else { '░' })
                    }
                    1 => { // Middle ring — electric cyan
                        ((50.0 * brightness) as u8, (200.0 * brightness) as u8, (255.0 * brightness) as u8,
                         if brightness > 0.6 { '█' } else { '▒' })
                    }
                    _ => { // Inner ring — blinding white-cyan
                        let w = (brightness * 255.0) as u8;
                        (w, w, (255.0 * brightness) as u8, '█')
                    }
                };

                buf[(px, py)].set_char(ch).set_fg(Color::Rgb(r, g, b));
            } else if is_spark && fade < 0.5 {
                // Arc sparks between rings
                buf[(px, py)].set_char('·').set_fg(Color::Rgb(0, 255, 255));
            }
        }
    }
}

/// Chem: dense dome/sphere expanding in ALL directions — purple palette, dripping
fn render_chem_explosion(exp: &ExplosionRender, x: u16, y: u16, area: Rect, global_frame: u64, buf: &mut Buffer, globe: Option<&GlobeViewport>) {
    let progress = if exp.frame < 20 {
        (exp.frame as f32 / 20.0).powf(0.6)
    } else if exp.frame < 40 {
        1.0 + ((exp.frame - 20) as f32 / 20.0) * 0.3
    } else {
        1.3
    };
    let max_r = exp.radius as f32 * progress;

    // Spherical: equal radius in all directions (above AND below)
    let sphere_r = (max_r * 1.5) as i16;
    let sphere_r_f32 = sphere_r as f32;

    let flash_phase = exp.frame < 6;
    let fireball_phase = exp.frame < 22;
    let cooling_phase = exp.frame < 45;

    let radius_i16 = (exp.radius as f32 * 1.5) as i16;
    let frame_seed_component = global_frame + exp.frame as u64;

    // Globe: geographic → screen distance mapping
    let center_vec = lonlat_to_vec3(exp.lon, exp.lat);
    let geo_scale = {
        let max_angle = exp.radius_km / 6371.0;
        // Scale maps geographic angle to screen units matching sphere_r_f32
        (exp.radius as f64 * 1.5) / max_angle
    };

    // Drip zone: extra chars trailing below the sphere
    let drip_extra = (max_r * 0.3) as i16;

    for dy in -sphere_r..=(sphere_r + drip_extra) {
        let py_signed = (y as i16) + dy;
        if py_signed < 0 || py_signed >= (area.y + area.height) as i16 { continue; }
        let py = py_signed as u16;

        let dy_sq = dy * dy;
        let is_drip_zone = dy > sphere_r;

        for dx in -(radius_i16)..=(radius_i16) {
            // Bounds check (moved up for globe path efficiency)
            let px_signed = (x as i16) + dx;
            if px_signed < 0 || px_signed >= (area.x + area.width) as i16 { continue; }
            let px = px_signed as u16;

            // Distance: geographic on globe, screen-space on Mercator
            let dist: f32 = if let Some(g) = globe {
                let bx = (px as i32 - area.x as i32) * 2;
                let by = (py as i32 - area.y as i32) * 4;
                match g.pixel_to_sphere_point(bx, by) {
                    None => continue, // outside globe disk
                    Some(p) => {
                        let dot = p.dot(center_vec).clamp(-1.0, 1.0);
                        (dot.acos() * geo_scale) as f32
                    }
                }
            } else {
                ((dx * dx + dy_sq) as f32).sqrt()
            };

            // Dense sphere check (less turbulence = more solid fill)
            let turb_seed = hash3(dx as u64, dy as u64, frame_seed_component);
            let turbulence = ((turb_seed & 0xFF) as f32 / 255.0 - 0.5) * 0.25; // Low turbulence

            let effective_r = sphere_r_f32 * (1.0 + turbulence);

            let in_sphere = if is_drip_zone {
                // Drip effect: narrow vertical trails below sphere (screen-space)
                let drip_seed = hash2(dx as u64, global_frame / 3);
                let drip_chance = (drip_seed & 0x7) < 2; // ~25% of columns drip
                let drip_progress = (dy - sphere_r) as f32 / drip_extra as f32;
                drip_chance && dx.abs() < radius_i16 / 2 && drip_progress < (1.0 - (dx.abs() as f32 / radius_i16 as f32))
            } else {
                dist <= effective_r
            };

            if in_sphere {
                let dist_norm = if is_drip_zone {
                    0.8 + 0.2 * ((dy - sphere_r) as f32 / drip_extra.max(1) as f32)
                } else {
                    (dist / effective_r).min(1.0)
                };

                let seed = hash3(px as u64, py as u64, global_frame + exp.frame as u64);
                let flicker = ((seed & 0xFF) as f32) / 255.0;

                let (r, g, b, ch) = if is_drip_zone {
                    // Dripping trails
                    ((60.0 + flicker * 20.0) as u8, 0, (80.0 + flicker * 20.0) as u8, '░')
                } else if flash_phase {
                    if dist_norm < 0.4 { (240, 200, 255, '█') }
                    else if dist_norm < 0.7 { (200, 100, 255, '█') }
                    else { (160, 60, 200, '▓') }
                } else if fireball_phase {
                    let p = (exp.frame - 6) as f32 / 16.0;
                    if dist_norm < 0.3 { (200, (50.0 * (1.0 - p)) as u8, 200, '█') }
                    else if dist_norm < 0.5 { ((150.0 + p * 20.0) as u8, 0, (200.0 - p * 40.0) as u8, '▓') }
                    else if dist_norm < 0.7 { ((120.0 - p * 30.0) as u8, 0, (160.0 - p * 40.0) as u8, '▒') }
                    else { ((80.0 - p * 20.0) as u8, 0, (120.0 - p * 30.0) as u8, '░') }
                } else if cooling_phase {
                    let p = (exp.frame - 22) as f32 / 23.0;
                    if dist_norm < 0.15 {
                        let pulse = if (exp.frame / 3) % 2 == 0 { 200 } else { 120 };
                        (pulse, 0, (200.0 - p * 40.0) as u8, '☠')
                    } else if dist_norm < 0.4 {
                        ((80.0 + flicker * 30.0 - p * 20.0) as u8, 0, (120.0 - p * 30.0) as u8, '▓')
                    } else if dist_norm < 0.7 {
                        ((60.0 - p * 15.0) as u8, 0, (80.0 - p * 20.0) as u8, '▒')
                    } else {
                        ((40.0 - p * 10.0) as u8, (10.0 * (1.0 - p)) as u8, (60.0 - p * 20.0) as u8, '░')
                    }
                } else {
                    let p = (exp.frame - 45) as f32 / 15.0;
                    let ch = if dist_norm > 0.5 { '░' } else { '▒' };
                    ((40.0 - p * 20.0) as u8, (20.0 - p * 10.0) as u8, (50.0 - p * 25.0) as u8, ch)
                };

                buf[(px, py)].set_char(ch).set_fg(Color::Rgb(r, g, b));
            }
        }
    }
}

/// Gas cloud: slow billowing noxious fog — neon green (Bio) or purple (Chem).
/// On globe: uses geographic distance (great-circle) so the cloud conforms to the sphere.
/// On mercator: uses screen-space distance (correct for flat projection).
fn render_gas_cloud(cloud: &GasCloudRender, area: Rect, global_frame: u64, buf: &mut Buffer, projection: &Projection) {
    let cx = area.x + cloud.x;
    let cy = area.y + cloud.y;
    let r = cloud.radius as i16;
    if r < 2 { return; }

    let intensity_norm = (cloud.intensity as f32 / 2000.0).min(1.0);
    let intensity_scale = 0.3 + intensity_norm * 0.7;

    // Very slow time phases for gradual morphing
    let time_slow = global_frame / 180;
    let time_glacial = global_frame / 300;

    // Stable cloud identity from geographic position (doesn't change with globe spin)
    let cloud_id = hash2(
        (cloud.lon * 1000.0).to_bits(),
        (cloud.lat * 1000.0).to_bits(),
    );

    // Geographic radius in radians (for globe sphere distance)
    let radius_rad = cloud.radius_km / 6371.0;

    // Precompute cloud center as unit-sphere Vec3 for globe mode
    let is_globe = matches!(projection, Projection::Globe(_));
    let cloud_vec3 = if is_globe {
        Some(lonlat_to_vec3(cloud.lon, cloud.lat))
    } else {
        None
    };

    // Precompute 12 angular lobe factors (0.55..0.95 range, slowly morphing)
    const N_LOBES: usize = 12;
    let mut lobe_factor = [0.0f32; N_LOBES];
    for i in 0..N_LOBES {
        let seed_a = hash3(i as u64, cloud_id, time_slow);
        let seed_b = hash3(i as u64, cloud_id, time_slow.wrapping_add(1));
        let na = (seed_a & 0xFF) as f32 / 255.0;
        let nb = (seed_b & 0xFF) as f32 / 255.0;

        let t_frac = (global_frame % 180) as f32 / 180.0;
        let t_smooth = (1.0 - (t_frac * std::f32::consts::PI).cos()) * 0.5;
        let n = na * (1.0 - t_smooth) + nb * t_smooth;

        lobe_factor[i] = (0.55 + n * 0.4) * intensity_scale;
    }

    // Widen bounding box slightly for globe limb distortion
    let scan_r = if is_globe { r + r / 4 } else { r };

    for dy in -scan_r..=scan_r {
        let py_signed = cy as i16 + dy;
        if py_signed < area.y as i16 || py_signed >= (area.y + area.height) as i16 { continue; }
        let py = py_signed as u16;

        for dx in -scan_r..=scan_r {
            let px_signed = cx as i16 + dx;
            if px_signed < area.x as i16 || px_signed >= (area.x + area.width) as i16 { continue; }
            let px = px_signed as u16;

            // Screen-space angle for lobe lookup (visual flair, same in both modes)
            let screen_angle = (dx as f32).atan2(dy as f32);
            let angle_norm = (screen_angle + std::f32::consts::PI) / std::f32::consts::TAU;
            let lobe_pos = angle_norm * N_LOBES as f32;
            let lobe_idx = (lobe_pos as usize) % N_LOBES;
            let lobe_next = (lobe_idx + 1) % N_LOBES;
            let lobe_frac = lobe_pos - lobe_pos.floor();
            let t = (1.0 - (lobe_frac * std::f32::consts::PI).cos()) * 0.5;
            let lobe_mult = lobe_factor[lobe_idx] * (1.0 - t) + lobe_factor[lobe_next] * t;

            // Compute normalized distance (0=center, 1=edge) using appropriate geometry
            let dist_norm = if is_globe {
                if let Projection::Globe(ref g) = projection {
                    let bx = (px as i32 - area.x as i32) * 2;
                    let by = (py as i32 - area.y as i32) * 4;
                    let point = match g.pixel_to_sphere_point(bx, by) {
                        Some(p) => p,
                        None => continue, // behind the globe
                    };
                    let cv = cloud_vec3.unwrap();
                    let dot = cv.dot(point).clamp(-1.0, 1.0);
                    let angle_dist = dot.acos(); // radians on unit sphere
                    let effective_r = radius_rad * lobe_mult as f64;
                    if effective_r < 0.0001 { continue; }
                    (angle_dist / effective_r) as f32
                } else {
                    unreachable!()
                }
            } else {
                // Mercator: screen-space distance
                let dist = ((dx * dx + dy * dy) as f32).sqrt();
                let effective_r = r as f32 * lobe_mult;
                if effective_r < 1.0 { continue; }
                dist / effective_r
            };

            if dist_norm > 1.0 { continue; }

            // Stable spatial texture using geographic coords for globe stability
            let tex_key = if is_globe {
                // Use pixel position XORed with cloud_id — stable relative to sphere
                hash3(
                    (px as u64).wrapping_mul(31337) ^ cloud_id,
                    (py as u64).wrapping_mul(7919),
                    time_glacial,
                )
            } else {
                hash3(
                    (px as u64).wrapping_mul(31337),
                    (py as u64).wrapping_mul(7919),
                    time_glacial,
                )
            };
            let texture = ((tex_key & 0xFF) as f32 / 255.0 - 0.5) * 0.15;

            // Edge-only noise: inner 60% stays solid, outer 40% gets wispy
            let edge_factor = ((dist_norm - 0.6) / 0.4).max(0.0);
            let adjusted_dist = dist_norm + texture * edge_factor * 2.0;
            if adjusted_dist > 1.0 { continue; }

            // Density: solid center, smooth quadratic falloff
            let density = (1.0 - adjusted_dist.max(0.0)).powi(2) * intensity_norm;

            // Gentle spatial color variation
            let shade_seed = hash2(px as u64 ^ 0xBEEF, py as u64 ^ 0xCAFE);
            let shade = ((shade_seed & 0x1F) as f32) / 31.0;

            let (r, g, b, ch) = match cloud.weapon_type {
                WeaponType::Bio => {
                    if density > 0.5 {
                        ((10.0 + shade * 15.0) as u8, (180.0 + shade * 40.0) as u8, (30.0 + shade * 15.0) as u8, '▓')
                    } else if density > 0.2 {
                        (0, (100.0 + shade * 40.0) as u8, (15.0 + shade * 10.0) as u8, '▒')
                    } else if density > 0.05 {
                        (0, (45.0 + shade * 25.0) as u8, (5.0 + shade * 5.0) as u8, '░')
                    } else {
                        continue;
                    }
                }
                _ => {
                    if density > 0.5 {
                        ((120.0 + shade * 40.0) as u8, (5.0 + shade * 10.0) as u8, (160.0 + shade * 40.0) as u8, '▓')
                    } else if density > 0.2 {
                        ((65.0 + shade * 30.0) as u8, 0, (100.0 + shade * 30.0) as u8, '▒')
                    } else if density > 0.05 {
                        ((25.0 + shade * 15.0) as u8, 0, (45.0 + shade * 20.0) as u8, '░')
                    } else {
                        continue;
                    }
                }
            };

            buf[(px, py)].set_char(ch).set_fg(Color::Rgb(r, g, b));
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
