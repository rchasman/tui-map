use crate::map::{LineString, Lod, MapRenderer};
use anyhow::Result;
use geojson::{GeoJson, Geometry, Value};
use rayon::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};

/// Parse GeoJSON using SIMD-accelerated JSON parsing
fn parse_geojson(content: String) -> Result<GeoJson> {
    let mut bytes = content.into_bytes();
    Ok(simd_json::serde::from_slice(&mut bytes)?)
}

/// Intermediate city data extracted during parallel parsing
struct CityData {
    lon: f64,
    lat: f64,
    name: String,
    population: u64,
    is_capital: bool,
    is_megacity: bool,
}

/// What kind of geometry a file contains and where it goes
enum FileKind {
    Coastline(Lod),
    Border(Lod),
    State,
    County,
    City,
    LandPolygon(Lod),
}

/// Result of loading + parsing a single file in parallel
enum LoadResult {
    Lines(Vec<LineString>, FileKind),
    Polygons(Vec<Vec<Vec<(f64, f64)>>>, Lod),
    Cities(Vec<CityData>),
    Failed(String, String), // filename, error
}

/// Load a single file and parse its geometries (no renderer dependency)
fn load_file(path: &Path, kind: FileKind) -> LoadResult {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => return LoadResult::Failed(
            path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            e.to_string(),
        ),
    };
    let geojson: GeoJson = match parse_geojson(content) {
        Ok(g) => g,
        Err(e) => return LoadResult::Failed(
            path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            e.to_string(),
        ),
    };

    match kind {
        FileKind::City => {
            let cities = extract_cities(&geojson);
            LoadResult::Cities(cities)
        }
        FileKind::LandPolygon(lod) => {
            let mut polygons = Vec::new();
            process_geojson_polygons(&geojson, |p| polygons.push(p));
            LoadResult::Polygons(polygons, lod)
        }
        _ => {
            let mut lines = Vec::new();
            process_geojson_lines(&geojson, |pts| lines.push(LineString::new(pts)));
            LoadResult::Lines(lines, kind)
        }
    }
}

