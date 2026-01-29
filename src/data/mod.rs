use crate::map::MapRenderer;
use anyhow::Result;
use geojson::{GeoJson, Geometry, Value};
use std::fs;
use std::path::Path;

/// Load Natural Earth GeoJSON data into the map renderer
pub fn load_geojson(renderer: &mut MapRenderer, path: &Path) -> Result<()> {
    let content = fs::read_to_string(path)?;
    let geojson: GeoJson = content.parse()?;

    match geojson {
        GeoJson::FeatureCollection(fc) => {
            for feature in fc.features {
                if let Some(geometry) = feature.geometry {
                    process_geometry(renderer, geometry);
                }
            }
        }
        GeoJson::Feature(f) => {
            if let Some(geometry) = f.geometry {
                process_geometry(renderer, geometry);
            }
        }
        GeoJson::Geometry(geometry) => {
            process_geometry(renderer, geometry);
        }
    }

    Ok(())
}

fn process_geometry(renderer: &mut MapRenderer, geometry: Geometry) {
    match geometry.value {
        Value::LineString(coords) => {
            let line: Vec<(f64, f64)> = coords.into_iter().map(|c| (c[0], c[1])).collect();
            renderer.add_coastline(line);
        }
        Value::MultiLineString(lines) => {
            for coords in lines {
                let line: Vec<(f64, f64)> = coords.into_iter().map(|c| (c[0], c[1])).collect();
                renderer.add_coastline(line);
            }
        }
        Value::Polygon(rings) => {
            // Use the exterior ring as a line
            if let Some(exterior) = rings.into_iter().next() {
                let line: Vec<(f64, f64)> = exterior.into_iter().map(|c| (c[0], c[1])).collect();
                renderer.add_coastline(line);
            }
        }
        Value::MultiPolygon(polygons) => {
            for rings in polygons {
                if let Some(exterior) = rings.into_iter().next() {
                    let line: Vec<(f64, f64)> = exterior.into_iter().map(|c| (c[0], c[1])).collect();
                    renderer.add_coastline(line);
                }
            }
        }
        Value::GeometryCollection(geometries) => {
            for g in geometries {
                process_geometry(renderer, g);
            }
        }
        _ => {}
    }
}

