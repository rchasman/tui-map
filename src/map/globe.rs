use glam::DVec3;
use std::f64::consts::PI;

use crate::map::projection::Viewport;

/// Globe viewport using orthographic projection of a rotating sphere.
/// Orientation stored as a rotation matrix (3 column vectors) for
/// efficient point transformation without quaternion dependency on DQuat.
#[derive(Clone)]
pub struct GlobeViewport {
    /// Forward direction (what points at the camera)
    forward: DVec3,
    /// Right direction
    right: DVec3,
    /// Up direction
    up: DVec3,
    /// Sphere radius in braille pixels (controls zoom)
    pub radius: f64,
    /// Canvas pixel width
    pub width: usize,
    /// Canvas pixel height
    pub height: usize,
}

impl GlobeViewport {
    /// Build a globe viewport centered on (lon, lat) with given radius.
    pub fn new(center_lon: f64, center_lat: f64, radius: f64, width: usize, height: usize) -> Self {
        let lon_rad = center_lon.to_radians();
        let lat_rad = center_lat.to_radians();

        // Forward = direction from origin to (lon, lat) on unit sphere
        let forward = DVec3::new(
            lat_rad.cos() * lon_rad.cos(),
            lat_rad.cos() * lon_rad.sin(),
            lat_rad.sin(),
        );

        // Up = derivative of forward w.r.t. latitude (points north on sphere)
        let raw_up = DVec3::new(
            -lat_rad.sin() * lon_rad.cos(),
            -lat_rad.sin() * lon_rad.sin(),
            lat_rad.cos(),
        );

        // Right = forward × up (points east)
        let right = forward.cross(raw_up).normalize();
        let up = right.cross(forward).normalize();

        Self { forward, right, up, radius, width, height }
    }

    /// Convert current Mercator viewport to globe, preserving center and proportional zoom.
    pub fn from_mercator(vp: &Viewport) -> Self {
        let radius = vp.width as f64 * 0.35 * vp.zoom;
        Self::new(vp.center_lon, vp.center_lat, radius, vp.width, vp.height)
    }

    /// Convert globe back to Mercator viewport, preserving center and zoom.
    pub fn to_mercator(&self) -> Viewport {
        let (lon, lat) = self.center_lonlat();
        let zoom = self.effective_zoom();
        Viewport::new(lon, lat, zoom, self.width, self.height)
    }

    /// Extract the center lon/lat that the globe is looking at.
    fn center_lonlat(&self) -> (f64, f64) {
        let lat = self.forward.z.asin().to_degrees();
        let lon = self.forward.y.atan2(self.forward.x).to_degrees();
        (lon, lat)
    }

    /// Project a geographic point to screen pixels.
    /// Returns `None` for back-face points (behind the visible hemisphere).
    pub fn project(&self, lon: f64, lat: f64) -> Option<(i32, i32)> {
        let p = lonlat_to_vec3(lon, lat);

        // Dot with forward: positive = front-facing
        let depth = p.dot(self.forward);
        if depth < 0.0 {
            return None;
        }

        // Orthographic: project onto right/up plane
        let sx = p.dot(self.right);
        let sy = p.dot(self.up);

        let px = (self.width as f64 / 2.0 + sx * self.radius) as i32;
        let py = (self.height as f64 / 2.0 - sy * self.radius) as i32;

        Some((px, py))
    }

    /// Unproject screen pixels back to lon/lat.
    /// Returns `None` if the point is outside the sphere disk.
    pub fn unproject(&self, px: i32, py: i32) -> Option<(f64, f64)> {
        let sx = (px as f64 - self.width as f64 / 2.0) / self.radius;
        let sy = -(py as f64 - self.height as f64 / 2.0) / self.radius;

        let r2 = sx * sx + sy * sy;
        if r2 > 1.0 {
            return None;
        }

        // Reconstruct 3D point on unit sphere
        let sz = (1.0 - r2).sqrt();
        let p = self.right * sx + self.up * sy + self.forward * sz;

        let lat = p.z.clamp(-1.0, 1.0).asin().to_degrees();
        let lon = p.y.atan2(p.x).to_degrees();

        Some((lon, lat))
    }

