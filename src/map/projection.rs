use std::f64::consts::PI;

/// Viewport representing the visible map area and zoom level
#[derive(Clone)]
pub struct Viewport {
    /// Center longitude (-180 to 180)
    pub center_lon: f64,
    /// Center latitude (-90 to 90)
    pub center_lat: f64,
    /// Zoom level (higher = more zoomed in)
    pub zoom: f64,
    /// Canvas pixel width
    pub width: usize,
    /// Canvas pixel height
    pub height: usize,
}

impl Viewport {
    pub fn new(center_lon: f64, center_lat: f64, zoom: f64, width: usize, height: usize) -> Self {
        Self {
            center_lon,
            center_lat,
            zoom,
            width,
            height,
        }
    }

    /// Create a world view (shows entire world)
    pub fn world(width: usize, height: usize) -> Self {
        Self::new(0.0, 20.0, 1.0, width, height)
    }

    /// Pan the viewport by pixel delta
    pub fn pan(&mut self, dx: i32, dy: i32) {
        let scale = 360.0 / (self.zoom * self.width as f64);
        self.center_lon += dx as f64 * scale;
        self.center_lat -= dy as f64 * scale * 0.5; // Mercator distortion

        // Wrap longitude
        if self.center_lon > 180.0 {
            self.center_lon -= 360.0;
        } else if self.center_lon < -180.0 {
            self.center_lon += 360.0;
        }

        // Clamp latitude
        self.center_lat = self.center_lat.clamp(-85.0, 85.0);
    }

    /// Zoom in by a factor
    pub fn zoom_in(&mut self) {
        self.zoom = (self.zoom * 1.5).min(100.0);
    }

    /// Zoom out by a factor
    pub fn zoom_out(&mut self) {
        self.zoom = (self.zoom / 1.5).max(0.5);
    }

    /// Zoom in towards a specific pixel location
    pub fn zoom_in_at(&mut self, px: i32, py: i32) {
        self.zoom_at(px, py, 1.5);
    }

    /// Zoom out from a specific pixel location
    pub fn zoom_out_at(&mut self, px: i32, py: i32) {
        self.zoom_at(px, py, 1.0 / 1.5);
    }

    /// Zoom by factor towards a specific pixel location
    fn zoom_at(&mut self, px: i32, py: i32, factor: f64) {
        // Get the geographic coordinates under the mouse
        let (lon, lat) = self.unproject(px, py);

        // Apply the zoom
        let new_zoom = (self.zoom * factor).clamp(0.5, 100.0);
        self.zoom = new_zoom;

        // Calculate where that point would now project to
        let (new_px, new_py) = self.project(lon, lat);

        // Calculate the offset needed to bring it back under the mouse
        let dx = new_px - px;
        let dy = new_py - py;

        // Pan to compensate
        self.pan(dx, dy);
    }

    /// Unproject pixel coordinates back to geographic coordinates (lon, lat)
    pub fn unproject(&self, px: i32, py: i32) -> (f64, f64) {
        let scale = self.zoom * self.width as f64;

        // Reverse the projection math
        let center_x = (self.center_lon + 180.0) / 360.0;
        let center_lat_rad = self.center_lat * PI / 180.0;
        let center_y = (1.0 - (center_lat_rad.tan() + 1.0 / center_lat_rad.cos()).ln() / PI) / 2.0;

        let x = (px as f64 - self.width as f64 / 2.0) / scale + center_x;
        let y = (py as f64 - self.height as f64 / 2.0) / scale + center_y;

        // Convert from Web Mercator normalized coords back to lon/lat
        let lon = x * 360.0 - 180.0;

        // Inverse Mercator for latitude
        let lat_rad = (PI * (1.0 - 2.0 * y)).sinh().atan();
        let lat = lat_rad * 180.0 / PI;

        (lon, lat)
    }

    /// Project a geographic coordinate (lon, lat) to pixel coordinates
    pub fn project(&self, lon: f64, lat: f64) -> (i32, i32) {
        // Web Mercator projection
        let x = (lon + 180.0) / 360.0;
        let lat_rad = lat * PI / 180.0;
        let y = (1.0 - (lat_rad.tan() + 1.0 / lat_rad.cos()).ln() / PI) / 2.0;

        // Apply zoom and center offset
        let center_x = (self.center_lon + 180.0) / 360.0;
        let center_lat_rad = self.center_lat * PI / 180.0;
        let center_y = (1.0 - (center_lat_rad.tan() + 1.0 / center_lat_rad.cos()).ln() / PI) / 2.0;

        let scale = self.zoom * self.width as f64;

        let px = ((x - center_x) * scale + self.width as f64 / 2.0) as i32;
        let py = ((y - center_y) * scale + self.height as f64 / 2.0) as i32;

        (px, py)
    }

    /// Check if a projected point is visible in the viewport
    pub fn is_visible(&self, px: i32, py: i32) -> bool {
        px >= -10
            && px < self.width as i32 + 10
            && py >= -10
            && py < self.height as i32 + 10
    }

    /// Check if a line segment might be visible (rough bounding box check)
    pub fn line_might_be_visible(&self, p1: (i32, i32), p2: (i32, i32)) -> bool {
        let min_x = p1.0.min(p2.0);
        let max_x = p1.0.max(p2.0);
        let min_y = p1.1.min(p2.1);
        let max_y = p1.1.max(p2.1);

        max_x >= 0
            && min_x < self.width as i32
            && max_y >= 0
            && min_y < self.height as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_project_center() {
        let vp = Viewport::new(0.0, 0.0, 1.0, 100, 100);
        let (x, y) = vp.project(0.0, 0.0);
        assert_eq!(x, 50);
        assert_eq!(y, 50);
    }

    #[test]
    fn test_pan() {
        let mut vp = Viewport::new(0.0, 0.0, 1.0, 100, 100);
        vp.pan(10, 0);
        assert!(vp.center_lon > 0.0);
    }
}
