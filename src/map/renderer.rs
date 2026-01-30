use crate::braille::BrailleCanvas;
use crate::map::geometry::draw_line;
use crate::map::projection::Viewport;

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

/// Map renderer with multi-resolution coastline data
pub struct MapRenderer {
    pub coastlines_low: Vec<LineString>,
    pub coastlines_medium: Vec<LineString>,
    pub coastlines_high: Vec<LineString>,
    pub borders_medium: Vec<LineString>,
    pub borders_high: Vec<LineString>,
    pub states: Vec<LineString>,
    pub counties: Vec<LineString>,
    pub cities: Vec<City>,
    pub settings: DisplaySettings,
}

impl MapRenderer {
    pub fn new() -> Self {
        Self {
            coastlines_low: Vec::new(),
            coastlines_medium: Vec::new(),
            coastlines_high: Vec::new(),
            borders_medium: Vec::new(),
            borders_high: Vec::new(),
            states: Vec::new(),
            counties: Vec::new(),
            cities: Vec::new(),
            settings: DisplaySettings::default(),
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

        // Create separate canvases for each layer
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

        // Collect cities for glyph rendering (viewport-aware filtering with wrapping)
        if self.settings.show_cities {
            // First, collect all visible cities with their screen positions
            // Try each city at 0°, +360°, and -360° longitude offsets
            let mut visible_cities: Vec<(&City, u16, u16)> = self.cities
                .iter()
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
        self.cities.push(City {
            lon,
            lat,
            name: name.to_string(),
            population,
            is_capital,
            is_megacity,
        });
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
