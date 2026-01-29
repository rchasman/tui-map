use crate::braille::BrailleCanvas;
use crate::map::geometry::{draw_circle, draw_line};
use crate::map::projection::Viewport;

/// A geographic line (sequence of lon/lat coordinates)
pub type LineString = Vec<(f64, f64)>;

/// A city marker with position and name
pub struct City {
    pub lon: f64,
    pub lat: f64,
    pub name: String,
}

/// Map renderer that draws geographic features to a braille canvas
pub struct MapRenderer {
    pub coastlines: Vec<LineString>,
    pub borders: Vec<LineString>,
    pub cities: Vec<City>,
}

impl MapRenderer {
    pub fn new() -> Self {
        Self {
            coastlines: Vec::new(),
            borders: Vec::new(),
            cities: Vec::new(),
        }
    }

    /// Render all map features to the canvas
    pub fn render(&self, canvas: &mut BrailleCanvas, viewport: &Viewport) {
        // Draw coastlines
        for line in &self.coastlines {
            self.draw_linestring(canvas, line, viewport);
        }

        // Draw borders (if any)
        for line in &self.borders {
            self.draw_linestring(canvas, line, viewport);
        }

        // Draw city markers at high zoom
        if viewport.zoom > 2.0 {
            for city in &self.cities {
                let (px, py) = viewport.project(city.lon, city.lat);
                if viewport.is_visible(px, py) {
                    let radius = if viewport.zoom > 5.0 { 2 } else { 1 };
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

    /// Add coastline data
    pub fn add_coastline(&mut self, line: LineString) {
        self.coastlines.push(line);
    }

    /// Add border data
    pub fn add_border(&mut self, line: LineString) {
        self.borders.push(line);
    }

    /// Add a city marker
    pub fn add_city(&mut self, lon: f64, lat: f64, name: &str) {
        self.cities.push(City {
            lon,
            lat,
            name: name.to_string(),
        });
    }
}

impl Default for MapRenderer {
    fn default() -> Self {
        Self::new()
    }
}