/// Extract city data from parsed GeoJSON
fn extract_cities(geojson: &GeoJson) -> Vec<CityData> {
    let mut cities = Vec::new();
    if let GeoJson::FeatureCollection(fc) = geojson {
        for feature in &fc.features {
            let props = feature.properties.as_ref();

            let name = props
                .and_then(|p| p.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown")
                .to_string();

            let population = props
                .and_then(|p| {
                    p.get("pop_max")
                        .or_else(|| p.get("pop_min"))
                        .or_else(|| p.get("population"))
                })
                .and_then(|v| v.as_f64())
                .map(|v| v as u64)
                .unwrap_or(0);

            let is_capital = props
                .and_then(|p| p.get("adm0cap"))
                .and_then(|v| v.as_f64())
                .map(|v| v >= 1.0)
                .unwrap_or(false);

            let is_megacity = props
                .and_then(|p| p.get("megacity"))
                .and_then(|v| v.as_f64())
                .map(|v| v >= 1.0)
                .unwrap_or(false);

            if let Some(ref geometry) = feature.geometry {
                if let Value::Point(ref coords) = geometry.value {
                    if coords.len() >= 2 {
                        cities.push(CityData {
                            lon: coords[0],
                            lat: coords[1],
                            name,
                            population,
                            is_capital,
                            is_megacity,
                        });
                    }
                }
            }
        }
    }
    cities
}

/// Load all available Natural Earth GeoJSON data into the map renderer
pub fn load_all_geojson(renderer: &mut MapRenderer, data_dir: &Path) -> Result<()> {
    // Collect all file tasks
    let mut tasks: Vec<(PathBuf, FileKind)> = Vec::new();

    // Coastlines
    for (filename, lod) in [
        ("ne_110m_coastline.json", Lod::Low),
        ("natural-earth.json", Lod::Medium),
        ("ne_50m_coastline.json", Lod::Medium),
        ("ne_10m_coastline.json", Lod::High),
    ] {
        let path = data_dir.join(filename);
        if path.exists() {
            tasks.push((path, FileKind::Coastline(lod)));
        }
    }

    // Borders
    for (filename, lod) in [
        ("ne_50m_borders.json", Lod::Medium),
        ("ne_10m_borders.json", Lod::High),
    ] {
        let path = data_dir.join(filename);
        if path.exists() {
            tasks.push((path, FileKind::Border(lod)));
        }
    }

    // States
    let states_path = data_dir.join("ne_10m_states.json");
    if states_path.exists() {
        tasks.push((states_path, FileKind::State));
    }

    // Counties (NE + GADM)
    let counties_path = data_dir.join("ne_10m_admin_2_counties.json");
    if counties_path.exists() {
        tasks.push((counties_path, FileKind::County));
    }
    if let Ok(entries) = fs::read_dir(data_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with("gadm41_") && name.ends_with("_2.json") {
                    tasks.push((path, FileKind::County));
                }
            }
        }
    }

    // Cities
    let cities_path = data_dir.join("ne_10m_cities.json");
    if cities_path.exists() {
        tasks.push((cities_path, FileKind::City));
    }

    // Land polygons
    for (filename, lod) in [
        ("ne_110m_land.json", Lod::Low),
        ("ne_50m_land.json", Lod::Medium),
        ("ne_10m_land.json", Lod::High),
    ] {
        let path = data_dir.join(filename);
        if path.exists() {
            tasks.push((path, FileKind::LandPolygon(lod)));
        }
    }

    // Load + parse all files in parallel
    let results: Vec<LoadResult> = tasks
        .into_par_iter()
        .map(|(path, kind)| load_file(&path, kind))
        .collect();

    // Merge results sequentially into renderer (just pushing to Vecs — fast)
    for result in results {
        match result {
            LoadResult::Lines(lines, kind) => {
                match kind {
                    FileKind::Coastline(lod) => {
                        for line in lines {
                            match lod {
                                Lod::Low => renderer.coastlines_low.push(line),
                                Lod::Medium => renderer.coastlines_medium.push(line),
                                Lod::High => renderer.coastlines_high.push(line),
                            }
                        }
                    }
                    FileKind::Border(lod) => {
                        for line in lines {
                            match lod {
                                Lod::Medium | Lod::Low => renderer.borders_medium.push(line),
                                Lod::High => renderer.borders_high.push(line),
                            }
                        }
                    }
                    FileKind::State => renderer.states.extend(lines),
                    FileKind::County => renderer.counties.extend(lines),
                    _ => {}
                }
            }
            LoadResult::Polygons(polygons, lod) => {
                for rings in polygons {
                    renderer.add_land_polygon(rings, lod);
                }
            }
            LoadResult::Cities(cities) => {
                for city in cities {
                    renderer.add_city(
                        city.lon, city.lat, &city.name,
                        city.population, city.is_capital, city.is_megacity,
                    );
                }
            }
            LoadResult::Failed(filename, error) => {
                eprintln!("Warning: Failed to load {}: {}", filename, error);
            }
        }
    }

    Ok(())
}

/// Process GeoJSON and extract line features
fn process_geojson_lines<F>(geojson: &GeoJson, mut add_line: F)
where
    F: FnMut(Vec<(f64, f64)>),
{
    match geojson {
        GeoJson::FeatureCollection(fc) => {
            for feature in &fc.features {
                if let Some(ref geometry) = feature.geometry {
                    process_geometry_lines(geometry, &mut add_line);
                }
            }
        }
        GeoJson::Feature(f) => {
            if let Some(ref geometry) = f.geometry {
                process_geometry_lines(geometry, &mut add_line);
            }
        }
        GeoJson::Geometry(geometry) => {
            process_geometry_lines(geometry, &mut add_line);
        }
    }
}

fn process_geometry_lines<F>(geometry: &Geometry, add_line: &mut F)
where
    F: FnMut(Vec<(f64, f64)>),
{
    match &geometry.value {
        Value::LineString(coords) => {
            let line: Vec<(f64, f64)> = coords.iter().map(|c| (c[0], c[1])).collect();
            add_line(line);
        }
        Value::MultiLineString(lines) => {
            for coords in lines {
                let line: Vec<(f64, f64)> = coords.iter().map(|c| (c[0], c[1])).collect();
                add_line(line);
            }
        }
        Value::Polygon(rings) => {
            if let Some(exterior) = rings.first() {
                let line: Vec<(f64, f64)> = exterior.iter().map(|c| (c[0], c[1])).collect();
                add_line(line);
            }
        }
        Value::MultiPolygon(polygons) => {
            for rings in polygons {
                if let Some(exterior) = rings.first() {
                    let line: Vec<(f64, f64)> = exterior.iter().map(|c| (c[0], c[1])).collect();
                    add_line(line);
                }
            }
        }
        Value::GeometryCollection(geometries) => {
            for g in geometries {
                process_geometry_lines(g, add_line);
            }
        }
        _ => {}
    }
}

