use crate::braille::BrailleCanvas;
use crate::map::geometry::draw_line;
use crate::map::projection::Viewport;
use crate::map::spatial::SpatialGrid;
use std::cell::RefCell;

/// Rendered map layers with separate canvases for color differentiation
pub struct MapLayers {
    pub coastlines: BrailleCanvas,
    pub borders: BrailleCanvas,
    pub states: BrailleCanvas,
    pub counties: BrailleCanvas,
    pub labels: Vec<(u16, u16, String)>,
}

/// Format population as compact string (e.g., 1.2M, 500K)
fn format_population(pop: u64) -> String {
    if pop >= 1_000_000 {
        format!("{:.1}M", pop as f64 / 1_000_000.0)
    } else if pop >= 1_000 {
        format!("{}K", pop / 1_000)
    } else {
        pop.to_string()
    }
}

/// A polygon with exterior ring and optional holes
/// First ring is exterior, subsequent rings are holes
#[derive(Clone)]
pub struct Polygon {
    pub rings: Vec<Vec<(f64, f64)>>,
    pub bbox: (f64, f64, f64, f64), // min_lon, min_lat, max_lon, max_lat
}

impl Polygon {
    pub fn new(rings: Vec<Vec<(f64, f64)>>) -> Self {
        let (mut min_lon, mut max_lon) = (f64::MAX, f64::MIN);
        let (mut min_lat, mut max_lat) = (f64::MAX, f64::MIN);

        for ring in &rings {
            for &(lon, lat) in ring {
                min_lon = min_lon.min(lon);
                max_lon = max_lon.max(lon);
                min_lat = min_lat.min(lat);
                max_lat = max_lat.max(lat);
            }
        }

        Self {
            rings,
            bbox: (min_lon, min_lat, max_lon, max_lat),
        }
    }

    /// Check if a point is inside this polygon using ray casting algorithm
    pub fn contains(&self, lon: f64, lat: f64) -> bool {
        // Quick bbox check first
        if lon < self.bbox.0 || lon > self.bbox.2 || lat < self.bbox.1 || lat > self.bbox.3 {
            return false;
        }

        if self.rings.is_empty() {
            return false;
        }

        // Check if point is in exterior ring
        let in_exterior = point_in_polygon(lon, lat, &self.rings[0]);

        if !in_exterior {
            return false;
        }

        // Check if point is in any hole (if so, it's not in the polygon)
        for hole in &self.rings[1..] {
            if point_in_polygon(lon, lat, hole) {
                return false;
            }
        }

        true
    }
}

