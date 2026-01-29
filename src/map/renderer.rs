use crate::braille::BrailleCanvas;
use crate::map::geometry::{draw_circle, draw_line};
use crate::map::projection::Viewport;

/// A geographic line (sequence of lon/lat coordinates)
pub type LineString = Vec<(f64, f64)>;

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

/// A city marker with position and name
pub struct City {
    pub lon: f64,
    pub lat: f64,
    pub name: String,
}

/// Map renderer with multi-resolution coastline data
pub struct MapRenderer {
    pub coastlines_low: Vec<LineString>,    // 110m
    pub coastlines_medium: Vec<LineString>, // 50m
    pub coastlines_high: Vec<LineString>,   // 10m
    pub cities: Vec<City>,
}

impl MapRenderer {
    pub fn new() -> Self {
        Self {
            coastlines_low: Vec::new(),
            coastlines_medium: Vec::new(),
            coastlines_high: Vec::new(),
            cities: Vec::new(),
        }
    }

    /// Get coastlines for the given LOD, falling back to lower resolution if unavailable
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

    /// Render all map features to the canvas
    pub fn render(&self, canvas: &mut BrailleCanvas, viewport: &Viewport) {
        let lod = Lod::from_zoom(viewport.zoom);
        let coastlines = self.get_coastlines(lod);

        // Draw coastlines
        for line in coastlines {
            self.draw_linestring(canvas, line, viewport);
        }

        // Draw city markers at high zoom
        if viewport.zoom > 3.0 {
            for city in &self.cities {
                let (px, py) = viewport.project(city.lon, city.lat);
                if viewport.is_visible(px, py) {
                    let radius = if viewport.zoom > 8.0 { 3 } else if viewport.zoom > 5.0 { 2 } else { 1 };
                    draw_circle(canvas, px, py, radius);
                }
            }
        }
    }

    /// Draw a linestring with viewport culling
    fn draw_linestring(&self, canvas: &mut BrailleCanvas, line: &LineString, viewport: &Viewport) {
        if line.len() < 2 {
            return;
        }

        let mut prev: Option<(i32, i32)> = None;

        for &(lon, lat) in line {
            let (px, py) = viewport.project(lon, lat);

            if let Some((prev_x, prev_y)) = prev {
                // Skip very long lines (likely wrapping around the world)
                let dist = ((px - prev_x).abs() + (py - prev_y).abs()) as usize;
                if dist < viewport.width && viewport.line_might_be_visible((prev_x, prev_y), (px, py)) {
                    draw_line(canvas, prev_x, prev_y, px, py);
                }
            }

            prev = Some((px, py));
        }
    }

    /// Add coastline data at a specific LOD
    pub fn add_coastline(&mut self, line: LineString, lod: Lod) {
        match lod {
            Lod::Low => self.coastlines_low.push(line),
            Lod::Medium => self.coastlines_medium.push(line),
            Lod::High => self.coastlines_high.push(line),
        }
    }

    /// Add a city marker
    pub fn add_city(&mut self, lon: f64, lat: f64, name: &str) {
        self.cities.push(City {
            lon,
            lat,
            name: name.to_string(),
        });
    }

    /// Check if any data is loaded
    pub fn has_data(&self) -> bool {
        !self.coastlines_low.is_empty()
            || !self.coastlines_medium.is_empty()
            || !self.coastlines_high.is_empty()
    }
}

impl Default for MapRenderer {
    fn default() -> Self {
        Self::new()
    }
}