    /// Rotate the globe by a pixel drag delta.
    /// Positive dx = dragged left → globe center shifts east (surface follows cursor).
    pub fn rotate_drag(&mut self, dx: i32, dy: i32) {
        let angle_x = (dx as f64) / self.radius;
        let angle_y = -(dy as f64) / self.radius;

        // Rotate around up axis (horizontal drag → longitude change)
        if angle_x.abs() > 1e-10 {
            let (sin_a, cos_a) = angle_x.sin_cos();
            let new_forward = self.forward * cos_a + self.right * sin_a;
            let new_right = self.right * cos_a - self.forward * sin_a;
            self.forward = new_forward.normalize();
            self.right = new_right.normalize();
        }

        // Rotate around right axis (vertical drag → latitude change)
        if angle_y.abs() > 1e-10 {
            let (sin_a, cos_a) = angle_y.sin_cos();
            let new_forward = self.forward * cos_a + self.up * sin_a;
            let new_up = self.up * cos_a - self.forward * sin_a;
            self.forward = new_forward.normalize();
            self.up = new_up.normalize();
        }
    }

    /// Apply angular momentum (radians) — used for inertial spin after drag release.
    pub fn apply_momentum(&mut self, vel_x: f64, vel_y: f64) {
        if vel_x.abs() > 1e-10 {
            let (sin_a, cos_a) = vel_x.sin_cos();
            let new_forward = self.forward * cos_a + self.right * sin_a;
            let new_right = self.right * cos_a - self.forward * sin_a;
            self.forward = new_forward.normalize();
            self.right = new_right.normalize();
        }
        if vel_y.abs() > 1e-10 {
            let (sin_a, cos_a) = vel_y.sin_cos();
            let new_forward = self.forward * cos_a + self.up * sin_a;
            let new_up = self.up * cos_a - self.forward * sin_a;
            self.forward = new_forward.normalize();
            self.up = new_up.normalize();
        }
    }

    /// Zoom in by scaling the sphere radius.
    pub fn zoom_in(&mut self) {
        self.radius = (self.radius * 1.5).min(self.width as f64 * 35.0);
    }

    /// Zoom out by scaling the sphere radius.
    pub fn zoom_out(&mut self) {
        self.radius = (self.radius / 1.5).max(self.width as f64 * 0.35);
    }

    /// Zoom in towards a specific pixel location.
    pub fn zoom_in_at(&mut self, px: i32, py: i32) {
        self.zoom_at(px, py, 1.5);
    }

    /// Zoom out from a specific pixel location.
    pub fn zoom_out_at(&mut self, px: i32, py: i32) {
        self.zoom_at(px, py, 1.0 / 1.5);
    }

    /// Zoom by factor towards a specific pixel, keeping the geographic point under cursor fixed.
    fn zoom_at(&mut self, px: i32, py: i32, factor: f64) {
        // Get geo coords under cursor before zoom
        let target = self.unproject(px, py);

        // Apply zoom
        let min_r = self.width as f64 * 0.35;
        let max_r = self.width as f64 * 35.0;
        self.radius = (self.radius * factor).clamp(min_r, max_r);

        // Re-orient so the same geo point stays under cursor
        if let Some((lon, lat)) = target {
            let target_vec = lonlat_to_vec3(lon, lat);
            // Where does this point project now?
            let sx_now = target_vec.dot(self.right);
            let sy_now = target_vec.dot(self.up);
            // Where should it be (in unit-sphere coords)?
            let sx_want = (px as f64 - self.width as f64 / 2.0) / self.radius;
            let sy_want = -(py as f64 - self.height as f64 / 2.0) / self.radius;

            let dsx = sx_want - sx_now;
            let dsy = sy_want - sy_now;

            // Apply small rotation to re-center
            let angle_x = -dsx;
            let angle_y = dsy;

            if angle_x.abs() > 1e-10 {
                let (sin_a, cos_a) = angle_x.sin_cos();
                let new_forward = self.forward * cos_a + self.right * sin_a;
                let new_right = self.right * cos_a - self.forward * sin_a;
                self.forward = new_forward.normalize();
                self.right = new_right.normalize();
            }
            if angle_y.abs() > 1e-10 {
                let (sin_a, cos_a) = angle_y.sin_cos();
                let new_forward = self.forward * cos_a + self.up * sin_a;
                let new_up = self.up * cos_a - self.forward * sin_a;
                self.forward = new_forward.normalize();
                self.up = new_up.normalize();
            }
        }
    }

