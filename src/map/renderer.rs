use crate::braille::BrailleCanvas;
use crate::map::geometry::draw_line;
use crate::map::globe::{self, GlobeViewport};
use crate::geo::{normalize_lat, normalize_lon};
use crate::map::projection::{Projection, Viewport, WRAP_OFFSETS, mercator_x, mercator_y};
use crate::map::spatial::{FeatureGrid, SpatialGrid};
use std::cell::RefCell;
use std::rc::Rc;

/// Rendered map layers with separate canvases for color differentiation.
/// Static layers use Rc — cache hits are a refcount bump, not a memcpy.
pub struct MapLayers {
    pub coastlines: Rc<BrailleCanvas>,
    pub borders: Rc<BrailleCanvas>,
    pub states: Rc<BrailleCanvas>,
    pub counties: Rc<BrailleCanvas>,
    pub globe_outline: Option<Rc<BrailleCanvas>>,
    pub labels: Vec<(u16, u16, String, f32)>,
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

/// A polygon with exterior ring and optional holes
/// First ring is exterior, subsequent rings are holes
#[derive(Clone)]
pub struct Polygon {
    pub rings: Vec<Vec<(f64, f64)>>,
    pub bbox: (f64, f64, f64, f64), // min_lon, min_lat, max_lon, max_lat
}

impl Polygon {
    pub fn new(rings: Vec<Vec<(f64, f64)>>) -> Self {
        let (mut min_lon, mut max_lon) = (f64::MAX, f64::MIN);
        let (mut min_lat, mut max_lat) = (f64::MAX, f64::MIN);

        for ring in &rings {
            for &(lon, lat) in ring {
                min_lon = min_lon.min(lon);
                max_lon = max_lon.max(lon);
                min_lat = min_lat.min(lat);
                max_lat = max_lat.max(lat);
            }
        }

        Self {
            rings,
            bbox: (min_lon, min_lat, max_lon, max_lat),
        }
    }

}

/// A geographic line (sequence of lon/lat coordinates) with precomputed bounding box
/// and unit-sphere geometry for globe rendering (conservative approximation).
#[derive(Clone)]
pub struct LineString {
    pub bbox: (f64, f64, f64, f64), // min_lon, min_lat, max_lon, max_lat
    /// Precomputed unit-sphere vectors — eliminates trig in globe hot loop.
    /// Amortized O(1) per frame vs O(n) sin/cos calls.
    pub vecs: Vec<globe::DVec3>,
    /// Bounding sphere center on unit sphere for O(1) hemisphere culling.
    /// Single dot product replaces 4× lonlat_to_vec3 + 4 dot products.
    pub center_vec: globe::DVec3,
    /// Feature invisible when center_vec·forward < cull_dot.
    /// Precomputed as -sin(angular_radius + padding).
    pub cull_dot: f64,
    /// Precomputed Mercator coordinates — eliminates trig in Mercator hot loop.
    pub mercator: Vec<(f64, f64)>,
    /// Mercator-space bounding box for trig-free bbox early-out.
    pub mercator_bbox: (f64, f64, f64, f64),
}

impl LineString {
    pub fn new(points: Vec<(f64, f64)>) -> Self {
        let (mut min_lon, mut max_lon) = (f64::MAX, f64::MIN);
        let (mut min_lat, mut max_lat) = (f64::MAX, f64::MIN);
        let (mut merc_min_x, mut merc_max_x) = (f64::MAX, f64::MIN);
        let (mut merc_min_y, mut merc_max_y) = (f64::MAX, f64::MIN);

        let mut mercator = Vec::with_capacity(points.len());

        for &(lon, lat) in &points {
            min_lon = min_lon.min(lon);
            max_lon = max_lon.max(lon);
            min_lat = min_lat.min(lat);
            max_lat = max_lat.max(lat);

            let mx = mercator_x(lon);
            let my = mercator_y(lat);
            merc_min_x = merc_min_x.min(mx);
            merc_max_x = merc_max_x.max(mx);
            merc_min_y = merc_min_y.min(my);
            merc_max_y = merc_max_y.max(my);
            mercator.push((mx, my));
        }

        // Phase 1 (blog: "coverage generation"): precompute unit-sphere vectors
        let vecs: Vec<globe::DVec3> = points.iter()
            .map(|&(lon, lat)| globe::lonlat_to_vec3(lon, lat))
            .collect();

        // Bounding sphere: normalized centroid + max angular distance
        let sum = vecs.iter().copied().fold(globe::DVec3::ZERO, |acc, v| acc + v);
        let center_vec = if sum.length_squared() > 1e-10 {
            sum.normalize()
        } else {
            globe::DVec3::X // fallback for antipodal point sets
        };
        let min_dot = vecs.iter()
            .map(|v| v.dot(center_vec))
            .fold(1.0_f64, f64::min);
        let angular_radius = min_dot.clamp(-1.0, 1.0).acos();
        // Small padding (0.05 rad ≈ 3°) for horizon continuity
        let cull_dot = -(angular_radius + 0.05).sin();

        Self {
            bbox: (min_lon, min_lat, max_lon, max_lat),
            vecs,
            center_vec,
            cull_dot,
            mercator,
            mercator_bbox: (merc_min_x, merc_min_y, merc_max_x, merc_max_y),
        }
    }

