use crate::geo::{normalize_lat, normalize_lon};
use crate::hash::{hash3, rand_simple};
use crate::map::{Lod, MapRenderer, Projection, Viewport};

/// A nuclear explosion with position and animation frame
#[derive(Clone)]
pub struct Explosion {
    pub lon: f64,
    pub lat: f64,
    pub frame: u8,
    pub radius_km: f64,
}

/// A spreading fire
#[derive(Clone)]
pub struct Fire {
    pub lon: f64,
    pub lat: f64,
    pub intensity: u8, // 0-255, decays over time
}

/// Radioactive fallout zone
#[derive(Clone)]
pub struct Fallout {
    pub lon: f64,
    pub lat: f64,
    pub radius_km: f64,
    pub intensity: u16, // Decays slowly over many frames
}

/// Multi-resolution fire grid for viewport-aware rendering.
/// Configurable cell resolution enables hierarchical spatial queries:
/// coarse (1°) for zoomed-out, fine (0.25°) for medium zoom.
pub struct FireGrid {
    /// Max intensity per cell (0 = no fire)
    pub cells: Vec<u8>,
    pub width: usize,
    pub height: usize,
    pub resolution: f64,
}

impl FireGrid {
    pub fn new(resolution: f64) -> Self {
        let width = (360.0 / resolution) as usize;
        let height = (180.0 / resolution) as usize;
        Self {
            cells: vec![0; width * height],
            width,
            height,
            resolution,
        }
    }

    /// Rebuild grid from fires Vec - called after fire updates
    pub fn rebuild(&mut self, fires: &[Fire]) {
        self.cells.fill(0);
        for fire in fires {
            let lon_idx = (normalize_lon(fire.lon) / self.resolution) as usize;
            let lat_idx = (normalize_lat(fire.lat) / self.resolution) as usize;
            let idx = lat_idx * self.width + lon_idx;
            if idx < self.cells.len() {
                self.cells[idx] = self.cells[idx].max(fire.intensity);
            }
        }
    }

    /// Query fires within viewport bounds only (not all cells).
    /// Returns (lon, lat, intensity) at cell centers.
    pub fn fires_in_region(&self, min_lon: f64, min_lat: f64, max_lon: f64, max_lat: f64) -> Vec<(f64, f64, u8)> {
        let min_x = ((min_lon + 180.0).max(0.0) / self.resolution) as usize;
        let max_x = (((max_lon + 180.0).min(360.0)) / self.resolution).ceil() as usize;
        let min_y = ((min_lat + 90.0).max(0.0) / self.resolution) as usize;
        let max_y = (((max_lat + 90.0).min(180.0)) / self.resolution).ceil() as usize;

        let max_x = max_x.min(self.width);
        let max_y = max_y.min(self.height);

        let mut results = Vec::new();
        for lat_idx in min_y..max_y {
            let row_start = lat_idx * self.width;
            for lon_idx in min_x..max_x {
                let intensity = self.cells[row_start + lon_idx];
                if intensity > 0 {
                    let lon = lon_idx as f64 * self.resolution - 180.0 + self.resolution / 2.0;
                    let lat = lat_idx as f64 * self.resolution - 90.0 + self.resolution / 2.0;
                    results.push((lon, lat, intensity));
                }
            }
        }
        results
    }
}

