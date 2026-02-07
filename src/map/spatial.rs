use std::collections::HashMap;

/// Spatial hash grid for O(1) region queries
/// Divides world into cells for fast spatial lookups
pub struct SpatialGrid<T> {
    /// Grid cells indexed by (cell_x, cell_y)
    cells: HashMap<(i32, i32), Vec<usize>>,
    /// All items (indices into this vec stored in cells)
    items: Vec<T>,
    /// Cell size in degrees
    cell_size: f64,
}

impl<T> SpatialGrid<T> {
    /// Create a new spatial grid with given cell size in degrees
    pub fn new(cell_size: f64) -> Self {
        Self {
            cells: HashMap::new(),
            items: Vec::new(),
            cell_size,
        }
    }

    /// Convert lon/lat to cell coordinates
    #[inline(always)]
    fn to_cell(&self, lon: f64, lat: f64) -> (i32, i32) {
        let x = (lon / self.cell_size).floor() as i32;
        let y = (lat / self.cell_size).floor() as i32;
        (x, y)
    }

    /// Insert an item at a geographic position
    pub fn insert(&mut self, lon: f64, lat: f64, item: T) {
        let idx = self.items.len();
        self.items.push(item);

        let cell = self.to_cell(lon, lat);
        self.cells.entry(cell).or_insert_with(Vec::new).push(idx);
    }

    /// Query items in a radius around a point (returns indices)
    pub fn query_radius(&self, lon: f64, lat: f64, radius_degrees: f64) -> Vec<usize> {
        let center_cell = self.to_cell(lon, lat);

        // Calculate cell radius to check (round up)
        let cell_radius = (radius_degrees / self.cell_size).ceil() as i32;

        let mut results = Vec::new();

        // Check all cells in the bounding box
        for dy in -cell_radius..=cell_radius {
            for dx in -cell_radius..=cell_radius {
                let cell = (center_cell.0 + dx, center_cell.1 + dy);

                if let Some(indices) = self.cells.get(&cell) {
                    results.extend_from_slice(indices);
                }
            }
        }

        results
    }

    /// Query items in a bounding box (returns indices)
    pub fn query_bbox(&self, min_lon: f64, min_lat: f64, max_lon: f64, max_lat: f64) -> Vec<usize> {
        let min_cell = self.to_cell(min_lon, min_lat);
        let max_cell = self.to_cell(max_lon, max_lat);

        let mut results = Vec::new();

        for y in min_cell.1..=max_cell.1 {
            for x in min_cell.0..=max_cell.0 {
                if let Some(indices) = self.cells.get(&(x, y)) {
                    results.extend_from_slice(indices);
                }
            }
        }

        results
    }

    /// Get item by index
    #[inline(always)]
    pub fn get(&self, idx: usize) -> Option<&T> {
        self.items.get(idx)
    }

    /// Get mutable item by index
    #[inline(always)]
    pub fn get_mut(&mut self, idx: usize) -> Option<&mut T> {
        self.items.get_mut(idx)
    }

    /// Get all items
    #[inline(always)]
    #[allow(dead_code)]
    pub fn items(&self) -> &[T] {
        &self.items
    }

    /// Get all items mutably
    #[inline(always)]
    #[allow(dead_code)]
    pub fn items_mut(&mut self) -> &mut [T] {
        &mut self.items
    }

    /// Number of items
    #[inline(always)]
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Check if empty
    #[inline(always)]
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// Spatial index for geographic features using conservative approximation.
/// Each feature's bounding box is indexed into every cell it overlaps,
/// guaranteeing no false negatives while allowing false positives
/// (eliminated by downstream bbox checks in draw_linestring).
pub struct FeatureGrid {
    cells: HashMap<(i32, i32), Vec<usize>>,
    cell_size: f64,
}

impl FeatureGrid {
    pub fn new(cell_size: f64) -> Self {
        Self {
            cells: HashMap::new(),
            cell_size,
        }
    }

    #[inline(always)]
    fn to_cell(&self, lon: f64, lat: f64) -> (i32, i32) {
        let x = (lon / self.cell_size).floor() as i32;
        let y = (lat / self.cell_size).floor() as i32;
        (x, y)
    }

    /// Build from feature bounding boxes (conservative approximation:
    /// each feature inserted into every cell its bbox overlaps)
    pub fn build(bboxes: impl Iterator<Item = (f64, f64, f64, f64)>, cell_size: f64) -> Self {
        let mut grid = Self::new(cell_size);
        for (idx, (min_lon, min_lat, max_lon, max_lat)) in bboxes.enumerate() {
            let min_cell = grid.to_cell(min_lon, min_lat);
            let max_cell = grid.to_cell(max_lon, max_lat);
            for y in min_cell.1..=max_cell.1 {
                for x in min_cell.0..=max_cell.0 {
                    grid.cells.entry((x, y)).or_default().push(idx);
                }
            }
        }
        grid
    }

    /// Append feature indices for the given bounds into results vec.
    /// May contain duplicates; caller should dedup after all queries.
    pub fn query_into(&self, min_lon: f64, min_lat: f64, max_lon: f64, max_lat: f64, results: &mut Vec<usize>) {
        let min_cell = self.to_cell(min_lon, min_lat);
        let max_cell = self.to_cell(max_lon, max_lat);
        for y in min_cell.1..=max_cell.1 {
            for x in min_cell.0..=max_cell.0 {
                if let Some(indices) = self.cells.get(&(x, y)) {
                    results.extend_from_slice(indices);
                }
            }
        }
    }
}