    pub fn len(&self) -> usize {
        self.vecs.len()
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
    pub original_population: u64,
    pub is_capital: bool,
    pub is_megacity: bool,
    pub radius_km: f64,
    /// Pre-formatted population string ("1.2M", "500K", etc.)
    /// Updated only when population changes — avoids per-frame format!()
    pub cached_pop_label: String,
}

impl City {
    /// Update population and refresh cached label
    pub fn set_population(&mut self, pop: u64) {
        self.population = pop;
        self.cached_pop_label = format_population(pop);
    }
}

/// Calculate city radius in km from population
/// Based on typical urban density: ~10,000 people/km² for cities
/// Radius = sqrt(population / (density * π))
pub fn city_radius_from_population(population: u64) -> f64 {
    if population == 0 {
        return 0.0;
    }

    // Urban density varies by region, but ~10k/km² is reasonable average
    // Megacities often lower density (~5k/km²) due to sprawl
    let density = if population > 10_000_000 {
        5000.0 // Megacity sprawl
    } else if population > 1_000_000 {
        8000.0 // Large city
    } else if population > 100_000 {
        10000.0 // Medium city
    } else {
        12000.0 // Small city/town (denser)
    };

    // Area = population / density
    // Radius = sqrt(area / π)
    let area_km2 = population as f64 / density;
    (area_km2 / std::f64::consts::PI).sqrt().max(0.5) // At least 0.5km radius
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

/// Cache key for static layer rendering
#[derive(Clone, PartialEq)]
struct RenderCacheKey {
    width: usize,
    height: usize,
    center_lon: i64,  // Quantized to 0.001 degrees
    center_lat: i64,
    zoom: i64,        // Quantized to 0.01
    is_globe: bool,
    show_coastlines: bool,
    show_borders: bool,
    show_states: bool,
    show_counties: bool,
}

impl RenderCacheKey {
    fn new(center_lon: f64, center_lat: f64, zoom: f64, is_globe: bool, width: usize, height: usize, settings: &DisplaySettings) -> Self {
        Self {
            width,
            height,
            center_lon: (center_lon * 1000.0) as i64,
            center_lat: (center_lat * 1000.0) as i64,
            zoom: (zoom * 100.0) as i64,
            is_globe,
            show_coastlines: settings.show_coastlines,
            show_borders: settings.show_borders,
            show_states: settings.show_states,
            show_counties: settings.show_counties,
        }
    }
}

/// Cached static layer renders (Rc-shared with MapLayers)
struct RenderCache {
    key: RenderCacheKey,
    coastlines: Rc<BrailleCanvas>,
    borders: Rc<BrailleCanvas>,
    states: Rc<BrailleCanvas>,
    counties: Rc<BrailleCanvas>,
    globe_outline: Option<Rc<BrailleCanvas>>,
}

/// Fast land/water lookup grid with two-tier conservative approximation.
/// Coarse 1° tier (360×180) classifies cells as all-land/all-water/mixed.
/// Fine 0.1° tier (3600×1800) bitmap provides exact checks for coastal cells.
/// Deep ocean/inland checks skip the fine tier entirely.
pub struct LandGrid {
    bitmap: Vec<u64>,
    /// Coarse 1° tier: 0=all water, 1=mixed, 2=all land
    coarse: Vec<u8>,
}

impl LandGrid {
    const WIDTH: usize = 14400;      // 360° / RESOLUTION
    const HEIGHT: usize = 7200;      // 180° / RESOLUTION
    const RESOLUTION: f64 = 0.025;   // Fine tier: 0.025° per cell (~2.8km)
    const COARSE_RATIO: usize = 40;  // Fine cells per coarse cell (1° / 0.025°)
    const TOTAL_BITS: usize = Self::WIDTH * Self::HEIGHT; // 103,680,000
    const BITMAP_LEN: usize = (Self::TOTAL_BITS + 63) / 64; // ~12.3MB
    /// Cache format version — bump when resolution or layout changes
    const CACHE_VERSION: u32 = 1;

    pub fn new() -> Self {
        Self {
            bitmap: vec![0u64; Self::BITMAP_LEN],
            coarse: vec![0u8; 360 * 180],
        }
    }

    /// Build coarse 1° tier from fine bitmap.
    /// Each 1° cell covers COARSE_RATIO×COARSE_RATIO fine cells; classified as
    /// all-water (0), mixed (1), or all-land (2).
    fn build_coarse(&mut self) {
        let r = Self::COARSE_RATIO;
        let all_land = r * r;
        self.coarse = vec![0u8; 360 * 180];
        for coarse_lat in 0..180usize {
            for coarse_lon in 0..360usize {
                let fine_lat_start = coarse_lat * r;
                let fine_lon_start = coarse_lon * r;
                let land_count = (0..r).flat_map(|fl| {
                    (0..r).map(move |fc| (fl, fc))
                }).filter(|&(fl, fc)| {
                    let fine_idx = (fine_lat_start + fl) * Self::WIDTH + (fine_lon_start + fc);
                    self.get_bit(fine_idx)
                }).count();

                self.coarse[coarse_lat * 360 + coarse_lon] = match land_count {
                    0 => 0,              // all water
                    n if n == all_land => 2, // all land
                    _ => 1,              // mixed - needs fine check
                };
            }
        }
    }

    #[inline(always)]
    fn get_bit(&self, idx: usize) -> bool {
        if idx < Self::TOTAL_BITS {
            (self.bitmap[idx / 64] >> (idx % 64)) & 1 == 1
        } else {
            false
        }
    }

    /// Cache file path keyed by version, polygon count, and total vertex count.
    fn cache_path(poly_count: usize, total_verts: usize) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("tui_map_land_v{}_p{}_e{}.bin",
            Self::CACHE_VERSION, poly_count, total_verts));
        path
    }

    /// Try loading a pre-built grid from disk cache.
    fn try_load_cache(path: &std::path::Path) -> Option<Self> {
        let data = std::fs::read(path).ok()?;
        let expected = Self::BITMAP_LEN * 8 + 360 * 180;
        if data.len() != expected { return None; }

        let bitmap: Vec<u64> = data[..Self::BITMAP_LEN * 8]
            .chunks_exact(8)
            .map(|c| u64::from_le_bytes(c.try_into().unwrap()))
            .collect();
        let coarse = data[Self::BITMAP_LEN * 8..].to_vec();

        Some(Self { bitmap, coarse })
    }

    /// Save grid to disk cache for instant subsequent loads.
    fn save_cache(&self, path: &std::path::Path) {
        let mut data = Vec::with_capacity(Self::BITMAP_LEN * 8 + 360 * 180);
        for &word in &self.bitmap {
            data.extend_from_slice(&word.to_le_bytes());
        }
        data.extend_from_slice(&self.coarse);
        let _ = std::fs::write(path, &data);
    }