/// Generate a simple world map outline for when no data file is available
pub fn generate_simple_world(renderer: &mut MapRenderer) {
    // Simplified continent outlines
    // North America
    renderer.add_coastline(vec![
        (-168.0, 65.0), (-166.0, 60.0), (-141.0, 60.0), (-130.0, 55.0),
        (-125.0, 48.0), (-124.0, 40.0), (-117.0, 32.0), (-110.0, 25.0),
        (-97.0, 25.0), (-97.0, 28.0), (-82.0, 24.0), (-80.0, 25.0),
        (-81.0, 31.0), (-75.0, 35.0), (-70.0, 41.0), (-67.0, 45.0),
        (-65.0, 47.0), (-55.0, 47.0), (-52.0, 47.0), (-55.0, 52.0),
        (-58.0, 55.0), (-64.0, 60.0), (-73.0, 62.0), (-80.0, 63.0),
        (-95.0, 62.0), (-110.0, 68.0), (-130.0, 70.0), (-145.0, 70.0),
        (-168.0, 65.0),
    ]);

    // South America
    renderer.add_coastline(vec![
        (-80.0, 10.0), (-75.0, 5.0), (-70.0, 5.0), (-60.0, 5.0),
        (-50.0, 0.0), (-35.0, -5.0), (-35.0, -10.0), (-38.0, -15.0),
        (-40.0, -22.0), (-48.0, -25.0), (-55.0, -34.0), (-58.0, -38.0),
        (-65.0, -42.0), (-68.0, -50.0), (-75.0, -52.0), (-75.0, -45.0),
        (-72.0, -40.0), (-72.0, -30.0), (-70.0, -20.0), (-70.0, -15.0),
        (-80.0, -5.0), (-80.0, 0.0), (-80.0, 10.0),
    ]);

    // Europe
    renderer.add_coastline(vec![
        (-10.0, 36.0), (-5.0, 36.0), (0.0, 38.0), (5.0, 43.0),
        (10.0, 44.0), (15.0, 45.0), (20.0, 40.0), (25.0, 37.0),
        (30.0, 40.0), (35.0, 42.0), (40.0, 43.0), (40.0, 55.0),
        (30.0, 60.0), (25.0, 65.0), (20.0, 70.0), (10.0, 71.0),
        (5.0, 62.0), (5.0, 58.0), (-5.0, 58.0), (-10.0, 52.0),
        (-5.0, 48.0), (-5.0, 43.0), (-10.0, 36.0),
    ]);

    // Africa
    renderer.add_coastline(vec![
        (-17.0, 15.0), (-15.0, 10.0), (-10.0, 5.0), (0.0, 5.0),
        (10.0, 5.0), (15.0, 0.0), (20.0, -5.0), (25.0, -10.0),
        (35.0, -20.0), (35.0, -25.0), (30.0, -30.0), (20.0, -35.0),
        (18.0, -35.0), (15.0, -30.0), (10.0, -15.0), (10.0, 0.0),
        (5.0, 5.0), (-5.0, 5.0), (-10.0, 10.0), (-17.0, 15.0),
    ]);

    // Africa north coast
    renderer.add_coastline(vec![
        (-17.0, 15.0), (-17.0, 20.0), (-15.0, 28.0), (-5.0, 35.0),
        (10.0, 37.0), (20.0, 33.0), (25.0, 32.0), (35.0, 30.0),
        (35.0, 20.0), (42.0, 12.0), (50.0, 12.0), (45.0, 5.0),
        (35.0, -5.0), (35.0, -20.0),
    ]);

    // Asia (simplified)
    renderer.add_coastline(vec![
        (35.0, 42.0), (40.0, 43.0), (50.0, 40.0), (55.0, 37.0),
        (60.0, 25.0), (65.0, 25.0), (70.0, 20.0), (75.0, 15.0),
        (80.0, 8.0), (80.0, 15.0), (88.0, 22.0), (92.0, 22.0),
        (95.0, 16.0), (100.0, 14.0), (105.0, 10.0), (110.0, 20.0),
        (115.0, 22.0), (120.0, 22.0), (122.0, 25.0), (125.0, 30.0),
        (130.0, 35.0), (135.0, 35.0), (140.0, 40.0), (145.0, 45.0),
        (145.0, 50.0), (140.0, 55.0), (135.0, 55.0), (130.0, 52.0),
        (130.0, 43.0), (120.0, 40.0), (110.0, 45.0), (90.0, 50.0),
        (70.0, 55.0), (60.0, 55.0), (50.0, 50.0), (40.0, 43.0),
    ]);

    // Australia
    renderer.add_coastline(vec![
        (115.0, -20.0), (120.0, -18.0), (130.0, -12.0), (140.0, -12.0),
        (145.0, -15.0), (150.0, -25.0), (153.0, -30.0), (150.0, -35.0),
        (145.0, -38.0), (140.0, -38.0), (135.0, -35.0), (130.0, -32.0),
        (125.0, -32.0), (115.0, -35.0), (115.0, -25.0), (115.0, -20.0),
    ]);

    // Major cities
    renderer.add_city(-74.0, 40.7, "New York");
    renderer.add_city(-0.1, 51.5, "London");
    renderer.add_city(2.3, 48.9, "Paris");
    renderer.add_city(139.7, 35.7, "Tokyo");
    renderer.add_city(151.2, -33.9, "Sydney");
    renderer.add_city(-43.2, -22.9, "Rio");
    renderer.add_city(37.6, 55.8, "Moscow");
    renderer.add_city(116.4, 39.9, "Beijing");
    renderer.add_city(77.2, 28.6, "Delhi");
    renderer.add_city(-118.2, 34.0, "Los Angeles");
}
