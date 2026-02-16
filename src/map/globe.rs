pub use glam::DVec3;

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

        let mut globe = Self { forward, right: DVec3::X, up: DVec3::Z, radius, width, height };
        globe.recompute_frame();
        globe
    }

    /// Derive right (east) and up (north) from forward using world Z-axis.
    /// No trig, no lon/lat roundtrip — just two cross products.
    fn recompute_frame(&mut self) {
        let world_z = DVec3::Z;
        let right = world_z.cross(self.forward);
        if right.length_squared() < 1e-10 {
            return; // at a pole — current frame is valid
        }
        self.right = right.normalize();
        self.up = self.forward.cross(self.right).normalize();
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

    /// Project a unit-sphere Vec3 directly to screen pixels.
    /// Skips the lon/lat → Vec3 conversion — use in tight loops.
    #[inline(always)]
    pub fn project_vec3(&self, p: DVec3) -> Option<(i32, i32)> {
        let depth = p.dot(self.forward);
        if depth < 0.0 {
            return None;
        }
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

        // Rotate forward around up axis (horizontal drag → longitude)
        if angle_x.abs() > 1e-10 {
            let (sin_a, cos_a) = angle_x.sin_cos();
            self.forward = (self.forward * cos_a + self.right * sin_a).normalize();
        }

        // Rotate forward around right axis (vertical drag → latitude)
        if angle_y.abs() > 1e-10 {
            let (sin_a, cos_a) = angle_y.sin_cos();
            self.forward = (self.forward * cos_a + self.up * sin_a).normalize();
        }

        self.recompute_frame();
    }

    /// Apply angular momentum (radians) — used for inertial spin after drag release.
    pub fn apply_momentum(&mut self, vel_x: f64, vel_y: f64) {
        if vel_x.abs() > 1e-10 {
            let (sin_a, cos_a) = vel_x.sin_cos();
            self.forward = (self.forward * cos_a + self.right * sin_a).normalize();
        }
        if vel_y.abs() > 1e-10 {
            let (sin_a, cos_a) = vel_y.sin_cos();
            self.forward = (self.forward * cos_a + self.up * sin_a).normalize();
        }

        self.recompute_frame();
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
                self.forward = (self.forward * cos_a + self.right * sin_a).normalize();
            }
            if angle_y.abs() > 1e-10 {
                let (sin_a, cos_a) = angle_y.sin_cos();
                self.forward = (self.forward * cos_a + self.up * sin_a).normalize();
            }

            self.recompute_frame();
        }
    }

    /// Conservative lat/lon bounding box of the visible hemisphere.
    /// Used for spatial index queries. Samples points around the visible disk edge.
    pub fn visible_bounds(&self) -> (f64, f64, f64, f64) {
        // Latitude: analytical from edge z-range.
        // Edge of visible disk: p = right*cos(θ) + up*sin(θ), so
        // p.z = right.z*cos(θ) + up.z*sin(θ), range = ±sqrt(right.z² + up.z²)
        let edge_z = (self.right.z * self.right.z + self.up.z * self.up.z).sqrt();
        let min_lat = (-edge_z).max(-1.0).asin().to_degrees();
        let max_lat = edge_z.min(1.0).asin().to_degrees();

        // Longitude: sample 8 cardinal/ordinal directions on the disk edge.
        // No sin/cos calls — just ±right, ±up, ±(right±up)/√2.
        const INV_SQRT2: f64 = std::f64::consts::FRAC_1_SQRT_2;
        let samples: [DVec3; 9] = [
            self.forward,                                          // center
            self.right,                                            // 0°
            self.right * INV_SQRT2 + self.up * INV_SQRT2,         // 45°
            self.up,                                               // 90°
            self.right * (-INV_SQRT2) + self.up * INV_SQRT2,      // 135°
            -self.right,                                           // 180°
            self.right * (-INV_SQRT2) + self.up * (-INV_SQRT2),   // 225°
            -self.up,                                              // 270°
            self.right * INV_SQRT2 + self.up * (-INV_SQRT2),      // 315°
        ];

        let mut min_lon = f64::MAX;
        let mut max_lon = f64::MIN;
        for p in &samples {
            let lon = p.y.atan2(p.x).to_degrees();
            min_lon = min_lon.min(lon);
            max_lon = max_lon.max(lon);
        }

        // If the visible hemisphere spans > 180° longitude, it wraps
        if max_lon - min_lon > 180.0 {
            min_lon = -180.0;
            max_lon = 180.0;
        }

        // Pole visibility: if forward.z magnitude > cos(edge angular radius),
        // the pole is within the visible hemisphere
        let center_lat = self.forward.z.clamp(-1.0, 1.0).asin().to_degrees();
        let min_lat = if self.forward.z < 0.0 && center_lat - 90.0 < -89.0 { -90.0 } else { min_lat };
        let max_lat = if self.forward.z > 0.0 && center_lat + 90.0 > 89.0 { 90.0 } else { max_lat };

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

    /// Access forward vector for back-face culling.
    #[inline(always)]
    pub fn forward_vec(&self) -> DVec3 {
        self.forward
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
pub fn lonlat_to_vec3(lon: f64, lat: f64) -> DVec3 {
    let lon_rad = lon.to_radians();
    let lat_rad = lat.to_radians();
    DVec3::new(
        lat_rad.cos() * lon_rad.cos(),
        lat_rad.cos() * lon_rad.sin(),
        lat_rad.sin(),
    )
}