    /// Build land grid: loads from disk cache if available, otherwise
    /// builds via scanline rasterization and caches for next startup.
    pub fn from_polygons(polygons: &[Polygon]) -> Self {
        let total_verts: usize = polygons.iter()
            .map(|p| p.rings.iter().map(|r| r.len()).sum::<usize>())
            .sum();
        let cache = Self::cache_path(polygons.len(), total_verts);

        if let Some(grid) = Self::try_load_cache(&cache) {
            return grid;
        }

        let grid = Self::build_scanline(polygons);
        grid.save_cache(&cache);
        grid
    }

    /// Scanline rasterization: for each row, compute edge crossings once
    /// then fill spans between pairs (even-odd rule). O(rows × edges)
    /// vs old brute-force O(cells × edges). Parallelized with rayon.
    pub fn build_scanline(polygons: &[Polygon]) -> Self {
        use rayon::prelude::*;

        let chunk_size = (polygons.len() / rayon::current_num_threads().max(1)).max(1);
        let sub_bitmaps: Vec<Vec<u64>> = polygons.par_chunks(chunk_size)
            .map(|chunk| {
                let mut bitmap = vec![0u64; Self::BITMAP_LEN];
                let mut crossings = Vec::new();
                for polygon in chunk {
                    let (_, min_lat, _, max_lat) = polygon.bbox;
                    let lat_start = (((min_lat + 90.0) / Self::RESOLUTION).floor() as usize).saturating_sub(1);
                    let lat_end = (((max_lat + 90.0) / Self::RESOLUTION).ceil() as usize + 1).min(Self::HEIGHT);

                    for lat_idx in lat_start..lat_end {
                        let lat = -90.0 + (lat_idx as f64 + 0.5) * Self::RESOLUTION;

                        crossings.clear();
                        for ring in &polygon.rings {
                            let n = ring.len();
                            if n < 3 { continue; }
                            for i in 0..n {
                                let j = if i + 1 < n { i + 1 } else { 0 };
                                let (x1, y1) = ring[i];
                                let (x2, y2) = ring[j];
                                if (y1 <= lat && y2 > lat) || (y2 <= lat && y1 > lat) {
                                    let t = (lat - y1) / (y2 - y1);
                                    crossings.push(x1 + t * (x2 - x1));
                                }
                            }
                        }

                        crossings.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

                        for pair in crossings.chunks_exact(2) {
                            let col_start = ((pair[0] + 180.0) / Self::RESOLUTION).ceil() as usize;
                            let col_end = (((pair[1] + 180.0) / Self::RESOLUTION).floor() as usize + 1).min(Self::WIDTH);
                            let row_base = lat_idx * Self::WIDTH;
                            for lon_idx in col_start..col_end {
                                let idx = row_base + lon_idx;
                                if idx < Self::TOTAL_BITS {
                                    bitmap[idx / 64] |= 1u64 << (idx % 64);
                                }
                            }
                        }
                    }
                }
                bitmap
            })
            .collect();

        let mut grid = Self::new();
        for sub in sub_bitmaps {
            for (i, bits) in sub.iter().enumerate() {
                grid.bitmap[i] |= bits;
            }
        }

        grid.build_coarse();
        grid
    }

    /// Smooth land fraction using bilinear interpolation of the 4 neighboring
    /// fine-grid cell centers. Returns 0.0 (water) to 1.0 (land).
    /// At high zoom, this softens fire boundaries at coastlines.
    #[inline(always)]
    pub fn land_fraction(&self, lon: f64, lat: f64) -> f64 {
        let fx = normalize_lon(lon) / Self::RESOLUTION;
        let fy = normalize_lat(lat) / Self::RESOLUTION;

        let x0f = (fx - 0.5).floor();
        let y0f = (fy - 0.5).floor();
        let x0 = (x0f as usize).min(Self::WIDTH - 1);
        let y0 = (y0f as usize).min(Self::HEIGHT - 1);
        let x1 = (x0 + 1).min(Self::WIDTH - 1);
        let y1 = (y0 + 1).min(Self::HEIGHT - 1);

        let tx = fx - 0.5 - x0f;
        let ty = fy - 0.5 - y0f;

        let c00 = self.get_bit(y0 * Self::WIDTH + x0) as u8 as f64;
        let c10 = self.get_bit(y0 * Self::WIDTH + x1) as u8 as f64;
        let c01 = self.get_bit(y1 * Self::WIDTH + x0) as u8 as f64;
        let c11 = self.get_bit(y1 * Self::WIDTH + x1) as u8 as f64;

        let top = c00 * (1.0 - tx) + c10 * tx;
        let bot = c01 * (1.0 - tx) + c11 * tx;
        top * (1.0 - ty) + bot * ty
    }