    /// Conservative lat/lon bounding box of the visible hemisphere.
    /// Used for spatial index queries. Samples points around the visible disk edge.
    pub fn visible_bounds(&self) -> (f64, f64, f64, f64) {
        let mut min_lon = f64::MAX;
        let mut max_lon = f64::MIN;
        let mut min_lat = f64::MAX;
        let mut max_lat = f64::MIN;

        // Sample the center
        let (clon, clat) = self.center_lonlat();
        min_lon = min_lon.min(clon);
        max_lon = max_lon.max(clon);
        min_lat = min_lat.min(clat);
        max_lat = max_lat.max(clat);

        // Sample 32 points around the visible disk edge
        for i in 0..32 {
            let angle = (i as f64 / 32.0) * 2.0 * PI;
            let sx = angle.cos();
            let sy = angle.sin();

            let p = self.right * sx + self.up * sy;
            let lat = p.z.clamp(-1.0, 1.0).asin().to_degrees();
            let lon = p.y.atan2(p.x).to_degrees();

            min_lon = min_lon.min(lon);
            max_lon = max_lon.max(lon);
            min_lat = min_lat.min(lat);
            max_lat = max_lat.max(lat);
        }

        // If the visible hemisphere spans > 180° longitude, it likely wraps
        // around — return full range to avoid missing features
        if max_lon - min_lon > 180.0 {
            min_lon = -180.0;
            max_lon = 180.0;
        }

        // Check if either pole is visible (forward.z close to ±1 means pole in view)
        if self.forward.z > 0.0 && clat + 90.0 > (90.0 - 1.0) {
            max_lat = 90.0;
        }
        if self.forward.z < 0.0 && clat - 90.0 < (-90.0 + 1.0) {
            min_lat = -90.0;
        }

        (min_lon.max(-180.0), min_lat.max(-90.0), max_lon.min(180.0), max_lat.min(90.0))
    }

    /// Effective zoom level normalized to match Mercator's zoom=1 at world view.
    /// Used for LOD selection, blast radius, fire density, etc.
    pub fn effective_zoom(&self) -> f64 {
        self.radius / (self.width as f64 * 0.35)
    }

    /// Convert degrees to screen pixels for this projection.
    /// Used for explosion/fallout radius rendering.
    pub fn deg_to_pixels(&self, degrees: f64) -> f64 {
        degrees.to_radians() * self.radius
    }

    /// Set viewport dimensions.
    pub fn set_size(&mut self, width: usize, height: usize) {
        self.width = width;
        self.height = height;
    }

    /// Get center longitude.
    pub fn center_lon(&self) -> f64 {
        self.center_lonlat().0
    }

    /// Get center latitude.
    pub fn center_lat(&self) -> f64 {
        self.center_lonlat().1
    }

    /// Check if a projected point is within the viewport.
    pub fn is_visible(&self, px: i32, py: i32) -> bool {
        px >= -10
            && px < self.width as i32 + 10
            && py >= -10
            && py < self.height as i32 + 10
    }

    /// Check if a line segment might be visible (rough bounding box check).
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

/// Convert lon/lat (degrees) to a unit sphere vector.
#[inline(always)]
fn lonlat_to_vec3(lon: f64, lat: f64) -> DVec3 {
    let lon_rad = lon.to_radians();
    let lat_rad = lat.to_radians();
    DVec3::new(
        lat_rad.cos() * lon_rad.cos(),
        lat_rad.cos() * lon_rad.sin(),
        lat_rad.sin(),
    )
}

/// Interpolate along a great circle arc and call a visitor for each subdivision point.
/// Subdivides adaptively: ~2° segments for smooth curves at braille resolution.
/// No allocation — projects each point inline and passes to visitor.
#[inline]
pub fn walk_great_circle(
    lon0: f64, lat0: f64,
    lon1: f64, lat1: f64,
    mut visitor: impl FnMut(f64, f64),
) {
    let a = lonlat_to_vec3(lon0, lat0);
    let b = lonlat_to_vec3(lon1, lat1);

    let dot = a.dot(b).clamp(-1.0, 1.0);
    let angle = dot.acos(); // angular distance in radians

    // ~2° segments
    let steps = ((angle.to_degrees() / 2.0).ceil() as usize).max(1);

    if steps == 1 {
        // Short segment, just emit endpoint
        visitor(lon1, lat1);
        return;
    }

    let sin_angle = angle.sin();
    if sin_angle.abs() < 1e-10 {
        // Points are nearly identical or antipodal
        visitor(lon1, lat1);
        return;
    }

    for i in 1..=steps {
        let t = i as f64 / steps as f64;
        let sa = ((1.0 - t) * angle).sin() / sin_angle;
        let sb = (t * angle).sin() / sin_angle;
        let p = a * sa + b * sb;

        let lat = p.z.clamp(-1.0, 1.0).asin().to_degrees();
        let lon = p.y.atan2(p.x).to_degrees();
        visitor(lon, lat);
    }
}