/// Ray casting algorithm for point-in-polygon test
fn point_in_polygon(x: f64, y: f64, ring: &[(f64, f64)]) -> bool {
    let mut inside = false;
    let n = ring.len();

    if n < 3 {
        return false;
    }

    let mut j = n - 1;
    for i in 0..n {
        let xi = ring[i].0;
        let yi = ring[i].1;
        let xj = ring[j].0;
        let yj = ring[j].1;

        if ((yi > y) != (yj > y)) && (x < (xj - xi) * (y - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }

    inside
}

/// A geographic line (sequence of lon/lat coordinates) with precomputed bounding box
#[derive(Clone)]
pub struct LineString {
    pub points: Vec<(f64, f64)>,
    pub bbox: (f64, f64, f64, f64), // min_lon, min_lat, max_lon, max_lat
}

impl LineString {
    pub fn new(points: Vec<(f64, f64)>) -> Self {
        let (mut min_lon, mut max_lon) = (f64::MAX, f64::MIN);
        let (mut min_lat, mut max_lat) = (f64::MAX, f64::MIN);
        for &(lon, lat) in &points {
            min_lon = min_lon.min(lon);
            max_lon = max_lon.max(lon);
            min_lat = min_lat.min(lat);
            max_lat = max_lat.max(lat);
        }
        Self {
            points,
            bbox: (min_lon, min_lat, max_lon, max_lat),
        }
    }

    pub fn len(&self) -> usize {
        self.points.len()
    }
}

/// Level of detail for map data
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Lod {
    Low,    // 110m - world view
    Medium, // 50m - continental
    High,   // 10m - regional
}

impl Lod {
    /// Select LOD based on zoom level
    pub fn from_zoom(zoom: f64) -> Self {
        if zoom < 2.0 {
            Lod::Low
        } else if zoom < 8.0 {
            Lod::Medium
        } else {
            Lod::High
        }
    }
}

/// A city marker with position, name, and metadata
#[derive(Clone)]
pub struct City {
    pub lon: f64,
    pub lat: f64,
    pub name: String,
    pub population: u64,
    pub is_capital: bool,
    pub is_megacity: bool,
    pub radius_km: f64, // Physical city radius based on population
}

/// Calculate city radius in km from population
/// Based on typical urban density: ~10,000 people/km² for cities
/// Radius = sqrt(population / (density * π))
pub fn city_radius_from_population(population: u64) -> f64 {
    if population == 0 {
        return 0.0;
    }

    // Urban density varies by region, but ~10k/km² is reasonable average
    // Megacities often lower density (~5k/km²) due to sprawl
    let density = if population > 10_000_000 {
        5000.0 // Megacity sprawl
    } else if population > 1_000_000 {
        8000.0 // Large city
    } else if population > 100_000 {
        10000.0 // Medium city
    } else {
        12000.0 // Small city/town (denser)
    };

    // Area = population / density
    // Radius = sqrt(area / π)
    let area_km2 = population as f64 / density;
    (area_km2 / std::f64::consts::PI).sqrt().max(0.5) // At least 0.5km radius
}

/// Display settings for map layers
#[derive(Clone)]
pub struct DisplaySettings {
    pub show_coastlines: bool,
    pub show_borders: bool,
    pub show_states: bool,
    pub show_counties: bool,
    pub show_cities: bool,
    pub show_labels: bool,
    pub show_population: bool,
}

impl Default for DisplaySettings {
    fn default() -> Self {
        Self {
            show_coastlines: true,
            show_borders: true,
            show_states: true,
            show_counties: true,
            show_cities: true,
            show_labels: true,
            show_population: false,
        }
    }
}

/// Cache key for static layer rendering
#[derive(Clone, PartialEq)]
struct RenderCacheKey {
    width: usize,
    height: usize,
    center_lon: i64,  // Quantized to 0.001 degrees
    center_lat: i64,
    zoom: i64,        // Quantized to 0.01
    show_coastlines: bool,
    show_borders: bool,
    show_states: bool,
    show_counties: bool,
}

impl RenderCacheKey {
    fn from_viewport(viewport: &Viewport, width: usize, height: usize, settings: &DisplaySettings) -> Self {
        Self {
            width,
            height,
            center_lon: (viewport.center_lon * 1000.0) as i64,
            center_lat: (viewport.center_lat * 1000.0) as i64,
            zoom: (viewport.zoom * 100.0) as i64,
            show_coastlines: settings.show_coastlines,
            show_borders: settings.show_borders,
            show_states: settings.show_states,
            show_counties: settings.show_counties,
        }
    }
}

/// Cached static layer renders
struct RenderCache {
    key: RenderCacheKey,
    coastlines: BrailleCanvas,
    borders: BrailleCanvas,
    states: BrailleCanvas,
    counties: BrailleCanvas,
}

/// Fast land/water lookup grid with two-tier conservative approximation.
/// Coarse 1° tier (360×180) classifies cells as all-land/all-water/mixed.
/// Fine 0.1° tier (3600×1800) bitmap provides exact checks for coastal cells.
/// Deep ocean/inland checks skip the fine tier entirely.
pub struct LandGrid {
    bitmap: Vec<u64>,
    /// Coarse 1° tier: 0=all water, 1=mixed, 2=all land
    coarse: Vec<u8>,
}

impl LandGrid {
    const WIDTH: usize = 3600;  // 360° / 0.1°
    const HEIGHT: usize = 1800; // 180° / 0.1°
    const RESOLUTION: f64 = 0.1;
    const TOTAL_BITS: usize = Self::WIDTH * Self::HEIGHT; // 6,480,000
    const BITMAP_LEN: usize = (Self::TOTAL_BITS + 63) / 64; // 101,250 u64s = 810KB

    pub fn new() -> Self {
        Self {
            bitmap: vec![0u64; Self::BITMAP_LEN],
            coarse: vec![0u8; 360 * 180],
        }
    }

    /// Build coarse 1° tier from fine 0.1° bitmap.
    /// Each 1° cell covers 10×10 fine cells; classified as
    /// all-water (0), mixed (1), or all-land (2).
    fn build_coarse(&mut self) {
        self.coarse = vec![0u8; 360 * 180];
        for coarse_lat in 0..180usize {
            for coarse_lon in 0..360usize {
                let fine_lat_start = coarse_lat * 10;
                let fine_lon_start = coarse_lon * 10;
                let land_count = (0..10usize).flat_map(|fl| {
                    (0..10usize).map(move |fc| (fl, fc))
                }).filter(|&(fl, fc)| {
                    let fine_idx = (fine_lat_start + fl) * Self::WIDTH + (fine_lon_start + fc);
                    self.get_bit(fine_idx)
                }).count();

                self.coarse[coarse_lat * 360 + coarse_lon] = match land_count {
                    0 => 0,     // all water
                    100 => 2,   // all land
                    _ => 1,     // mixed - needs fine check
                };
            }
        }
    }

    #[inline(always)]
    fn set_bit(&mut self, idx: usize) {
        if idx < Self::TOTAL_BITS {
            self.bitmap[idx / 64] |= 1u64 << (idx % 64);
        }
    }

    #[inline(always)]
    fn get_bit(&self, idx: usize) -> bool {
        if idx < Self::TOTAL_BITS {
            (self.bitmap[idx / 64] >> (idx % 64)) & 1 == 1
        } else {
            false
        }
    }

    /// Precompute land grid from polygons (call once at startup)
    pub fn from_polygons(polygons: &[Polygon]) -> Self {
        let mut grid = Self::new();

        // Process each polygon and fill its cells (bbox-optimized)
        for polygon in polygons {
            let (min_lon, min_lat, max_lon, max_lat) = polygon.bbox;

            // Convert bbox to grid indices (with padding for edge cases)
            let lon_start = (((min_lon + 180.0) / Self::RESOLUTION).floor() as usize).saturating_sub(1);
            let lon_end = (((max_lon + 180.0) / Self::RESOLUTION).ceil() as usize + 1).min(Self::WIDTH);
            let lat_start = (((min_lat + 90.0) / Self::RESOLUTION).floor() as usize).saturating_sub(1);
            let lat_end = (((max_lat + 90.0) / Self::RESOLUTION).ceil() as usize + 1).min(Self::HEIGHT);

            // Only check cells within polygon's bounding box
            for lat_idx in lat_start..lat_end {
                let lat = -90.0 + (lat_idx as f64 + 0.5) * Self::RESOLUTION;

                for lon_idx in lon_start..lon_end {
                    let lon = -180.0 + (lon_idx as f64 + 0.5) * Self::RESOLUTION;

                    if polygon.contains(lon, lat) {
                        let idx = lat_idx * Self::WIDTH + lon_idx;
                        grid.set_bit(idx);
                    }
                }
            }
        }

        // Build coarse tier from fine bitmap
        grid.build_coarse();
        grid
    }

    /// Two-phase land check: coarse 1° tier short-circuits for deep
    /// ocean/inland, fine 0.1° tier resolves coastal cells.
    #[inline(always)]
    pub fn is_land(&self, lon: f64, lat: f64) -> bool {
        // Phase 1: Coarse 1° check
        let coarse_lon = ((lon + 180.0).rem_euclid(360.0)) as usize;
        let coarse_lat = ((lat + 90.0).clamp(0.0, 179.999)) as usize;
        let coarse_idx = coarse_lat * 360 + coarse_lon.min(359);

        match self.coarse[coarse_idx] {
            0 => false, // all water - skip fine check
            2 => true,  // all land - skip fine check
            _ => {
                // Phase 2: Fine 0.1° check (coastal cells only)
                let lon_idx = (((lon + 180.0).rem_euclid(360.0)) / Self::RESOLUTION) as usize;
                let lat_idx = (((lat + 90.0).clamp(0.0, 179.999)) / Self::RESOLUTION) as usize;
                let idx = lat_idx.min(Self::HEIGHT - 1) * Self::WIDTH + lon_idx.min(Self::WIDTH - 1);
                self.get_bit(idx)
            }
        }
    }
}

/// Map renderer with multi-resolution coastline data and spatial indexes
pub struct MapRenderer {
    pub coastlines_low: Vec<LineString>,
    pub coastlines_medium: Vec<LineString>,
    pub coastlines_high: Vec<LineString>,
    pub borders_medium: Vec<LineString>,
    pub borders_high: Vec<LineString>,
    pub states: Vec<LineString>,
    pub counties: Vec<LineString>,
    pub land_polygons_low: Vec<Polygon>,
    pub land_polygons_medium: Vec<Polygon>,
    pub land_polygons_high: Vec<Polygon>,
    pub land_grid: Option<LandGrid>,
    pub city_grid: SpatialGrid<City>,
    pub settings: DisplaySettings,
    cache: RefCell<Option<RenderCache>>,
}

impl MapRenderer {
    pub fn new() -> Self {
        // Use 10° cells: 36 columns x 18 rows = 648 cells for the world
        // Good balance between memory and query performance
        Self {
            coastlines_low: Vec::new(),
            coastlines_medium: Vec::new(),
            coastlines_high: Vec::new(),
            borders_medium: Vec::new(),
            borders_high: Vec::new(),
            states: Vec::new(),
            counties: Vec::new(),
            land_polygons_low: Vec::new(),
            land_polygons_medium: Vec::new(),
            land_polygons_high: Vec::new(),
            land_grid: None,
            city_grid: SpatialGrid::new(10.0),
            settings: DisplaySettings::default(),
            cache: RefCell::new(None),
        }
    }

    /// Get coastlines for the given LOD
    fn get_coastlines(&self, lod: Lod) -> &Vec<LineString> {
        match lod {
            Lod::High => {
                if !self.coastlines_high.is_empty() {
                    &self.coastlines_high
                } else if !self.coastlines_medium.is_empty() {
                    &self.coastlines_medium
                } else {
                    &self.coastlines_low
                }
            }
            Lod::Medium => {
                if !self.coastlines_medium.is_empty() {
                    &self.coastlines_medium
                } else {
                    &self.coastlines_low
                }
            }
            Lod::Low => &self.coastlines_low,
        }
    }

    /// Get borders for the given LOD
    fn get_borders(&self, lod: Lod) -> &Vec<LineString> {
        match lod {
            Lod::High => {
                if !self.borders_high.is_empty() {
                    &self.borders_high
                } else {
                    &self.borders_medium
                }
            }
            _ => &self.borders_medium,
        }
    }

    /// Get max number of cities to show based on zoom
    fn max_cities_for_zoom(zoom: f64) -> usize {
        if zoom > 20.0 {
            800
        } else if zoom > 15.0 {
            400
        } else if zoom > 10.0 {
            200
        } else if zoom > 6.0 {
            100
        } else if zoom > 4.0 {
            60
        } else if zoom > 3.0 {
            40
        } else if zoom > 2.0 {
            30
        } else {
            20
        }
    }

    /// Render all map features to separate layered canvases
    pub fn render(&self, width: usize, height: usize, viewport: &Viewport) -> MapLayers {
        let lod = Lod::from_zoom(viewport.zoom);
        let mut labels = Vec::new();

        // Check if we can use cached static layers
        let cache_key = RenderCacheKey::from_viewport(viewport, width, height, &self.settings);
        let cache_borrow = self.cache.borrow();
        let use_cache = cache_borrow.as_ref().map(|c| c.key == cache_key).unwrap_or(false);

        let (coastlines_canvas, borders_canvas, states_canvas, counties_canvas) = if use_cache {
            // Use cached canvases (clone is cheap for braille - just Vec<Vec<u8>>)
            let cache = cache_borrow.as_ref().unwrap();
            (
                cache.coastlines.clone(),
                cache.borders.clone(),
                cache.states.clone(),
                cache.counties.clone(),
            )
        } else {
            drop(cache_borrow); // Release borrow before updating

            // Render static layers from scratch
            let mut coastlines_canvas = BrailleCanvas::new(width, height);
            let mut borders_canvas = BrailleCanvas::new(width, height);
            let mut states_canvas = BrailleCanvas::new(width, height);
            let mut counties_canvas = BrailleCanvas::new(width, height);

            // Draw coastlines (Cyan - base map)
            if self.settings.show_coastlines {
                let coastlines = self.get_coastlines(lod);
                for line in coastlines {
                    self.draw_linestring(&mut coastlines_canvas, line, viewport);
                }
            }

            // Draw country borders (master toggle for all political boundaries)
            if self.settings.show_borders {
                let borders = self.get_borders(lod);
                for line in borders {
                    self.draw_linestring(&mut borders_canvas, line, viewport);
                }

                // Draw state/province borders (sub-toggle, visible at zoom >= 4.0)
                if self.settings.show_states && viewport.zoom >= 4.0 {
                    for line in &self.states {
                        self.draw_linestring(&mut states_canvas, line, viewport);
                    }
                }

                // Draw county borders (sub-toggle, visible at zoom >= 8.0)
                if self.settings.show_counties && viewport.zoom >= 8.0 {
                    for line in &self.counties {
                        self.draw_linestring(&mut counties_canvas, line, viewport);
                    }
                }
            }

            // Update cache
            *self.cache.borrow_mut() = Some(RenderCache {
                key: cache_key,
                coastlines: coastlines_canvas.clone(),
                borders: borders_canvas.clone(),
                states: states_canvas.clone(),
                counties: counties_canvas.clone(),
            });

            (coastlines_canvas, borders_canvas, states_canvas, counties_canvas)
        };

        // Collect cities for glyph rendering (viewport-aware filtering with wrapping)
        if self.settings.show_cities {
            // Calculate viewport bounds in geographic coordinates
            let vp_min_lon = viewport.center_lon - (180.0 / viewport.zoom);
            let vp_max_lon = viewport.center_lon + (180.0 / viewport.zoom);
            let vp_min_lat = (viewport.center_lat - (90.0 / viewport.zoom)).max(-85.0);
            let vp_max_lat = (viewport.center_lat + (90.0 / viewport.zoom)).min(85.0);

            // Query spatial grid for cities in viewport (with wrapping)
            let mut candidate_indices = Vec::new();

            // Normal query
            candidate_indices.extend(
                self.city_grid.query_bbox(vp_min_lon, vp_min_lat, vp_max_lon, vp_max_lat)
            );

            // Wrapped queries (handle date line crossing)
            if vp_min_lon < -180.0 {
                candidate_indices.extend(
                    self.city_grid.query_bbox(vp_min_lon + 360.0, vp_min_lat, 180.0, vp_max_lat)
                );
            }
            if vp_max_lon > 180.0 {
                candidate_indices.extend(
                    self.city_grid.query_bbox(-180.0, vp_min_lat, vp_max_lon - 360.0, vp_max_lat)
                );
            }

            // First, collect all visible cities with their screen positions
            // Try each city at 0°, +360°, and -360° longitude offsets
            let mut visible_cities: Vec<(&City, u16, u16)> = candidate_indices
                .iter()
                .filter_map(|&idx| self.city_grid.get(idx))
                .flat_map(|city| {
                    // Try normal position and wrapped positions
                    [0.0, -360.0, 360.0].iter().filter_map(move |&offset| {
                        let ((px, py), _) = viewport.project_wrapped(city.lon, city.lat, offset);

                        // Early rejection: negative coords or out of bounds
                        if px < 0 || py < 0 || !viewport.is_visible(px, py) {
                            return None;
                        }

                        Some((city, (px / 2) as u16, (py / 4) as u16))
                    })
                })
                .collect();

            // Sort by population descending
            visible_cities.sort_by(|a, b| b.0.population.cmp(&a.0.population));

            // Take top N based on zoom level
            let max_cities = Self::max_cities_for_zoom(viewport.zoom);

            // Find max population in visible set for relative sizing
            let max_pop = visible_cities.first().map(|(c, _, _)| c.population).unwrap_or(1);

            for (city, char_x, char_y) in visible_cities.into_iter().take(max_cities) {
                // Dead city - skull marker
                if city.population == 0 {
                    labels.push((char_x, char_y, "☠".to_string()));
                    if self.settings.show_labels {
                        if let Some(label_x) = char_x.checked_add(2) {
                            // Prefix with ~ to indicate dead city for strikethrough
                            labels.push((label_x, char_y, format!("~{}", city.name)));
                        }
                    }
                    continue;
                }

                // Choose glyph based on city type and relative population
                // Choose glyph based on city type and relative population
                let ratio = city.population as f64 / max_pop.max(1) as f64;
                let glyph = if city.is_capital {
                    '⚜' // National capital - fleur-de-lis
                } else if city.is_megacity || city.population >= 10_000_000 {
                    '★' // Megacity (10M+)
                } else if ratio > 0.6 || city.population >= 5_000_000 {
                    '◆' // Major metro (5M+)
                } else if ratio > 0.4 || city.population >= 2_000_000 {
                    '■' // Large city (2M+)
                } else if ratio > 0.2 || city.population >= 500_000 {
                    '●' // City (500K+)
                } else if ratio > 0.1 || city.population >= 100_000 {
                    '○' // Small city (100K+)
                } else if city.population >= 20_000 {
                    '◦' // Town (20K+)
                } else {
                    '·' // Village
                };

                // Add city marker
                labels.push((char_x, char_y, glyph.to_string()));

                // Add label after marker
                if self.settings.show_labels {
                    if let Some(label_x) = char_x.checked_add(2) {
                        let label = if self.settings.show_population {
                            format!("{} ({})", city.name, format_population(city.population))
                        } else {
                            city.name.clone()
                        };
                        labels.push((label_x, char_y, label));
                    }
                }
            }
        }

        MapLayers {
            coastlines: coastlines_canvas,
            borders: borders_canvas,
            states: states_canvas,
            counties: counties_canvas,
            labels,
        }
    }

    /// Draw a linestring with viewport culling and world wrapping
    fn draw_linestring(&self, canvas: &mut BrailleCanvas, line: &LineString, viewport: &Viewport) {
        if line.len() < 2 {
            return;
        }

        // Draw the linestring at its normal position and potentially wrapped
        // This handles the case where the viewport crosses the date line
        let offsets = [0.0, -360.0, 360.0];

        for &lon_offset in &offsets {
            self.draw_linestring_with_offset(canvas, line, viewport, lon_offset);
        }
    }

    /// Draw a linestring with a longitude offset (for wrapping)
    fn draw_linestring_with_offset(&self, canvas: &mut BrailleCanvas, line: &LineString, viewport: &Viewport, lon_offset: f64) {
        // Quick bounding box check using precomputed bbox with offset
        let (min_lon, min_lat, max_lon, max_lat) = line.bbox;
        let ((px1, py1), _) = viewport.project_wrapped(min_lon, min_lat, lon_offset);
        let ((px2, py2), _) = viewport.project_wrapped(max_lon, max_lat, lon_offset);
        let bb_min_x = px1.min(px2);
        let bb_max_x = px1.max(px2);
        let bb_min_y = py1.min(py2);
        let bb_max_y = py1.max(py2);

        // Skip if bounding box is entirely outside viewport
        if bb_max_x < -50 || bb_min_x > viewport.width as i32 + 50 ||
           bb_max_y < -50 || bb_min_y > viewport.height as i32 + 50 {
            return;
        }

        let mut prev: Option<(i32, i32)> = None;

        for &(lon, lat) in &line.points {
            let ((px, py), _) = viewport.project_wrapped(lon, lat, lon_offset);

            if let Some((prev_x, prev_y)) = prev {
                // Skip drawing if jump is too large (crossing date line within this offset)
                let dx = (px - prev_x).abs();
                let dy = (py - prev_y).abs();
                let dist = (dx + dy) as usize;

                // Only draw if the segment is reasonable and might be visible
                if dist < viewport.width / 2 && viewport.line_might_be_visible((prev_x, prev_y), (px, py)) {
                    draw_line(canvas, prev_x, prev_y, px, py);
                }
            }

            prev = Some((px, py));
        }
    }

    /// Add coastline data at a specific LOD
    pub fn add_coastline(&mut self, points: Vec<(f64, f64)>, lod: Lod) {
        let line = LineString::new(points);
        match lod {
            Lod::Low => self.coastlines_low.push(line),
            Lod::Medium => self.coastlines_medium.push(line),
            Lod::High => self.coastlines_high.push(line),
        }
    }

    /// Add border data at a specific LOD
    pub fn add_border(&mut self, points: Vec<(f64, f64)>, lod: Lod) {
        let line = LineString::new(points);
        match lod {
            Lod::Medium => self.borders_medium.push(line),
            Lod::High => self.borders_high.push(line),
            Lod::Low => self.borders_medium.push(line), // Low uses medium
        }
    }

    /// Add state/province border
    pub fn add_state(&mut self, points: Vec<(f64, f64)>) {
        self.states.push(LineString::new(points));
    }

    /// Add county border
    pub fn add_county(&mut self, points: Vec<(f64, f64)>) {
        self.counties.push(LineString::new(points));
    }

    /// Add a city marker
    pub fn add_city(&mut self, lon: f64, lat: f64, name: &str, population: u64, is_capital: bool, is_megacity: bool) {
        let radius_km = city_radius_from_population(population);
        self.city_grid.insert(lon, lat, City {
            lon,
            lat,
            name: name.to_string(),
            population,
            is_capital,
            is_megacity,
            radius_km,
        });
    }

    /// Add land polygon for accurate land/water detection
    pub fn add_land_polygon(&mut self, rings: Vec<Vec<(f64, f64)>>, lod: Lod) {
        let polygon = Polygon::new(rings);
        match lod {
            Lod::Low => self.land_polygons_low.push(polygon),
            Lod::Medium => self.land_polygons_medium.push(polygon),
            Lod::High => self.land_polygons_high.push(polygon),
        }
    }

    /// Build fast land/water lookup grid (call after loading all polygons)
    pub fn build_land_grid(&mut self) {
        // Use lowest resolution for grid building (faster, good enough for fire filtering)
        if !self.land_polygons_low.is_empty() {
            self.land_grid = Some(LandGrid::from_polygons(&self.land_polygons_low));
        } else if !self.land_polygons_medium.is_empty() {
            self.land_grid = Some(LandGrid::from_polygons(&self.land_polygons_medium));
        }
    }

    /// Check if a point is on land (O(1) grid lookup)
    #[inline(always)]
    pub fn is_on_land(&self, lon: f64, lat: f64) -> bool {
        if let Some(ref grid) = self.land_grid {
            grid.is_land(lon, lat)
        } else {
            // Fallback: assume land if no grid available
            true
        }
    }

    /// Check if any data is loaded
    pub fn has_data(&self) -> bool {
        !self.coastlines_low.is_empty()
            || !self.coastlines_medium.is_empty()
            || !self.coastlines_high.is_empty()
    }

    /// Toggle city labels
    pub fn toggle_labels(&mut self) {
        self.settings.show_labels = !self.settings.show_labels;
    }

    /// Toggle population display
    pub fn toggle_population(&mut self) {
        self.settings.show_population = !self.settings.show_population;
    }

    /// Toggle country borders
    pub fn toggle_borders(&mut self) {
        self.settings.show_borders = !self.settings.show_borders;
    }

    /// Toggle state/province borders
    pub fn toggle_states(&mut self) {
        self.settings.show_states = !self.settings.show_states;
    }

    /// Toggle county borders
    pub fn toggle_counties(&mut self) {
        self.settings.show_counties = !self.settings.show_counties;
    }

    /// Toggle cities
    pub fn toggle_cities(&mut self) {
        self.settings.show_cities = !self.settings.show_cities;
    }
}

impl Default for MapRenderer {
    fn default() -> Self {
        Self::new()
    }
}