    /// Two-phase land check: coarse 1° tier short-circuits for deep
    /// ocean/inland, fine 0.025° tier resolves coastal cells.
    #[inline(always)]
    pub fn is_land(&self, lon: f64, lat: f64) -> bool {
        // Phase 1: Coarse 1° check
        let coarse_lon = normalize_lon(lon) as usize;
        let coarse_lat = normalize_lat(lat) as usize;
        let coarse_idx = coarse_lat * 360 + coarse_lon.min(359);

        match self.coarse[coarse_idx] {
            0 => false, // all water - skip fine check
            2 => true,  // all land - skip fine check
            _ => {
                // Phase 2: Fine 0.025° check (coastal cells only)
                let lon_idx = (normalize_lon(lon) / Self::RESOLUTION) as usize;
                let lat_idx = (normalize_lat(lat) / Self::RESOLUTION) as usize;
                let idx = lat_idx.min(Self::HEIGHT - 1) * Self::WIDTH + lon_idx.min(Self::WIDTH - 1);
                self.get_bit(idx)
            }
        }
    }
}

/// Map renderer with multi-resolution coastline data and spatial indexes
pub struct MapRenderer {
    pub coastlines_low: Vec<LineString>,
    pub coastlines_medium: Vec<LineString>,
    pub coastlines_high: Vec<LineString>,
    pub borders_medium: Vec<LineString>,
    pub borders_high: Vec<LineString>,
    pub states: Vec<LineString>,
    pub counties: Vec<LineString>,
    pub land_polygons_low: Vec<Polygon>,
    pub land_polygons_medium: Vec<Polygon>,
    pub land_polygons_high: Vec<Polygon>,
    pub land_grid: Option<LandGrid>,
    pub city_grid: SpatialGrid<City>,
    pub settings: DisplaySettings,
    cache: RefCell<Option<RenderCache>>,
    // Conservative-approximation spatial indexes for O(1) viewport queries
    coastline_grid_low: FeatureGrid,
    coastline_grid_medium: FeatureGrid,
    coastline_grid_high: FeatureGrid,
    border_grid_medium: FeatureGrid,
    border_grid_high: FeatureGrid,
    state_grid: FeatureGrid,
    county_grid: FeatureGrid,
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
            land_polygons_low: Vec::new(),
            land_polygons_medium: Vec::new(),
            land_polygons_high: Vec::new(),
            land_grid: None,
            city_grid: SpatialGrid::new(10.0),
            settings: DisplaySettings::default(),
            cache: RefCell::new(None),
            coastline_grid_low: FeatureGrid::new(5.0),
            coastline_grid_medium: FeatureGrid::new(5.0),
            coastline_grid_high: FeatureGrid::new(5.0),
            border_grid_medium: FeatureGrid::new(5.0),
            border_grid_high: FeatureGrid::new(5.0),
            state_grid: FeatureGrid::new(5.0),
            county_grid: FeatureGrid::new(5.0),
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

    /// Get spatial index for coastlines at given LOD (mirrors get_coastlines fallback)
    fn get_coastline_grid(&self, lod: Lod) -> &FeatureGrid {
        match lod {
            Lod::High => {
                if !self.coastlines_high.is_empty() {
                    &self.coastline_grid_high
                } else if !self.coastlines_medium.is_empty() {
                    &self.coastline_grid_medium
                } else {
                    &self.coastline_grid_low
                }
            }
            Lod::Medium => {
                if !self.coastlines_medium.is_empty() {
                    &self.coastline_grid_medium
                } else {
                    &self.coastline_grid_low
                }
            }
            Lod::Low => &self.coastline_grid_low,
        }
    }

    /// Get spatial index for borders at given LOD (mirrors get_borders fallback)
    fn get_border_grid(&self, lod: Lod) -> &FeatureGrid {
        match lod {
            Lod::High => {
                if !self.borders_high.is_empty() {
                    &self.border_grid_high
                } else {
                    &self.border_grid_medium
                }
            }
            _ => &self.border_grid_medium,
        }
    }

    /// Query a FeatureGrid with date-line wrapping support.
    /// Returns deduplicated feature indices using O(n) bitset instead of O(n log n) sort.
    fn query_grid_wrapped(grid: &FeatureGrid, min_lon: f64, min_lat: f64, max_lon: f64, max_lat: f64) -> Vec<usize> {
        let mut raw = Vec::new();
        grid.query_into(min_lon.max(-180.0), min_lat, max_lon.min(180.0), max_lat, &mut raw);
        if min_lon < -180.0 {
            grid.query_into(min_lon + 360.0, min_lat, 180.0, max_lat, &mut raw);
        }
        if max_lon > 180.0 {
            grid.query_into(-180.0, min_lat, max_lon - 360.0, max_lat, &mut raw);
        }
        // O(n) dedup via bitset — each feature index is dense in [0, num_features)
        let n = grid.num_features();
        if n == 0 {
            return raw;
        }
        let mut seen = vec![0u64; (n + 63) / 64];
        let mut unique = Vec::with_capacity(raw.len().min(n));
        for idx in raw {
            let word = idx / 64;
            let bit = 1u64 << (idx % 64);
            if seen[word] & bit == 0 {
                seen[word] |= bit;
                unique.push(idx);
            }
        }
        unique
    }

    /// Build spatial indexes for all feature collections in parallel.
    /// Order is fixed: the Vec indices match the grid assignments below.
    pub fn build_spatial_indexes(&mut self) {
        use rayon::prelude::*;
        const CELL_SIZE: f64 = 5.0;

        // Collect bboxes upfront so we can release the borrow on self.
        // Order must match the assignment sequence below (0=coast_low, ..., 6=county).
        let bbox_sets: Vec<Vec<(f64, f64, f64, f64)>> = vec![
            self.coastlines_low.iter().map(|l| l.bbox).collect(),
            self.coastlines_medium.iter().map(|l| l.bbox).collect(),
            self.coastlines_high.iter().map(|l| l.bbox).collect(),
            self.borders_medium.iter().map(|l| l.bbox).collect(),
            self.borders_high.iter().map(|l| l.bbox).collect(),
            self.states.iter().map(|l| l.bbox).collect(),
            self.counties.iter().map(|l| l.bbox).collect(),
        ];

        // Build all 7 grids in parallel
        let grids: Vec<FeatureGrid> = bbox_sets
            .into_par_iter()
            .map(|bbs| FeatureGrid::build(bbs.into_iter(), CELL_SIZE))
            .collect();

        let mut grids = grids.into_iter();
        self.coastline_grid_low = grids.next().unwrap();
        self.coastline_grid_medium = grids.next().unwrap();
        self.coastline_grid_high = grids.next().unwrap();
        self.border_grid_medium = grids.next().unwrap();
        self.border_grid_high = grids.next().unwrap();
        self.state_grid = grids.next().unwrap();
        self.county_grid = grids.next().unwrap();
    }

    /// Get max number of cities to show based on zoom
    fn max_cities_for_zoom(zoom: f64) -> usize {
        if zoom > 20.0 {
            1000
        } else if zoom > 15.0 {
            600
        } else if zoom > 10.0 {
            300
        } else if zoom > 6.0 {
            150
        } else if zoom > 4.0 {
            80
        } else if zoom > 3.0 {
            50
        } else if zoom > 2.0 {
            40
        } else {
            30
        }
    }

