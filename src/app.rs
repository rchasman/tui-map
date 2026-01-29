use crate::map::{Lod, MapRenderer, Viewport};

/// Application state
pub struct App {
    pub viewport: Viewport,
    pub map_renderer: MapRenderer,
    pub should_quit: bool,
    /// Last mouse position for drag tracking
    pub last_mouse: Option<(u16, u16)>,
}

impl App {
    pub fn new(width: usize, height: usize) -> Self {
        // Braille gives 2x4 resolution per character
        let pixel_width = width * 2;
        let pixel_height = height * 4;

        Self {
            viewport: Viewport::world(pixel_width, pixel_height),
            map_renderer: MapRenderer::new(),
            should_quit: false,
            last_mouse: None,
        }
    }

    /// Update viewport size when terminal resizes
    pub fn resize(&mut self, width: usize, height: usize) {
        self.viewport.width = width * 2;
        self.viewport.height = height * 4;
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
            // Scale by 2 for more responsive dragging
            self.pan(dx * 2, dy * 2);
        }
        self.last_mouse = Some((x, y));
    }

    /// Reset drag state when mouse button released
    pub fn end_drag(&mut self) {
        self.last_mouse = None;
    }
}
