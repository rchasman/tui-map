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

/// A city marker with position, name, and population
#[derive(Clone)]
pub struct City {
    pub lon: f64,
    pub lat: f64,
    pub name: String,
    pub population: u64,
}

/// Display settings for map layers
#[derive(Clone)]
pub struct DisplaySettings {
    pub show_coastlines: bool,
    pub show_borders: bool,
    pub show_states: bool,
    pub show_cities: bool,
    pub show_labels: bool,
}

impl Default for DisplaySettings {
    fn default() -> Self {
        Self {
            show_coastlines: true,
            show_borders: true,
            show_states: true,
            show_cities: true,
            show_labels: true,
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

    /// Get visible cities based on zoom level (filter by population)
    fn get_visible_cities(&self, zoom: f64) -> impl Iterator<Item = &City> {
        let min_pop = if zoom > 15.0 {
            0 // Show all cities
        } else if zoom > 10.0 {
            50_000
        } else if zoom > 6.0 {
            200_000
        } else if zoom > 4.0 {
            1_000_000
        } else if zoom > 2.0 {
            5_000_000
        } else {
            10_000_000
        };

        self.cities.iter().filter(move |c| c.population >= min_pop)
    }

    /// Render all map features to the canvas
    pub fn render(&self, canvas: &mut BrailleCanvas, viewport: &Viewport) -> Vec<(u16, u16, String)> {
        let lod = Lod::from_zoom(viewport.zoom);
        let mut labels = Vec::new();

        // Draw coastlines
        if self.settings.show_coastlines {
            let coastlines = self.get_coastlines(lod);
            for line in coastlines {
                self.draw_linestring(canvas, line, viewport);
            }
        }

        // Draw borders
        if self.settings.show_borders {
            let borders = self.get_borders(lod);
            for line in borders {
                self.draw_linestring(canvas, line, viewport);
            }
        }

        // Draw state/province borders (only at high zoom)
        if self.settings.show_states && viewport.zoom >= 4.0 {
            for line in &self.states {
                self.draw_linestring(canvas, line, viewport);
            }
        }

        // Draw cities and collect labels
        if self.settings.show_cities && viewport.zoom > 2.0 {
            for city in self.get_visible_cities(viewport.zoom) {
                let (px, py) = viewport.project(city.lon, city.lat);
                if viewport.is_visible(px, py) {
                    // Draw marker
                    let radius = if viewport.zoom > 10.0 {
                        3
                    } else if viewport.zoom > 6.0 {
                        2
                    } else {
                        1
                    };
                    draw_circle(canvas, px, py, radius);

                    // Collect label position (convert braille coords to char coords)
                    if self.settings.show_labels && px >= 0 && py >= 0 {
                        let char_x = (px / 2) as u16;
                        let char_y = (py / 4) as u16;
                        if let Some(label_x) = char_x.checked_add(2) {
                            labels.push((label_x, char_y, city.name.clone()));
                        }
                    }
                }
            }
        }

        labels
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

    /// Add border data at a specific LOD
    pub fn add_border(&mut self, line: LineString, lod: Lod) {
        match lod {
            Lod::Medium => self.borders_medium.push(line),
            Lod::High => self.borders_high.push(line),
            Lod::Low => self.borders_medium.push(line), // Low uses medium
        }
    }

    /// Add state/province border
    pub fn add_state(&mut self, line: LineString) {
        self.states.push(line);
    }

    /// Add a city marker
    pub fn add_city(&mut self, lon: f64, lat: f64, name: &str, population: u64) {
        self.cities.push(City {
            lon,
            lat,
            name: name.to_string(),
            population,
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

    /// Toggle borders
    pub fn toggle_borders(&mut self) {
        self.settings.show_borders = !self.settings.show_borders;
    }

    /// Toggle state/province borders
    pub fn toggle_states(&mut self) {
        self.settings.show_states = !self.settings.show_states;
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