    /// Render all map features to separate layered canvases
    pub fn render(&self, width: usize, height: usize, projection: &Projection) -> MapLayers {
        match projection {
            Projection::Mercator(viewport) => self.render_mercator(width, height, viewport),
            Projection::Globe(globe) => self.render_globe(width, height, globe),
        }
    }

    /// Mercator render path (existing logic, unchanged)
    fn render_mercator(&self, width: usize, height: usize, viewport: &Viewport) -> MapLayers {
        let lod = Lod::from_zoom(viewport.zoom);
        let mut labels = Vec::new();

        // Viewport geographic bounds (exact Mercator unproject, not linear approx)
        let vp_min_lon = viewport.center_lon - (180.0 / viewport.zoom);
        let vp_max_lon = viewport.center_lon + (180.0 / viewport.zoom);
        let (_, top_lat) = viewport.unproject(0, 0);
        let (_, bottom_lat) = viewport.unproject(0, viewport.height as i32);
        let vp_min_lat = bottom_lat.max(-85.0);
        let vp_max_lat = top_lat.min(85.0);

        // Padded bounds for FeatureGrid queries: match draw_linestring's 50px
        // screen-space padding converted to geographic degrees at current zoom
        let deg_per_px = 360.0 / (viewport.zoom * width as f64 * 2.0);
        let pad = (50.0 * deg_per_px).max(5.0);
        let fg_min_lon = vp_min_lon - pad;
        let fg_max_lon = vp_max_lon + pad;
        let fg_min_lat = (vp_min_lat - pad).max(-90.0);
        let fg_max_lat = (vp_max_lat + pad).min(90.0);

        // Compute wrap offsets once per frame — skip ±360 when viewport doesn't cross dateline.
        // Must use padded bounds (fg_*) not viewport bounds, since the spatial query fetches
        // features in the padded region and draw_linestring needs matching offsets to render them.
        let offsets = Self::needed_wrap_offsets(fg_min_lon, fg_max_lon);

        // Check if we can use cached static layers
        let cache_key = RenderCacheKey::new(viewport.center_lon, viewport.center_lat, viewport.zoom, false, width, height, &self.settings);
        let cache_borrow = self.cache.borrow();
        let use_cache = cache_borrow.as_ref().map(|c| c.key == cache_key).unwrap_or(false);

        let (coastlines_canvas, borders_canvas, states_canvas, counties_canvas, _globe_outline) = if use_cache {
            let cache = cache_borrow.as_ref().unwrap();
            (
                Rc::clone(&cache.coastlines),
                Rc::clone(&cache.borders),
                Rc::clone(&cache.states),
                Rc::clone(&cache.counties),
                cache.globe_outline.as_ref().map(Rc::clone),
            )
        } else {
            drop(cache_borrow);

            let mut coastlines_canvas = BrailleCanvas::new(width, height);
            let mut borders_canvas = BrailleCanvas::new(width, height);
            let mut states_canvas = BrailleCanvas::new(width, height);
            let mut counties_canvas = BrailleCanvas::new(width, height);

            if self.settings.show_coastlines {
                let coastlines = self.get_coastlines(lod);
                let grid = self.get_coastline_grid(lod);
                let candidates = Self::query_grid_wrapped(grid, fg_min_lon, fg_min_lat, fg_max_lon, fg_max_lat);
                for &idx in &candidates {
                    self.draw_linestring(&mut coastlines_canvas, &coastlines[idx], viewport, offsets);
                }
            }

            if self.settings.show_borders {
                let borders = self.get_borders(lod);
                let grid = self.get_border_grid(lod);
                let candidates = Self::query_grid_wrapped(grid, fg_min_lon, fg_min_lat, fg_max_lon, fg_max_lat);
                for &idx in &candidates {
                    self.draw_linestring(&mut borders_canvas, &borders[idx], viewport, offsets);
                }

                if self.settings.show_states && viewport.zoom >= 4.0 {
                    let candidates = Self::query_grid_wrapped(&self.state_grid, fg_min_lon, fg_min_lat, fg_max_lon, fg_max_lat);
                    for &idx in &candidates {
                        self.draw_linestring(&mut states_canvas, &self.states[idx], viewport, offsets);
                    }
                }

                if self.settings.show_counties && viewport.zoom >= 7.0 {
                    let candidates = Self::query_grid_wrapped(&self.county_grid, fg_min_lon, fg_min_lat, fg_max_lon, fg_max_lat);
                    for &idx in &candidates {
                        self.draw_linestring(&mut counties_canvas, &self.counties[idx], viewport, offsets);
                    }
                }
            }

            let coastlines_rc = Rc::new(coastlines_canvas);
            let borders_rc = Rc::new(borders_canvas);
            let states_rc = Rc::new(states_canvas);
            let counties_rc = Rc::new(counties_canvas);

            *self.cache.borrow_mut() = Some(RenderCache {
                key: cache_key,
                coastlines: Rc::clone(&coastlines_rc),
                borders: Rc::clone(&borders_rc),
                states: Rc::clone(&states_rc),
                counties: Rc::clone(&counties_rc),
                globe_outline: None,
            });

            (coastlines_rc, borders_rc, states_rc, counties_rc, None)
        };

        // Collect cities for glyph rendering (viewport-aware filtering with wrapping)
        if self.settings.show_cities {
            let mut candidate_indices = Vec::new();
            candidate_indices.extend(
                self.city_grid.query_bbox(vp_min_lon, vp_min_lat, vp_max_lon, vp_max_lat)
            );
            if vp_min_lon < -180.0 {
                candidate_indices.extend(
                    self.city_grid.query_bbox(vp_min_lon + 360.0, vp_min_lat, 180.0, vp_max_lat)
                );
            }
            if vp_max_lon > 180.0 {
                candidate_indices.extend(
                    self.city_grid.query_bbox(-180.0, vp_min_lat, vp_max_lon - 360.0, vp_max_lat)
                );
            }

            let mut visible_cities: Vec<(&City, u16, u16)> = candidate_indices
                .iter()
                .filter_map(|&idx| self.city_grid.get(idx))
                .flat_map(|city| {
                    WRAP_OFFSETS.iter().filter_map(move |&offset| {
                        let ((px, py), _) = viewport.project_wrapped(city.lon, city.lat, offset);
                        if px < 0 || py < 0 || !viewport.is_visible(px, py) {
                            return None;
                        }
                        Some((city, (px / 2) as u16, (py / 4) as u16))
                    })
                })
                .collect();

            visible_cities.sort_by(|a, b| b.0.original_population.cmp(&a.0.original_population));
            let max_cities = Self::max_cities_for_zoom(viewport.zoom);
            let max_pop = visible_cities.first().map(|(c, _, _)| c.original_population).unwrap_or(1);

            self.collect_city_labels(&mut labels, visible_cities, max_cities, max_pop);
        }

        MapLayers {
            coastlines: coastlines_canvas,
            borders: borders_canvas,
            states: states_canvas,
            counties: counties_canvas,
            globe_outline: None,
            labels,
        }
    }

