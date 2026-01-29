use crate::map::{Lod, MapRenderer, Viewport};

/// Application state
pub struct App {
    pub viewport: Viewport,
    pub map_renderer: MapRenderer,
    pub should_quit: bool,
    /// Last mouse position for drag tracking
    pub last_mouse: Option<(u16, u16)>,
    /// Current mouse position for cursor marker
    pub mouse_pos: Option<(u16, u16)>,
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
}
