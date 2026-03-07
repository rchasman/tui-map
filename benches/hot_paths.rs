use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use tui_map::braille::BrailleCanvas;
use tui_map::map::geometry::draw_line;
use tui_map::map::projection::{mercator_x, mercator_y, Viewport, WRAP_OFFSETS};
use tui_map::map::renderer::{LineString, Polygon, LandGrid, MapRenderer};
use tui_map::map::spatial::FeatureGrid;
use tui_map::map::globe::GlobeViewport;
use tui_map::app::FireGrid;

// ---------------------------------------------------------------------------
// 1. BrailleCanvas::set_pixel — tightest inner loop of Bresenham
// ---------------------------------------------------------------------------
fn bench_set_pixel(c: &mut Criterion) {
    let mut group = c.benchmark_group("set_pixel");

    // Typical terminal: 200×50 chars = 400×200 braille pixels
    let width = 200;
    let height = 50;
    let px_w = width * 2;
    let px_h = height * 4;

    group.bench_function("scatter_10k", |b| {
        let mut canvas = BrailleCanvas::new(width, height);
        let points: Vec<(usize, usize)> = (0..10_000)
            .map(|i| (i * 7 % px_w, i * 13 % px_h))
            .collect();
        b.iter(|| {
            for &(x, y) in &points {
                canvas.set_pixel(black_box(x), black_box(y));
            }
        });
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// 2. draw_line (Bresenham) — per-segment cost
// ---------------------------------------------------------------------------
fn bench_draw_line(c: &mut Criterion) {
    let mut group = c.benchmark_group("bresenham");

    for &len in &[10, 50, 200, 1000] {
        group.bench_with_input(BenchmarkId::new("line_len", len), &len, |b, &len| {
            let mut canvas = BrailleCanvas::new(200, 50);
            b.iter(|| {
                draw_line(&mut canvas, 0, 0, black_box(len), black_box(len / 2));
            });
        });
    }

    // Batch of short lines (typical coastline segments)
    group.bench_function("batch_500_short", |b| {
        let mut canvas = BrailleCanvas::new(200, 50);
        let segments: Vec<(i32, i32, i32, i32)> = (0..500)
            .map(|i| {
                let x0 = (i * 3) % 400;
                let y0 = (i * 7) % 200;
                (x0, y0, x0 + 5, y0 + 3)
            })
            .collect();
        b.iter(|| {
            for &(x0, y0, x1, y1) in &segments {
                draw_line(&mut canvas, x0, y0, x1, y1);
            }
        });
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// 3. Mercator projection — per-vertex cost in hot render loop
// ---------------------------------------------------------------------------
fn bench_mercator_projection(c: &mut Criterion) {
    let mut group = c.benchmark_group("mercator");

    group.bench_function("mercator_x_y_10k", |b| {
        let lons: Vec<f64> = (0..10_000).map(|i| -180.0 + (i as f64 * 0.036)).collect();
        let lats: Vec<f64> = (0..10_000).map(|i| -85.0 + (i as f64 * 0.017)).collect();
        b.iter(|| {
            for (&lon, &lat) in lons.iter().zip(lats.iter()) {
                black_box(mercator_x(lon));
                black_box(mercator_y(lat));
            }
        });
    });

    group.bench_function("project_mercator_10k", |b| {
        let vp = Viewport::new(0.0, 30.0, 5.0, 400, 200);
        let coords: Vec<(f64, f64)> = (0..10_000)
            .map(|i| (mercator_x(-50.0 + i as f64 * 0.01), mercator_y(20.0 + i as f64 * 0.006)))
            .collect();
        b.iter(|| {
            for &(mx, my) in &coords {
                black_box(vp.project_mercator(mx, my, 0.0));
            }
        });
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// 4. Globe projection — project_vec3 in tight globe render loop
// ---------------------------------------------------------------------------
fn bench_globe_projection(c: &mut Criterion) {
    let mut group = c.benchmark_group("globe");
    use tui_map::map::globe::lonlat_to_vec3;

    let globe = GlobeViewport::new(0.0, 20.0, 140.0, 400, 200);
    let vecs: Vec<_> = (0..10_000)
        .map(|i| lonlat_to_vec3(-180.0 + i as f64 * 0.036, -60.0 + i as f64 * 0.012))
        .collect();

    group.bench_function("project_vec3_10k", |b| {
        b.iter(|| {
            for &v in &vecs {
                black_box(globe.project_vec3(v));
            }
        });
    });

    group.bench_function("lonlat_to_vec3_10k", |b| {
        let lons: Vec<f64> = (0..10_000).map(|i| -180.0 + i as f64 * 0.036).collect();
        let lats: Vec<f64> = (0..10_000).map(|i| -85.0 + i as f64 * 0.017).collect();
        b.iter(|| {
            for (&lon, &lat) in lons.iter().zip(lats.iter()) {
                black_box(lonlat_to_vec3(lon, lat));
            }
        });
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// 5. LineString::new — precomputation cost (Mercator + Vec3 + bounding sphere)
// ---------------------------------------------------------------------------
fn bench_linestring_construction(c: &mut Criterion) {
    let mut group = c.benchmark_group("linestring_new");

    for &n_points in &[10, 100, 1000] {
        let points: Vec<(f64, f64)> = (0..n_points)
            .map(|i| (-180.0 + i as f64 * 360.0 / n_points as f64, -60.0 + i as f64 * 120.0 / n_points as f64))
            .collect();

        group.bench_with_input(BenchmarkId::new("points", n_points), &points, |b, pts| {
            b.iter(|| LineString::new(black_box(pts.clone())));
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// 6. FeatureGrid — build + query (spatial index hot path)
// ---------------------------------------------------------------------------
fn bench_feature_grid(c: &mut Criterion) {
    let mut group = c.benchmark_group("feature_grid");

    // Realistic: ~4000 features (10m coastlines)
    let bboxes: Vec<(f64, f64, f64, f64)> = (0..4000)
        .map(|i| {
            let lon = -180.0 + (i as f64 * 0.09) % 360.0;
            let lat = -85.0 + (i as f64 * 0.042) % 170.0;
            (lon, lat, lon + 2.0, lat + 1.5)
        })
        .collect();

    group.bench_function("build_4000_features", |b| {
        b.iter(|| FeatureGrid::build(black_box(bboxes.iter().copied()), 5.0));
    });

    let grid = FeatureGrid::build(bboxes.iter().copied(), 5.0);

    // Query at various viewport sizes
    for &(label, bounds) in &[
        ("world_view", (-180.0, -85.0, 180.0, 85.0)),
        ("continental", (-30.0, 30.0, 60.0, 70.0)),
        ("regional", (0.0, 45.0, 15.0, 55.0)),
    ] {
        group.bench_function(format!("query_{label}"), |b| {
            b.iter(|| {
                let mut results = Vec::new();
                grid.query_into(bounds.0, bounds.1, bounds.2, bounds.3, &mut results);
                results.sort_unstable();
                results.dedup();
                black_box(&results);
            });
        });
    }

    // query_grid_wrapped pattern — bitset dedup (current implementation)
    group.bench_function("query_wrapped_continental_bitset", |b| {
        b.iter(|| {
            let (min_lon, min_lat, max_lon, max_lat): (f64, f64, f64, f64) = (-30.0, 30.0, 60.0, 70.0);
            let mut raw = Vec::new();
            grid.query_into(min_lon.max(-180.0), min_lat, max_lon.min(180.0), max_lat, &mut raw);
            let n = grid.num_features();
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
            black_box(&unique);
        });
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// 7. LandGrid::is_land — two-tier lookup (fire spawn gatekeeper)
// ---------------------------------------------------------------------------
fn bench_land_grid(c: &mut Criterion) {
    use tui_map::geo::{normalize_lon, normalize_lat};

    let mut group = c.benchmark_group("land_grid");

    // Build a small grid from synthetic polygons (rectangle continents)
    let polygons = vec![
        // North America-ish
        Polygon::new(vec![vec![
            (-130.0, 25.0), (-60.0, 25.0), (-60.0, 55.0), (-130.0, 55.0), (-130.0, 25.0),
        ]]),
        // Europe-ish
        Polygon::new(vec![vec![
            (-10.0, 35.0), (40.0, 35.0), (40.0, 70.0), (-10.0, 70.0), (-10.0, 35.0),
        ]]),
        // Africa-ish
        Polygon::new(vec![vec![
            (-20.0, -35.0), (50.0, -35.0), (50.0, 35.0), (-20.0, 35.0), (-20.0, -35.0),
        ]]),
    ];

    let grid = LandGrid::from_polygons(&polygons);

    // Verify coarse tier is populated correctly
    {
        let mut water = 0usize;
        let mut land = 0usize;
        let mut mixed = 0usize;
        for &v in &grid.coarse {
            match v {
                0 => water += 1,
                2 => land += 1,
                _ => mixed += 1,
            }
        }
        eprintln!(
            "  [land_grid coarse tier] water={water}, land={land}, mixed={mixed} (total={})",
            water + land + mixed
        );

        // Spot-check: deep inland point
        let c_lon = normalize_lon(-100.0) as usize;
        let c_lat = normalize_lat(40.0) as usize;
        let c_idx = c_lat * 360 + c_lon.min(359);
        eprintln!(
            "  [spot] (-100, 40) → coarse[{c_idx}] = {} (expect 2=land)",
            grid.coarse[c_idx]
        );

        // Spot-check: deep ocean point
        let c_lon = normalize_lon(-155.0) as usize;
        let c_lat = normalize_lat(-55.0) as usize;
        let c_idx = c_lat * 360 + c_lon.min(359);
        eprintln!(
            "  [spot] (-155, -55) → coarse[{c_idx}] = {} (expect 0=water)",
            grid.coarse[c_idx]
        );
    }

    // Generate points guaranteed in deep-inland coarse=2 cells (3° buffer from edges)
    let land_points: Vec<(f64, f64)> = (0..10_000)
        .map(|i| {
            // Interior of North America rectangle: [-127, -63] × [28, 52]
            let lon = -127.0 + (i as f64 * 0.0064); // spans 64°
            let lat = 28.0 + (i as f64 * 0.0024);   // spans 24°
            (lon, lat)
        })
        .collect();

    // Generate points guaranteed in deep-ocean coarse=0 cells
    let water_points: Vec<(f64, f64)> = (0..10_000)
        .map(|i| {
            // Central Pacific: lon [-175, -140], lat [-55, -40]
            let lon = -175.0 + (i as f64 * 0.0035);
            let lat = -55.0 + (i as f64 * 0.0015);
            (lon, lat)
        })
        .collect();

    // Points that straddle continent edges (should hit mixed → fine tier)
    let mixed_points: Vec<(f64, f64)> = (0..10_000)
        .map(|i| {
            // Walk along the -130° edge of "North America"
            let lon = -130.0 + (i as f64 * 0.00005); // tiny range around edge
            let lat = 30.0 + (i as f64 * 0.002);
            (lon, lat)
        })
        .collect();

    // Baseline: just the normalize calls (to isolate overhead)
    group.bench_function("normalize_only_10k", |b| {
        b.iter(|| {
            for &(lon, lat) in &land_points {
                black_box(normalize_lon(lon));
                black_box(normalize_lat(lat));
            }
        });
    });

    group.bench_function("is_land_10k_deep_land", |b| {
        b.iter(|| {
            for &(lon, lat) in &land_points {
                black_box(grid.is_land(lon, lat));
            }
        });
    });

    group.bench_function("is_land_10k_deep_water", |b| {
        b.iter(|| {
            for &(lon, lat) in &water_points {
                black_box(grid.is_land(lon, lat));
            }
        });
    });

    group.bench_function("is_land_10k_mixed_edge", |b| {
        b.iter(|| {
            for &(lon, lat) in &mixed_points {
                black_box(grid.is_land(lon, lat));
            }
        });
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// 8. FireGrid::rebuild — O(n) insert from fire Vec
// ---------------------------------------------------------------------------
fn bench_fire_grid(c: &mut Criterion) {
    use tui_map::app::{Fire, WeaponType};

    let mut group = c.benchmark_group("fire_grid");

    for &n_fires in &[1000, 10_000, 30_000] {
        let fires: Vec<Fire> = (0..n_fires)
            .map(|i| Fire {
                lon: -100.0 + (i as f64 * 0.01) % 50.0,
                lat: 30.0 + (i as f64 * 0.005) % 20.0,
                intensity: (200 - (i % 200)) as u8,
                weapon_type: WeaponType::Nuke,
            })
            .collect();

        group.bench_with_input(BenchmarkId::new("rebuild", n_fires), &fires, |b, fires| {
            let mut grid = FireGrid::new(0.25);
            b.iter(|| {
                grid.rebuild(black_box(fires));
            });
        });
    }

    // fires_in_region query
    let fires: Vec<Fire> = (0..30_000)
        .map(|i| Fire {
            lon: -100.0 + (i as f64 * 0.01) % 50.0,
            lat: 30.0 + (i as f64 * 0.005) % 20.0,
            intensity: 200,
            weapon_type: WeaponType::Nuke,
        })
        .collect();
    let mut grid = FireGrid::new(0.25);
    grid.rebuild(&fires);

    group.bench_function("fires_in_region_30k", |b| {
        b.iter(|| {
            black_box(grid.fires_in_region(-90.0, 35.0, -70.0, 45.0));
        });
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// 9. draw_linestring_with_offset — full per-feature Mercator render
//    (benchmarked via the public draw_linestring → 3 offsets)
// ---------------------------------------------------------------------------
fn bench_draw_linestring_mercator(c: &mut Criterion) {
    let mut group = c.benchmark_group("draw_linestring_mercator");

    // Simulate a 200-point coastline feature
    let points: Vec<(f64, f64)> = (0..200)
        .map(|i| (-10.0 + i as f64 * 0.15, 40.0 + (i as f64 * 0.05).sin() * 5.0))
        .collect();
    let line = LineString::new(points);
    let vp = Viewport::new(10.0, 45.0, 5.0, 400, 200);

    group.bench_function("200pt_feature_visible", |b| {
        let mut canvas = BrailleCanvas::new(200, 50);
        b.iter(|| {
            // Replicate draw_linestring: try all 3 wrap offsets
            for &lon_offset in &WRAP_OFFSETS {
                // Bbox early-out
                let (merc_min_x, merc_min_y, merc_max_x, merc_max_y) = line.mercator_bbox;
                let (px1, py1) = vp.project_mercator(merc_min_x, merc_min_y, lon_offset);
                let (px2, py2) = vp.project_mercator(merc_max_x, merc_max_y, lon_offset);
                let bb_min_x = px1.min(px2);
                let bb_max_x = px1.max(px2);
                let bb_min_y = py1.min(py2);
                let bb_max_y = py1.max(py2);

                if bb_max_x < -50 || bb_min_x > vp.width as i32 + 50 ||
                   bb_max_y < -50 || bb_min_y > vp.height as i32 + 50 {
                    continue;
                }

                let mut prev: Option<(i32, i32)> = None;
                for &(mx, my) in &line.mercator {
                    let (px, py) = vp.project_mercator(mx, my, lon_offset);
                    if let Some((prev_x, prev_y)) = prev {
                        let dx = (px - prev_x).abs();
                        let dy = (py - prev_y).abs();
                        let dist = (dx + dy) as usize;
                        if dist < vp.width / 2 && vp.line_might_be_visible((prev_x, prev_y), (px, py)) {
                            draw_line(&mut canvas, prev_x, prev_y, px, py);
                        }
                    }
                    prev = Some((px, py));
                }
            }
        });
    });

    // Feature completely off-screen (should early-out at bbox)
    let far_points: Vec<(f64, f64)> = (0..200)
        .map(|i| (150.0 + i as f64 * 0.01, -50.0 + i as f64 * 0.01))
        .collect();
    let far_line = LineString::new(far_points);

    group.bench_function("200pt_feature_culled", |b| {
        let mut canvas = BrailleCanvas::new(200, 50);
        b.iter(|| {
            for &lon_offset in &WRAP_OFFSETS {
                let (merc_min_x, merc_min_y, merc_max_x, merc_max_y) = far_line.mercator_bbox;
                let (px1, py1) = vp.project_mercator(merc_min_x, merc_min_y, lon_offset);
                let (px2, py2) = vp.project_mercator(merc_max_x, merc_max_y, lon_offset);
                let bb_min_x = px1.min(px2);
                let bb_max_x = px1.max(px2);
                let bb_min_y = py1.min(py2);
                let bb_max_y = py1.max(py2);

                if bb_max_x < -50 || bb_min_x > vp.width as i32 + 50 ||
                   bb_max_y < -50 || bb_min_y > vp.height as i32 + 50 {
                    continue;
                }

                let mut prev: Option<(i32, i32)> = None;
                for &(mx, my) in &far_line.mercator {
                    let (px, py) = vp.project_mercator(mx, my, lon_offset);
                    if let Some((prev_x, prev_y)) = prev {
                        let dx = (px - prev_x).abs();
                        let dy = (py - prev_y).abs();
                        let dist = (dx + dy) as usize;
                        if dist < vp.width / 2 && vp.line_might_be_visible((prev_x, prev_y), (px, py)) {
                            draw_line(&mut canvas, prev_x, prev_y, px, py);
                        }
                    }
                    prev = Some((px, py));
                }
            }
        });
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// 10. Full render — end-to-end with synthetic data at different zoom levels
// ---------------------------------------------------------------------------
fn bench_full_render(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_render");
    // Longer measurement for these expensive benchmarks
    group.sample_size(30);

    let mut renderer = MapRenderer::new();
    tui_map::data::generate_simple_world(&mut renderer);
    renderer.build_spatial_indexes();

    let width = 200usize;
    let height = 50usize;

    for &(label, zoom) in &[("world_1x", 1.0), ("continental_4x", 4.0), ("regional_10x", 10.0)] {
        // Mercator
        group.bench_function(format!("mercator_{label}"), |b| {
            let projection = tui_map::map::Projection::Mercator(Viewport::new(0.0, 30.0, zoom, width * 2, height * 4));
            b.iter(|| {
                black_box(renderer.render(width, height, &projection));
            });
        });

        // Globe
        group.bench_function(format!("globe_{label}"), |b| {
            let projection = tui_map::map::Projection::Globe(GlobeViewport::new(0.0, 30.0, width as f64 * 0.35 * zoom, width * 2, height * 4));
            b.iter(|| {
                black_box(renderer.render(width, height, &projection));
            });
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// 11. Full render with REAL GeoJSON data (if available)
// ---------------------------------------------------------------------------
fn bench_real_data_render(c: &mut Criterion) {
    use std::path::Path;

    let data_dir = Path::new("data");
    if !data_dir.exists() {
        eprintln!("  [real_data] Skipping — no data/ directory found");
        return;
    }

    let mut renderer = MapRenderer::new();
    if tui_map::data::load_all_geojson(&mut renderer, data_dir).is_err() {
        eprintln!("  [real_data] Skipping — failed to load GeoJSON");
        return;
    }

    if !renderer.has_data() {
        eprintln!("  [real_data] Skipping — no data loaded");
        return;
    }

    renderer.build_land_grid();
    renderer.build_spatial_indexes();

    eprintln!(
        "  [real_data] Loaded: coast_low={}, coast_med={}, coast_high={}, borders_med={}, borders_high={}, states={}, counties={}",
        renderer.coastlines_low.len(),
        renderer.coastlines_medium.len(),
        renderer.coastlines_high.len(),
        renderer.borders_medium.len(),
        renderer.borders_high.len(),
        renderer.states.len(),
        renderer.counties.len(),
    );

    let mut group = c.benchmark_group("real_data_render");
    group.sample_size(20);

    let width = 200usize;
    let height = 50usize;

    // Mercator at various zoom levels
    for &(label, zoom, center_lon, center_lat) in &[
        ("world_1x", 1.0, 0.0, 20.0),
        ("europe_4x", 4.0, 15.0, 50.0),
        ("usa_8x", 8.0, -95.0, 38.0),
        ("city_20x", 20.0, -74.0, 40.7),  // NYC
    ] {
        group.bench_function(format!("mercator_{label}"), |b| {
            let projection = tui_map::map::Projection::Mercator(
                Viewport::new(center_lon, center_lat, zoom, width * 2, height * 4),
            );
            b.iter(|| {
                black_box(renderer.render(width, height, &projection));
            });
        });
    }

    // Globe at various zoom levels
    for &(label, zoom, center_lon, center_lat) in &[
        ("world_1x", 1.0, 0.0, 20.0),
        ("europe_4x", 4.0, 15.0, 50.0),
        ("usa_8x", 8.0, -95.0, 38.0),
    ] {
        group.bench_function(format!("globe_{label}"), |b| {
            let projection = tui_map::map::Projection::Globe(
                GlobeViewport::new(center_lon, center_lat, width as f64 * 0.35 * zoom, width * 2, height * 4),
            );
            b.iter(|| {
                black_box(renderer.render(width, height, &projection));
            });
        });
    }

    group.finish();

    // Also benchmark the land_grid with real polygons (if built)
    if renderer.is_on_land(0.0, 0.0) || !renderer.is_on_land(0.0, 0.0) {
        let mut group = c.benchmark_group("real_land_grid");

        // London (land)
        let land_points: Vec<(f64, f64)> = (0..10_000)
            .map(|i| (-0.1 + i as f64 * 0.00001, 51.5 + i as f64 * 0.00001))
            .collect();

        // Mid-Atlantic (water)
        let water_points: Vec<(f64, f64)> = (0..10_000)
            .map(|i| (-40.0 + i as f64 * 0.00001, 30.0 + i as f64 * 0.00001))
            .collect();

        // Coastline walk (mixed)
        let coast_points: Vec<(f64, f64)> = (0..10_000)
            .map(|i| {
                // Walk along US east coast
                let t = i as f64 / 10_000.0;
                let lon = -80.0 + t * 15.0;
                let lat = 25.0 + t * 20.0;
                (lon, lat)
            })
            .collect();

        group.bench_function("is_land_10k_london", |b| {
            b.iter(|| {
                for &(lon, lat) in &land_points {
                    black_box(renderer.is_on_land(lon, lat));
                }
            });
        });

        group.bench_function("is_land_10k_atlantic", |b| {
            b.iter(|| {
                for &(lon, lat) in &water_points {
                    black_box(renderer.is_on_land(lon, lat));
                }
            });
        });

        group.bench_function("is_land_10k_us_coast", |b| {
            b.iter(|| {
                for &(lon, lat) in &coast_points {
                    black_box(renderer.is_on_land(lon, lat));
                }
            });
        });

        group.finish();
    }
}

// ---------------------------------------------------------------------------
// 12. fire_map clear — per-frame buffer zeroing cost
// ---------------------------------------------------------------------------
fn bench_fire_map_clear(c: &mut Criterion) {
    let mut group = c.benchmark_group("fire_map_clear");

    for &(w, h) in &[(200, 50), (400, 100)] {
        let size = w * h;
        let mut intensity = vec![0u8; size];

        group.bench_function(format!("{w}x{h}_fill_zero"), |b| {
            b.iter(|| {
                intensity.fill(0);
                black_box(&intensity);
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_set_pixel,
    bench_draw_line,
    bench_mercator_projection,
    bench_globe_projection,
    bench_linestring_construction,
    bench_feature_grid,
    bench_land_grid,
    bench_fire_grid,
    bench_draw_linestring_mercator,
    bench_full_render,
    bench_real_data_render,
    bench_fire_map_clear,
);
criterion_main!(benches);