    /// Globe render path: orthographic projection with great circle subdivision
    fn render_globe(&self, width: usize, height: usize, globe: &GlobeViewport) -> MapLayers {
        let zoom = globe.effective_zoom();
        let lod = Lod::from_zoom(zoom);
        let mut labels = Vec::new();

        let (vp_min_lon, vp_min_lat, vp_max_lon, vp_max_lat) = globe.visible_bounds();

        // Padded bounds for spatial queries
        let pad = 5.0;
        let fg_min_lon = (vp_min_lon - pad).max(-180.0);
        let fg_max_lon = (vp_max_lon + pad).min(180.0);
        let fg_min_lat = (vp_min_lat - pad).max(-90.0);
        let fg_max_lat = (vp_max_lat + pad).min(90.0);

        // Check cache
        let cache_key = RenderCacheKey::new(globe.center_lon(), globe.center_lat(), globe.effective_zoom(), true, width, height, &self.settings);
        let cache_borrow = self.cache.borrow();
        let use_cache = cache_borrow.as_ref().map(|c| c.key == cache_key).unwrap_or(false);

        let (coastlines_canvas, borders_canvas, states_canvas, counties_canvas, globe_outline_rc) = if use_cache {
            let cache = cache_borrow.as_ref().unwrap();
            (
                Rc::clone(&cache.coastlines),
                Rc::clone(&cache.borders),
                Rc::clone(&cache.states),
                Rc::clone(&cache.counties),
                cache.globe_outline.as_ref().map(Rc::clone),
            )
        } else {
            drop(cache_borrow);

            let mut coastlines_canvas = BrailleCanvas::new(width, height);
            let mut borders_canvas = BrailleCanvas::new(width, height);
            let mut states_canvas = BrailleCanvas::new(width, height);
            let mut counties_canvas = BrailleCanvas::new(width, height);

            // No wrap offsets needed for globe — natural wrapping
            if self.settings.show_coastlines {
                let coastlines = self.get_coastlines(lod);
                let grid = self.get_coastline_grid(lod);
                let candidates = Self::query_grid_wrapped(grid, fg_min_lon, fg_min_lat, fg_max_lon, fg_max_lat);
                for &idx in &candidates {
                    self.draw_linestring_globe(&mut coastlines_canvas, &coastlines[idx], globe);
                }
            }

            if self.settings.show_borders {
                let borders = self.get_borders(lod);
                let grid = self.get_border_grid(lod);
                let candidates = Self::query_grid_wrapped(grid, fg_min_lon, fg_min_lat, fg_max_lon, fg_max_lat);
                for &idx in &candidates {
                    self.draw_linestring_globe(&mut borders_canvas, &borders[idx], globe);
                }

                if self.settings.show_states && zoom >= 1.5 {
                    let candidates = Self::query_grid_wrapped(&self.state_grid, fg_min_lon, fg_min_lat, fg_max_lon, fg_max_lat);
                    for &idx in &candidates {
                        self.draw_linestring_globe(&mut states_canvas, &self.states[idx], globe);
                    }
                }

                if self.settings.show_counties && zoom >= 3.5 {
                    let candidates = Self::query_grid_wrapped(&self.county_grid, fg_min_lon, fg_min_lat, fg_max_lon, fg_max_lat);
                    for &idx in &candidates {
                        self.draw_linestring_globe(&mut counties_canvas, &self.counties[idx], globe);
                    }
                }
            }

            // Globe outline — only when sphere edge is visible in viewport
            let globe_outline_rc = if globe.radius < (globe.width.min(globe.height) as f64 / 2.0) {
                let mut outline = BrailleCanvas::new(width, height);
                let cx = globe.width as f64 / 2.0;
                let cy = globe.height as f64 / 2.0;
                let r = globe.radius;
                let circumference = 2.0 * std::f64::consts::PI * r;
                let steps = (circumference * 0.5) as usize; // every other pixel for faintness
                for i in 0..steps {
                    let theta = 2.0 * std::f64::consts::PI * i as f64 / steps as f64;
                    let x = (cx + r * theta.cos()) as usize;
                    let y = (cy - r * theta.sin()) as usize;
                    outline.set_pixel(x, y);
                }
                Some(Rc::new(outline))
            } else {
                None
            };

            let coastlines_rc = Rc::new(coastlines_canvas);
            let borders_rc = Rc::new(borders_canvas);
            let states_rc = Rc::new(states_canvas);
            let counties_rc = Rc::new(counties_canvas);

            *self.cache.borrow_mut() = Some(RenderCache {
                key: cache_key,
                coastlines: Rc::clone(&coastlines_rc),
                borders: Rc::clone(&borders_rc),
                states: Rc::clone(&states_rc),
                counties: Rc::clone(&counties_rc),
                globe_outline: globe_outline_rc.as_ref().map(Rc::clone),
            });

            (coastlines_rc, borders_rc, states_rc, counties_rc, globe_outline_rc)
        };

        // Cities on globe
        if self.settings.show_cities {
            let candidate_indices = self.city_grid.query_bbox(
                vp_min_lon, vp_min_lat, vp_max_lon, vp_max_lat
            );

            let mut visible_cities: Vec<(&City, u16, u16)> = candidate_indices
                .iter()
                .filter_map(|&idx| self.city_grid.get(idx))
                .filter_map(|city| {
                    let (px, py) = globe.project(city.lon, city.lat)?;
                    if !globe.is_visible(px, py) {
                        return None;
                    }
                    Some((city, (px / 2) as u16, (py / 4) as u16))
                })
                .collect();

            visible_cities.sort_by(|a, b| b.0.original_population.cmp(&a.0.original_population));
            let max_cities = Self::max_cities_for_zoom(zoom);
            let max_pop = visible_cities.first().map(|(c, _, _)| c.original_population).unwrap_or(1);

            self.collect_city_labels(&mut labels, visible_cities, max_cities, max_pop);
        }

        MapLayers {
            coastlines: coastlines_canvas,
            borders: borders_canvas,
            states: states_canvas,
            counties: counties_canvas,
            globe_outline: globe_outline_rc,
            labels,
        }
    }

