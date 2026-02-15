/// Normalize longitude from [-180, 180] to [0, 360) for grid indexing
#[inline(always)]
pub fn normalize_lon(lon: f64) -> f64 {
    (lon + 180.0).rem_euclid(360.0)
}

/// Normalize latitude from [-90, 90] to [0, 180) for grid indexing
#[inline(always)]
pub fn normalize_lat(lat: f64) -> f64 {
    (lat + 90.0).clamp(0.0, 179.999)
}