/// Process GeoJSON and extract polygon features
fn process_geojson_polygons<F>(geojson: &GeoJson, mut add_polygon: F)
where
    F: FnMut(Vec<Vec<(f64, f64)>>),
{
    match geojson {
        GeoJson::FeatureCollection(fc) => {
            for feature in &fc.features {
                if let Some(ref geometry) = feature.geometry {
                    process_geometry_polygons(geometry, &mut add_polygon);
                }
            }
        }
        GeoJson::Feature(f) => {
            if let Some(ref geometry) = f.geometry {
                process_geometry_polygons(geometry, &mut add_polygon);
            }
        }
        GeoJson::Geometry(geometry) => {
            process_geometry_polygons(geometry, &mut add_polygon);
        }
    }
}

fn process_geometry_polygons<F>(geometry: &Geometry, add_polygon: &mut F)
where
    F: FnMut(Vec<Vec<(f64, f64)>>),
{
    match &geometry.value {
        Value::Polygon(rings) => {
            let polygon: Vec<Vec<(f64, f64)>> = rings
                .iter()
                .map(|ring| ring.iter().map(|c| (c[0], c[1])).collect())
                .collect();
            add_polygon(polygon);
        }
        Value::MultiPolygon(polygons) => {
            for rings in polygons {
                let polygon: Vec<Vec<(f64, f64)>> = rings
                    .iter()
                    .map(|ring| ring.iter().map(|c| (c[0], c[1])).collect())
                    .collect();
                add_polygon(polygon);
            }
        }
        Value::GeometryCollection(geometries) => {
            for g in geometries {
                process_geometry_polygons(g, add_polygon);
            }
        }
        _ => {}
    }
}