    /// Shared city label collection logic used by both render paths
    fn collect_city_labels(&self, labels: &mut Vec<(u16, u16, String, f32)>, visible_cities: Vec<(&City, u16, u16)>, max_cities: usize, max_pop: u64) {
        for (city, char_x, char_y) in visible_cities.into_iter().take(max_cities) {
            let health = if city.original_population > 0 {
                city.population as f32 / city.original_population as f32
            } else {
                1.0
            };

            let label_y = char_y.saturating_sub(1);

            if city.population == 0 {
                labels.push((char_x, label_y, "☠".to_string(), 0.0));
                if self.settings.show_labels {
                    if let Some(label_x) = char_x.checked_add(1) {
                        let label = if self.settings.show_population {
                            format!(" {} (0)", city.name)
                        } else {
                            format!(" {}", city.name)
                        };
                        labels.push((label_x, label_y, label, 0.0));
                    }
                }
                continue;
            }

            let ratio = city.original_population as f64 / max_pop.max(1) as f64;
            let glyph = if city.is_capital {
                '⚜'
            } else if city.is_megacity || city.population >= 10_000_000 {
                '★'
            } else if ratio > 0.6 || city.population >= 5_000_000 {
                '◆'
            } else if ratio > 0.4 || city.population >= 2_000_000 {
                '■'
            } else if ratio > 0.2 || city.population >= 500_000 {
                '●'
            } else if ratio > 0.1 || city.population >= 100_000 {
                '○'
            } else if city.population >= 20_000 {
                '◦'
            } else {
                '·'
            };

            labels.push((char_x, label_y, glyph.to_string(), health));

            if self.settings.show_labels {
                if let Some(label_x) = char_x.checked_add(1) {
                    let label = if self.settings.show_population {
                        format!(" {} ({})", city.name, city.cached_pop_label)
                    } else {
                        format!(" {}", city.name)
                    };
                    labels.push((label_x, label_y, label, health));
                }
            }
        }
    }

    /// Compute which wrap offsets are needed for this viewport.
    /// Offset 0 always needed; ±360 only when viewport crosses the dateline.
    fn needed_wrap_offsets(vp_min_lon: f64, vp_max_lon: f64) -> &'static [f64] {
        let needs_neg = vp_max_lon > 180.0;  // viewport wraps east → need -360
        let needs_pos = vp_min_lon < -180.0;  // viewport wraps west → need +360
        match (needs_neg, needs_pos) {
            (true, true) => &[0.0, -360.0, 360.0],
            (true, false) => &[0.0, -360.0],
            (false, true) => &[0.0, 360.0],
            (false, false) => &[0.0],
        }
    }

    /// Draw a linestring with viewport culling and world wrapping
    fn draw_linestring(&self, canvas: &mut BrailleCanvas, line: &LineString, viewport: &Viewport, offsets: &[f64]) {
        if line.len() < 2 {
            return;
        }

        for &lon_offset in offsets {
            self.draw_linestring_with_offset(canvas, line, viewport, lon_offset);
        }
    }