/// Application state
pub struct App {
    pub projection: Projection,
    pub map_renderer: MapRenderer,
    pub should_quit: bool,
    /// Last mouse position for drag tracking
    pub last_mouse: Option<(u16, u16)>,
    /// Current mouse position for cursor marker
    pub mouse_pos: Option<(u16, u16)>,
    /// Active explosions
    pub explosions: Vec<Explosion>,
    /// Active fires
    pub fires: Vec<Fire>,
    /// Coarse 1° fire grid for zoomed-out rendering
    pub fire_grid: FireGrid,
    /// Fine 0.25° fire grid for medium-zoom rendering
    pub fire_grid_fine: FireGrid,
    /// Fallout zones
    pub fallout: Vec<Fallout>,
    /// Total casualties
    pub casualties: u64,
    /// Frame counter for animation randomness
    pub frame: u64,
    /// Last frame when a nuke was launched (for cooldown)
    last_nuke_frame: u64,
    /// Globe spin momentum (angular velocity in radians/frame)
    spin_velocity: (f64, f64),
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
            projection: Projection::Mercator(Viewport::world(pixel_width, pixel_height)),
            map_renderer: MapRenderer::new(),
            should_quit: false,
            last_mouse: None,
            mouse_pos: None,
            explosions: Vec::new(),
            fires: Vec::new(),
            fire_grid: FireGrid::new(1.0),
            fire_grid_fine: FireGrid::new(0.25),
            fallout: Vec::new(),
            casualties: 0,
            frame: 0,
            last_nuke_frame: 0,
            spin_velocity: (0.0, 0.0),
        }
    }

    /// Update viewport size when terminal resizes
    pub fn resize(&mut self, width: usize, height: usize) {
        let inner_width = width.saturating_sub(2);
        let inner_height = height.saturating_sub(3);
        self.projection.set_size(inner_width * 2, inner_height * 4);
    }

    /// Pan the map
    pub fn pan(&mut self, dx: i32, dy: i32) {
        self.projection.pan(dx, dy);
    }

    /// Zoom in
    pub fn zoom_in(&mut self) {
        self.projection.zoom_in();
    }

    /// Zoom out
    pub fn zoom_out(&mut self) {
        self.projection.zoom_out();
    }

    /// Zoom in towards a screen position (terminal column/row)
    pub fn zoom_in_at(&mut self, col: u16, row: u16) {
        let px = ((col.saturating_sub(1)) as i32) * 2;
        let py = ((row.saturating_sub(1)) as i32) * 4;
        self.projection.zoom_in_at(px, py);
    }

    /// Zoom out from a screen position (terminal column/row)
    pub fn zoom_out_at(&mut self, col: u16, row: u16) {
        let px = ((col.saturating_sub(1)) as i32) * 2;
        let py = ((row.saturating_sub(1)) as i32) * 4;
        self.projection.zoom_out_at(px, py);
    }

    /// Request quit
    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    /// Get current zoom level as a string
    pub fn zoom_level(&self) -> String {
        format!("{:.1}x", self.projection.effective_zoom())
    }

    /// Get current center coordinates as a string
    pub fn center_coords(&self) -> String {
        let lat = self.projection.center_lat();
        let lon = self.projection.center_lon();
        format!(
            "{:.1}°{}, {:.1}°{}",
            lat.abs(),
            if lat >= 0.0 { "N" } else { "S" },
            lon.abs(),
            if lon >= 0.0 { "E" } else { "W" }
        )
    }

    /// Get current LOD level as a string
    pub fn lod_level(&self) -> &'static str {
        match Lod::from_zoom(self.projection.effective_zoom()) {
            Lod::Low => "110m",
            Lod::Medium => "50m",
            Lod::High => "10m",
        }
    }

    /// Handle mouse drag
    pub fn handle_drag(&mut self, x: u16, y: u16) {
        if let Some((last_x, last_y)) = self.last_mouse {
            let dx = last_x as i32 - x as i32;
            let dy = last_y as i32 - y as i32;
            let zoom = self.projection.effective_zoom();
            let scale = if zoom < 2.0 { 2 } else if zoom < 4.0 { 3 } else { 4 };
            self.pan(dx * scale, dy * scale);

            // Track angular velocity for globe momentum
            if let Projection::Globe(ref g) = self.projection {
                let ax = (dx * scale) as f64 / g.radius;
                let ay = -(dy * scale) as f64 / g.radius;
                // EMA smoothing to filter jitter from individual mouse events
                self.spin_velocity.0 = self.spin_velocity.0 * 0.5 + ax * 0.5;
                self.spin_velocity.1 = self.spin_velocity.1 * 0.5 + ay * 0.5;
            }
        }
        self.last_mouse = Some((x, y));
    }

    /// Reset drag state when mouse button released — momentum persists
    pub fn end_drag(&mut self) {
        self.last_mouse = None;
    }

    /// Cancel spin momentum (called on new drag start)
    pub fn start_drag(&mut self, x: u16, y: u16) {
        self.spin_velocity = (0.0, 0.0);
        self.last_mouse = Some((x, y));
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
        const NUKE_COOLDOWN_FRAMES: u64 = 15;

        if self.frame < self.last_nuke_frame + NUKE_COOLDOWN_FRAMES {
            return;
        }

        let px = ((col.saturating_sub(1)) as i32) * 2;
        let py = ((row.saturating_sub(1)) as i32) * 4;

        // On globe, clicking outside the sphere does nothing
        let (lon, lat) = match self.projection.unproject(px, py) {
            Some(coords) => coords,
            None => return,
        };

        self.last_nuke_frame = self.frame;

        let radius_km = 50.0 + 700.0 / self.projection.effective_zoom();

        self.explosions.push(Explosion {
            lon,
            lat,
            frame: 0,
            radius_km,
        });

        // Spawn MASSIVE DENSE fire coverage - scale with area, not radius
        // Fire density should be consistent regardless of zoom level
        let area_km2 = std::f64::consts::PI * radius_km * radius_km;
        // Target: ~1 fire per 5km² for dense coverage, cap at 20k fires per blast
        let target_fires = ((area_km2 / 5.0) as usize + 200).min(20000);

        // Pre-allocate to avoid reallocations
        self.fires.reserve(target_fires);

        // Batch generate fires using rejection sampling (faster than individual checks)
        let cos_lat = lat.to_radians().cos().max(0.1);
        let mut spawned = 0;
        let mut attempt = 0;

        while spawned < target_fires && attempt < target_fires * 2 {
            // Generate fire position
            let angle = rand_simple((attempt as u64).wrapping_mul(7919)) * std::f64::consts::TAU;
            let rand_dist = rand_simple((attempt as u64).wrapping_mul(6547));
            let dist = radius_km * rand_dist.sqrt();

            let dlat = (dist * angle.sin()) / 111.0;
            let dlon = (dist * angle.cos()) / (111.0 * cos_lat);

            let fire_lon = lon + dlon;
            let fire_lat = lat + dlat;

            attempt += 1;

            // Fast O(1) land check
            if !self.map_renderer.is_on_land(fire_lon, fire_lat) {
                continue;
            }

            // Vary intensity based on distance from center
            let center_factor = 1.0 - (dist / radius_km);
            let base_intensity = 200.0 + center_factor * 55.0;
            let intensity = (base_intensity + rand_simple((attempt as u64).wrapping_add(1000)) * 30.0).min(255.0) as u8;

            self.fires.push(Fire {
                lon: fire_lon,
                lat: fire_lat,
                intensity,
            });

            spawned += 1;
        }

        // Create fallout zone (larger than blast, persists longer)
        self.fallout.push(Fallout {
            lon,
            lat,
            radius_km: radius_km * 2.0, // Fallout spreads wider than blast
            intensity: 1000, // Lasts ~1000 frames
        });

        // Calculate immediate blast casualties
        self.apply_blast_damage(lon, lat, radius_km);
    }

    /// Apply blast damage to cities within radius
    fn apply_blast_damage(&mut self, lon: f64, lat: f64, radius_km: f64) {
        // Query radius needs to include city sizes too (add max possible city radius ~50km)
        let query_radius_degrees = (radius_km + 50.0) / 111.0;

        // Query spatial grid for cities in expanded radius
        let candidate_indices = self.map_renderer.city_grid.query_radius(lon, lat, query_radius_degrees);

        for &idx in &candidate_indices {
            if let Some(city) = self.map_renderer.city_grid.get_mut(idx) {
                // Skip dead cities early
                if city.population == 0 {
                    continue;
                }

                // Distance from blast center to city center
                let center_dist = fast_distance_km(lon, lat, city.lon, city.lat);

                // Blast affects city if circles overlap: center_dist < blast_radius + city_radius
                let effective_blast_reach = radius_km + city.radius_km;

                if center_dist < effective_blast_reach {
                    // Calculate what portion of city is affected
                    // If blast center is inside city, entire city affected
                    // If partial overlap, proportional damage

                    let killed = if center_dist < city.radius_km {
                        // Blast center inside city = total destruction
                        city.population
                    } else if center_dist < radius_km * 0.3 {
                        // Very close blast = massive casualties
                        let damage_ratio = 1.0 - (center_dist / (radius_km * 0.3)).powi(2);
                        (city.population as f64 * damage_ratio.max(0.8)) as u64
                    } else {
                        // Partial overlap - calculate overlap area ratio
                        // Simplified: use distance-based falloff with city size consideration
                        let normalized_dist = (center_dist - city.radius_km) / radius_km;
                        let damage_ratio = (1.0 - normalized_dist.powi(2)).max(0.0);

                        // More damage to larger cities (more exposed area)
                        let size_factor = (city.radius_km / 10.0).min(2.0); // Up to 2x for large cities
                        (city.population as f64 * damage_ratio * 0.7 * size_factor) as u64
                    };

                    city.population = city.population.saturating_sub(killed);
                    self.casualties += killed;
                }
            }
        }
    }

    /// Update explosion animations, returns true if any are active
    pub fn update_explosions(&mut self) -> bool {
        // Increment global frame counter for randomness
        self.frame = self.frame.wrapping_add(1);

        // Apply globe spin momentum (only when not dragging)
        if self.last_mouse.is_none() {
            let (vx, vy) = self.spin_velocity;
            if vx.abs() > 0.0001 || vy.abs() > 0.0001 {
                if let Projection::Globe(ref mut g) = self.projection {
                    g.apply_momentum(vx, vy);
                }
                // Exponential decay — ~3 seconds to stop at 60fps (0.95^180 ≈ 0.0001)
                self.spin_velocity.0 *= 0.95;
                self.spin_velocity.1 *= 0.95;
            }
        }

        self.explosions.retain_mut(|exp| {
            exp.frame += 1;
            exp.frame < 60 // Animation lasts 60 frames (~1 second at 60fps)
        });

        // Update fires - VERY slow decay and VERY aggressive spreading
        // Pre-allocate for spreading fires (estimate ~15% spread rate × avg 1.5 fires)
        let mut new_fires = Vec::with_capacity(self.fires.len() / 5);
        self.fires.retain_mut(|fire| {
            // VERY SLOW decay - only decay every 5 frames (5x longer fires!)
            if self.frame % 5 == 0 {
                fire.intensity = fire.intensity.saturating_sub(1);
            }

            // VERY aggressive spreading - fires spread like wildfire
            let should_check_spread = fire.intensity > 60;  // Even weak fires spread
            if should_check_spread {
                // Use both lon and lat for unique per-fire randomness
                let lon_bits = (fire.lon * 10000.0).to_bits();
                let lat_bits = (fire.lat * 10000.0).to_bits();
                let rand_val = rand_simple(hash3(lon_bits, lat_bits, self.frame));
                if rand_val > 0.85 {  // Much more frequent spreading (was 0.92)
                    // Spawn 1-3 spread fires per spread event
                    let num_spreads = if rand_simple(hash3(lat_bits, lon_bits, self.frame)) > 0.7 { 2 } else { 1 };

                    for s in 0..num_spreads {
                        // Include frame so each spread event goes a different direction
                        let spread_seed = hash3(lon_bits, lat_bits, self.frame.wrapping_add(s as u64));
                        let spread_dist = 0.03 + rand_simple(spread_seed) * 0.15;
                        let angle = rand_simple(spread_seed.wrapping_mul(31337)) * std::f64::consts::TAU;

                        let new_lon = fire.lon + spread_dist * angle.cos();
                        let new_lat = fire.lat + spread_dist * angle.sin();

                        // Collect all potential spread fires (land check happens later)
                        new_fires.push(Fire {
                            lon: new_lon,
                            lat: new_lat,
                            intensity: fire.intensity.saturating_sub(10),
                        });
                    }
                }
            }

            fire.intensity > 0
        });

        // Filter out fires that would spawn on water (only keep land fires)
        new_fires.retain(|fire| self.map_renderer.is_on_land(fire.lon, fire.lat));

        // Add spread fires (massive limit for apocalyptic infernos)
        // Check cap BEFORE spawning to avoid wasted allocations
        let fires_remaining = 30000_usize.saturating_sub(self.fires.len());
        if fires_remaining > 0 {
            let to_add = new_fires.len().min(fires_remaining);
            self.fires.extend(new_fires.into_iter().take(to_add));
        }

        // Update fallout - decay slowly
        self.fallout.retain_mut(|zone| {
            zone.intensity = zone.intensity.saturating_sub(1);
            zone.intensity > 0
        });

        // Apply ongoing damage every 10 frames (imperceptible skip)
        // Flipped join: iterate cities and probe fire grid, not fires → city query.
        // O(7K cities) with O(1) grid lookups instead of O(25K fires) with HashMap queries.
        if self.frame % 10 == 0 {
            self.apply_fire_damage_to_cities();

            // Fallout damage (few zones, keep the per-zone city query)
            for i in 0..self.fallout.len() {
                let zone = &self.fallout[i];
                if zone.intensity > 0 {
                    let rate = (zone.intensity as f64 / 10000.0) * 0.05;
                    let lon = zone.lon;
                    let lat = zone.lat;
                    let radius_km = zone.radius_km;
                    self.apply_ongoing_damage(lon, lat, radius_km, rate);
                }
            }
        }

        // Rebuild both fire grids: coarse for zoom-out, fine for medium zoom
        self.fire_grid.rebuild(&self.fires);
        self.fire_grid_fine.rebuild(&self.fires);

        !self.explosions.is_empty() || !self.fires.is_empty() || !self.fallout.is_empty()
    }

    /// Flipped join: for each city, probe fire grid neighborhood to check if burning.
    /// O(cities × 9) with flat array lookups vs old O(fires) with HashMap queries.
    /// Scales damage by number of burning cells to match old per-fire behavior.
    fn apply_fire_damage_to_cities(&mut self) {
        let rate = 0.01;
        let res = self.fire_grid_fine.resolution;
        let width = self.fire_grid_fine.width;
        let height = self.fire_grid_fine.height;

        for idx in 0..self.map_renderer.city_grid.len() {
            let (lon, lat, pop) = {
                let city = match self.map_renderer.city_grid.get(idx) {
                    Some(c) => c,
                    None => continue,
                };
                if city.population == 0 {
                    continue;
                }
                (city.lon, city.lat, city.population)
            };

            // Probe 3×3 neighborhood around city — 9 flat array lookups
            let cx = (normalize_lon(lon) / res) as i32;
            let cy = (normalize_lat(lat) / res) as i32;

            let mut fire_cells = 0u32;
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let nx = (cx + dx).clamp(0, width as i32 - 1) as usize;
                    let ny = (cy + dy).clamp(0, height as i32 - 1) as usize;
                    if self.fire_grid_fine.cells[ny * width + nx] > 50 {
                        fire_cells += 1;
                    }
                }
            }

            if fire_cells > 0 {
                // Scale damage by burning cells: matches old per-fire compounding
                let damage = ((pop as f64 * rate * fire_cells as f64) as u64).max(1);
                if let Some(city) = self.map_renderer.city_grid.get_mut(idx) {
                    city.population = city.population.saturating_sub(damage);
                    self.casualties += damage;
                }
            }
        }
    }

    /// Toggle between Mercator and Globe projection
    pub fn toggle_projection(&mut self) {
        let old = std::mem::replace(
            &mut self.projection,
            Projection::Mercator(Viewport::world(1, 1)), // placeholder
        );
        self.projection = old.toggle();
    }

    /// Whether we're in globe mode
    pub fn is_globe(&self) -> bool {
        matches!(self.projection, Projection::Globe(_))
    }

    /// Apply ongoing damage (fire/fallout) - small percentage casualties
    fn apply_ongoing_damage(&mut self, lon: f64, lat: f64, radius_km: f64, rate: f64) {
        // Query radius needs to include city sizes too
        let query_radius_degrees = (radius_km + 50.0) / 111.0;

        // Query spatial grid for cities in expanded radius
        let candidate_indices = self.map_renderer.city_grid.query_radius(lon, lat, query_radius_degrees);

        for &idx in &candidate_indices {
            if let Some(city) = self.map_renderer.city_grid.get_mut(idx) {
                if city.population == 0 {
                    continue;
                }

                let dist = fast_distance_km(lon, lat, city.lon, city.lat);

                // Fire/fallout affects city if circles overlap
                if dist < radius_km + city.radius_km {
                    let damage = ((city.population as f64 * rate) as u64).max(1);
                    city.population = city.population.saturating_sub(damage);
                    self.casualties += damage;
                }
            }
        }
    }

}

/// Fast equirectangular distance approximation in kilometers
/// Good for small distances (<1000km), avoids expensive trig
#[inline(always)]
fn fast_distance_km(lon1: f64, lat1: f64, lon2: f64, lat2: f64) -> f64 {
    const R: f64 = 6371.0; // Earth radius in km
    const DEG_TO_RAD: f64 = 0.017453292519943295; // π/180

    let dlat = (lat2 - lat1) * DEG_TO_RAD;
    let dlon = (lon2 - lon1) * DEG_TO_RAD;

    // Use average latitude for longitude scaling - good enough for game physics
    let lat_avg = (lat1 + lat2) * 0.5 * DEG_TO_RAD;
    let cos_lat = lat_avg.cos();

    let dx = dlon * cos_lat;
    let dy = dlat;

    R * (dx * dx + dy * dy).sqrt()
}

