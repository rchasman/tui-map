use crate::map::{Lod, MapRenderer, Viewport};

/// A nuclear explosion with position and animation frame
#[derive(Clone)]
pub struct Explosion {
    pub lon: f64,
    pub lat: f64,
    pub frame: u8,
    pub radius_km: f64,
}

/// Application state
pub struct App {
    pub viewport: Viewport,
    pub map_renderer: MapRenderer,
    pub should_quit: bool,
    /// Last mouse position for drag tracking
    pub last_mouse: Option<(u16, u16)>,
    /// Current mouse position for cursor marker
    pub mouse_pos: Option<(u16, u16)>,
    /// Active explosions
    pub explosions: Vec<Explosion>,
    /// Total casualties
    pub casualties: u64,
}

impl App {
    pub fn new(width: usize, height: usize) -> Self {
        // Braille gives 2x4 resolution per character
        // Account for border (2 chars horizontal, 2 chars vertical including status bar)
        let inner_width = width.saturating_sub(2);
        let inner_height = height.saturating_sub(3); // 2 for border + 1 for status bar
        let pixel_width = inner_width * 2;
        let pixel_height = inner_height * 4;

        Self {
            viewport: Viewport::world(pixel_width, pixel_height),
            map_renderer: MapRenderer::new(),
            should_quit: false,
            last_mouse: None,
            mouse_pos: None,
            explosions: Vec::new(),
            casualties: 0,
        }
    }

    /// Update viewport size when terminal resizes
    pub fn resize(&mut self, width: usize, height: usize) {
        // Account for border (2 chars horizontal, 2 chars vertical including status bar)
        let inner_width = width.saturating_sub(2);
        let inner_height = height.saturating_sub(3);
        self.viewport.width = inner_width * 2;
        self.viewport.height = inner_height * 4;
    }

    /// Pan the map
    pub fn pan(&mut self, dx: i32, dy: i32) {
        self.viewport.pan(dx, dy);
    }

    /// Zoom in
    pub fn zoom_in(&mut self) {
        self.viewport.zoom_in();
    }

    /// Zoom out
    pub fn zoom_out(&mut self) {
        self.viewport.zoom_out();
    }

    /// Zoom in towards a screen position (terminal column/row)
    pub fn zoom_in_at(&mut self, col: u16, row: u16) {
        // Convert terminal coords to braille pixel coords
        // Each terminal cell is 2 braille pixels wide, 4 tall
        // Account for border (1 cell offset)
        let px = ((col.saturating_sub(1)) as i32) * 2;
        let py = ((row.saturating_sub(1)) as i32) * 4;
        self.viewport.zoom_in_at(px, py);
    }

    /// Zoom out from a screen position (terminal column/row)
    pub fn zoom_out_at(&mut self, col: u16, row: u16) {
        let px = ((col.saturating_sub(1)) as i32) * 2;
        let py = ((row.saturating_sub(1)) as i32) * 4;
        self.viewport.zoom_out_at(px, py);
    }

    /// Request quit
    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    /// Get current zoom level as a string
    pub fn zoom_level(&self) -> String {
        format!("{:.1}x", self.viewport.zoom)
    }

    /// Get current center coordinates as a string
    pub fn center_coords(&self) -> String {
        format!(
            "{:.1}°{}, {:.1}°{}",
            self.viewport.center_lat.abs(),
            if self.viewport.center_lat >= 0.0 { "N" } else { "S" },
            self.viewport.center_lon.abs(),
            if self.viewport.center_lon >= 0.0 { "E" } else { "W" }
        )
    }

    /// Get current LOD level as a string
    pub fn lod_level(&self) -> &'static str {
        match Lod::from_zoom(self.viewport.zoom) {
            Lod::Low => "110m",
            Lod::Medium => "50m",
            Lod::High => "10m",
        }
    }

    /// Handle mouse drag - returns true if we should pan
    pub fn handle_drag(&mut self, x: u16, y: u16) {
        if let Some((last_x, last_y)) = self.last_mouse {
            let dx = last_x as i32 - x as i32;
            let dy = last_y as i32 - y as i32;
            // Scale based on zoom: less sensitive when zoomed out
            let scale = if self.viewport.zoom < 2.0 {
                2
            } else if self.viewport.zoom < 4.0 {
                3
            } else {
                4
            };
            self.pan(dx * scale, dy * scale);
        }
        self.last_mouse = Some((x, y));
    }

    /// Reset drag state when mouse button released
    pub fn end_drag(&mut self) {
        self.last_mouse = None;
    }

    /// Update mouse cursor position
    pub fn set_mouse_pos(&mut self, col: u16, row: u16) {
        self.mouse_pos = Some((col, row));
    }

    /// Get mouse position in braille pixel coordinates (for rendering marker)
    pub fn mouse_pixel_pos(&self) -> Option<(i32, i32)> {
        self.mouse_pos.map(|(col, row)| {
            // Convert terminal coords to braille pixel coords
            // Account for border (1 cell offset)
            let px = ((col.saturating_sub(1)) as i32) * 2;
            let py = ((row.saturating_sub(1)) as i32) * 4;
            (px, py)
        })
    }

    /// Launch a nuke at the given screen position
    pub fn launch_nuke(&mut self, col: u16, row: u16) {
        let px = ((col.saturating_sub(1)) as i32) * 2;
        let py = ((row.saturating_sub(1)) as i32) * 4;
        let (lon, lat) = self.viewport.unproject(px, py);

        // Blast radius ~50km (scales with zoom for visual effect)
        let radius_km = 50.0 + 100.0 / self.viewport.zoom;

        self.explosions.push(Explosion {
            lon,
            lat,
            frame: 0,
            radius_km,
        });

        // Calculate casualties
        self.apply_blast_damage(lon, lat, radius_km);
    }

    /// Apply blast damage to cities within radius
    fn apply_blast_damage(&mut self, lon: f64, lat: f64, radius_km: f64) {
        for city in &mut self.map_renderer.cities {
            let dist = haversine_km(lon, lat, city.lon, city.lat);
            if dist < radius_km {
                // Closer = more casualties (inverse square falloff)
                let damage_ratio = 1.0 - (dist / radius_km).powi(2);

                // Direct hit (within 20% of radius) = total destruction
                let killed = if dist < radius_km * 0.2 {
                    city.population // Everyone dies
                } else {
                    (city.population as f64 * damage_ratio * 0.9) as u64
                };

                city.population = city.population.saturating_sub(killed);
                self.casualties += killed;
            }
        }
    }

    /// Update explosion animations, returns true if any are active
    pub fn update_explosions(&mut self) -> bool {
        self.explosions.retain_mut(|exp| {
            exp.frame += 1;
            exp.frame < 20 // Animation lasts 20 frames
        });
        !self.explosions.is_empty()
    }
}

/// Haversine distance in kilometers
fn haversine_km(lon1: f64, lat1: f64, lon2: f64, lat2: f64) -> f64 {
    let r = 6371.0; // Earth radius in km
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let lat1 = lat1.to_radians();
    let lat2 = lat2.to_radians();

    let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().asin();
    r * c
}