/// Generate a simple world map outline for when no data file is available
pub fn generate_simple_world(renderer: &mut MapRenderer) {
    // Simplified continent outlines (used as Low LOD fallback)
    renderer.add_coastline(
        vec![
            (-168.0, 65.0), (-166.0, 60.0), (-141.0, 60.0), (-130.0, 55.0),
            (-125.0, 48.0), (-124.0, 40.0), (-117.0, 32.0), (-110.0, 25.0),
            (-97.0, 25.0), (-97.0, 28.0), (-82.0, 24.0), (-80.0, 25.0),
            (-81.0, 31.0), (-75.0, 35.0), (-70.0, 41.0), (-67.0, 45.0),
            (-65.0, 47.0), (-55.0, 47.0), (-52.0, 47.0), (-55.0, 52.0),
            (-58.0, 55.0), (-64.0, 60.0), (-73.0, 62.0), (-80.0, 63.0),
            (-95.0, 62.0), (-110.0, 68.0), (-130.0, 70.0), (-145.0, 70.0),
            (-168.0, 65.0),
        ],
        Lod::Low,
    );

    renderer.add_coastline(
        vec![
            (-80.0, 10.0), (-75.0, 5.0), (-70.0, 5.0), (-60.0, 5.0),
            (-50.0, 0.0), (-35.0, -5.0), (-35.0, -10.0), (-38.0, -15.0),
            (-40.0, -22.0), (-48.0, -25.0), (-55.0, -34.0), (-58.0, -38.0),
            (-65.0, -42.0), (-68.0, -50.0), (-75.0, -52.0), (-75.0, -45.0),
            (-72.0, -40.0), (-72.0, -30.0), (-70.0, -20.0), (-70.0, -15.0),
            (-80.0, -5.0), (-80.0, 0.0), (-80.0, 10.0),
        ],
        Lod::Low,
    );

    renderer.add_coastline(
        vec![
            (-10.0, 36.0), (-5.0, 36.0), (0.0, 38.0), (5.0, 43.0),
            (10.0, 44.0), (15.0, 45.0), (20.0, 40.0), (25.0, 37.0),
            (30.0, 40.0), (35.0, 42.0), (40.0, 43.0), (40.0, 55.0),
            (30.0, 60.0), (25.0, 65.0), (20.0, 70.0), (10.0, 71.0),
            (5.0, 62.0), (5.0, 58.0), (-5.0, 58.0), (-10.0, 52.0),
            (-5.0, 48.0), (-5.0, 43.0), (-10.0, 36.0),
        ],
        Lod::Low,
    );

    renderer.add_coastline(
        vec![
            (-17.0, 15.0), (-15.0, 10.0), (-10.0, 5.0), (0.0, 5.0),
            (10.0, 5.0), (15.0, 0.0), (20.0, -5.0), (25.0, -10.0),
            (35.0, -20.0), (35.0, -25.0), (30.0, -30.0), (20.0, -35.0),
            (18.0, -35.0), (15.0, -30.0), (10.0, -15.0), (10.0, 0.0),
            (5.0, 5.0), (-5.0, 5.0), (-10.0, 10.0), (-17.0, 15.0),
        ],
        Lod::Low,
    );

    renderer.add_coastline(
        vec![
            (-17.0, 15.0), (-17.0, 20.0), (-15.0, 28.0), (-5.0, 35.0),
            (10.0, 37.0), (20.0, 33.0), (25.0, 32.0), (35.0, 30.0),
            (35.0, 20.0), (42.0, 12.0), (50.0, 12.0), (45.0, 5.0),
            (35.0, -5.0), (35.0, -20.0),
        ],
        Lod::Low,
    );

    renderer.add_coastline(
        vec![
            (35.0, 42.0), (40.0, 43.0), (50.0, 40.0), (55.0, 37.0),
            (60.0, 25.0), (65.0, 25.0), (70.0, 20.0), (75.0, 15.0),
            (80.0, 8.0), (80.0, 15.0), (88.0, 22.0), (92.0, 22.0),
            (95.0, 16.0), (100.0, 14.0), (105.0, 10.0), (110.0, 20.0),
            (115.0, 22.0), (120.0, 22.0), (122.0, 25.0), (125.0, 30.0),
            (130.0, 35.0), (135.0, 35.0), (140.0, 40.0), (145.0, 45.0),
            (145.0, 50.0), (140.0, 55.0), (135.0, 55.0), (130.0, 52.0),
            (130.0, 43.0), (120.0, 40.0), (110.0, 45.0), (90.0, 50.0),
            (70.0, 55.0), (60.0, 55.0), (50.0, 50.0), (40.0, 43.0),
        ],
        Lod::Low,
    );

    renderer.add_coastline(
        vec![
            (115.0, -20.0), (120.0, -18.0), (130.0, -12.0), (140.0, -12.0),
            (145.0, -15.0), (150.0, -25.0), (153.0, -30.0), (150.0, -35.0),
            (145.0, -38.0), (140.0, -38.0), (135.0, -35.0), (130.0, -32.0),
            (125.0, -32.0), (115.0, -35.0), (115.0, -25.0), (115.0, -20.0),
        ],
        Lod::Low,
    );

    // Major cities with populations (is_capital, is_megacity)
    renderer.add_city(-74.0, 40.7, "New York", 18_800_000, false, true);
    renderer.add_city(-0.1, 51.5, "London", 9_000_000, true, true);
    renderer.add_city(2.3, 48.9, "Paris", 11_000_000, true, true);
    renderer.add_city(139.7, 35.7, "Tokyo", 37_400_000, true, true);
    renderer.add_city(151.2, -33.9, "Sydney", 5_300_000, false, false);
    renderer.add_city(-43.2, -22.9, "Rio", 13_500_000, false, true);
    renderer.add_city(37.6, 55.8, "Moscow", 12_500_000, true, true);
    renderer.add_city(116.4, 39.9, "Beijing", 21_500_000, true, true);
    renderer.add_city(77.2, 28.6, "Delhi", 32_900_000, true, true);
    renderer.add_city(-118.2, 34.0, "Los Angeles", 12_400_000, false, true);
    renderer.add_city(-77.0, 38.9, "Washington", 5_300_000, true, false);
    renderer.add_city(-99.1, 19.4, "Mexico City", 21_800_000, true, true);
    renderer.add_city(-58.4, -34.6, "Buenos Aires", 15_000_000, true, true);
}