    /// Draw a linestring with a longitude offset (for wrapping).
    /// Uses precomputed Mercator coordinates — pure arithmetic, zero trig per vertex.
    fn draw_linestring_with_offset(&self, canvas: &mut BrailleCanvas, line: &LineString, viewport: &Viewport, lon_offset: f64) {
        // Bbox early-out using precomputed Mercator bbox (pure arithmetic, no trig)
        let (merc_min_x, merc_min_y, merc_max_x, merc_max_y) = line.mercator_bbox;
        let (px1, py1) = viewport.project_mercator(merc_min_x, merc_min_y, lon_offset);
        let (px2, py2) = viewport.project_mercator(merc_max_x, merc_max_y, lon_offset);
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

        for &(mx, my) in &line.mercator {
            let (px, py) = viewport.project_mercator(mx, my, lon_offset);

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

    /// Draw a linestring on the globe with great circle subdivision.
    /// Three-phase conservative approximation (à la FloeDB H3 joins):
    ///   Phase 1: Bounding sphere cull — single dot product (O(1) vs 8 trig ops)
    ///   Phase 2: Per-segment back-face skip — 2 dot products
    ///   Phase 3: Slerp + project using precomputed Vec3s — zero trig in hot loop
    fn draw_linestring_globe(&self, canvas: &mut BrailleCanvas, line: &LineString, globe: &GlobeViewport) {
        if line.len() < 2 {
            return;
        }

        // Phase 1: O(1) hemisphere cull via precomputed bounding sphere
        if line.center_vec.dot(globe.forward_vec()) < line.cull_dot {
            return;
        }

        let forward = globe.forward_vec();
        let half_w = globe.width as i32 / 2;
        let mut prev_screen: Option<(i32, i32)> = None;
        let mut prev_vec: Option<globe::DVec3> = None;

        // Phase 3: iterate precomputed unit-sphere vectors (zero lonlat_to_vec3 calls)
        for &cur in &line.vecs {
            if let Some(pv) = prev_vec {
                // Phase 2: skip segments entirely behind the globe
                if cur.dot(forward) < -0.1 && pv.dot(forward) < -0.1 {
                    prev_screen = None;
                    prev_vec = Some(cur);
                    continue;
                }

                let dot = pv.dot(cur).clamp(-1.0, 1.0);

                // Fast path: dot > cos(2°) ≈ 0.9994 means angle < 2°, steps = 1.
                // Skips acos + sin entirely — handles ~95% of segments.
                if dot > 0.9994 {
                    match globe.project_vec3(cur) {
                        Some((px, py)) => {
                            if let Some((prev_x, prev_y)) = prev_screen {
                                let dist = (px - prev_x).abs() + (py - prev_y).abs();
                                if dist < half_w && globe.line_might_be_visible((prev_x, prev_y), (px, py)) {
                                    draw_line(canvas, prev_x, prev_y, px, py);
                                }
                            }
                            prev_screen = Some((px, py));
                        }
                        None => prev_screen = None,
                    }
                } else {
                    // Slow path: large arc — subdivide with slerp
                    let angle = dot.acos();
                    let steps = ((angle.to_degrees() / 2.0).ceil() as usize).max(1);
                    let sin_angle = angle.sin();

                    if sin_angle.abs() < 1e-10 {
                        prev_screen = globe.project_vec3(cur);
                    } else {
                        for i in 1..=steps {
                            let t = i as f64 / steps as f64;
                            let sa = ((1.0 - t) * angle).sin() / sin_angle;
                            let sb = (t * angle).sin() / sin_angle;
                            let p = pv * sa + cur * sb;

                            match globe.project_vec3(p) {
                                Some((px, py)) => {
                                    if let Some((prev_x, prev_y)) = prev_screen {
                                        let dist = (px - prev_x).abs() + (py - prev_y).abs();
                                        if dist < half_w && globe.line_might_be_visible((prev_x, prev_y), (px, py)) {
                                            draw_line(canvas, prev_x, prev_y, px, py);
                                        }
                                    }
                                    prev_screen = Some((px, py));
                                }
                                None => prev_screen = None,
                            }
                        }
                    }
                }
            } else {
                prev_screen = globe.project_vec3(cur);
            }

            prev_vec = Some(cur);
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

    /// Add a city marker
    pub fn add_city(&mut self, lon: f64, lat: f64, name: &str, population: u64, is_capital: bool, is_megacity: bool) {
        let radius_km = city_radius_from_population(population);
        self.city_grid.insert(lon, lat, City {
            lon,
            lat,
            cached_pop_label: format_population(population),
            name: name.to_string(),
            population,
            original_population: population,
            is_capital,
            is_megacity,
            radius_km,
        });
    }

    /// Add land polygon for accurate land/water detection
    pub fn add_land_polygon(&mut self, rings: Vec<Vec<(f64, f64)>>, lod: Lod) {
        let polygon = Polygon::new(rings);
        match lod {
            Lod::Low => self.land_polygons_low.push(polygon),
            Lod::Medium => self.land_polygons_medium.push(polygon),
            Lod::High => self.land_polygons_high.push(polygon),
        }
    }

    /// Build fast land/water lookup grid (call after loading all polygons).
    /// Uses best available polygons; disk-cached for instant subsequent startups.
    pub fn build_land_grid(&mut self) {
        if !self.land_polygons_high.is_empty() {
            self.land_grid = Some(LandGrid::from_polygons(&self.land_polygons_high));
        } else if !self.land_polygons_medium.is_empty() {
            self.land_grid = Some(LandGrid::from_polygons(&self.land_polygons_medium));
        } else if !self.land_polygons_low.is_empty() {
            self.land_grid = Some(LandGrid::from_polygons(&self.land_polygons_low));
        }
    }

    /// Check if a point is on land (O(1) grid lookup)
    #[inline(always)]
    pub fn is_on_land(&self, lon: f64, lat: f64) -> bool {
        if let Some(ref grid) = self.land_grid {
            grid.is_land(lon, lat)
        } else {
            true
        }
    }

    /// Smooth land fraction (0.0–1.0) via bilinear interpolation.
    /// Used at high zoom to fade fires near coastlines.
    #[inline(always)]
    pub fn land_fraction(&self, lon: f64, lat: f64) -> f64 {
        if let Some(ref grid) = self.land_grid {
            grid.land_fraction(lon, lat)
        } else {
            1.0
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn city_set_population_updates_cached_label() {
        let mut city = City {
            lon: 0.0, lat: 0.0,
            name: "Test".to_string(),
            population: 5_000_000,
            original_population: 5_000_000,
            is_capital: false,
            is_megacity: false,
            radius_km: 10.0,
            cached_pop_label: format_population(5_000_000),
        };
        assert_eq!(city.cached_pop_label, "5.0M");

        city.set_population(250_000);
        assert_eq!(city.population, 250_000);
        assert_eq!(city.cached_pop_label, "250K");

        city.set_population(0);
        assert_eq!(city.cached_pop_label, "0");
    }

    #[test]
    fn linestring_len_matches_mercator_coords() {
        let pts = vec![(0.0, 0.0), (10.0, 20.0), (30.0, 40.0)];
        let ls = LineString::new(pts);
        assert_eq!(ls.len(), 3);
        assert_eq!(ls.mercator.len(), 3);
    }

    #[test]
    fn linestring_mercator_bbox_contains_all_points() {
        let pts = vec![(-10.0, -20.0), (30.0, 50.0), (0.0, 0.0)];
        let ls = LineString::new(pts);
        let (min_x, min_y, max_x, max_y) = ls.mercator_bbox;
        for &(mx, my) in &ls.mercator {
            assert!(mx >= min_x && mx <= max_x, "mx {mx} outside [{min_x}, {max_x}]");
            assert!(my >= min_y && my <= max_y, "my {my} outside [{min_y}, {max_y}]");
        }
    }
}
