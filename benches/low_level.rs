use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use ratatui::{buffer::Buffer, layout::Rect};
use std::{collections::HashMap, hash::BuildHasher, hint::black_box};
use tui_map::{
    app::WeaponType,
    interactions::Interactions,
    map::{spatial::SpatialGrid, Projection, Viewport},
    ui::weapons::{composite::Compositor, ExplosionRender},
};

fn populate<H: BuildHasher + Default>() -> HashMap<(i32, i32), Vec<usize>, H> {
    let mut cells = HashMap::<_, Vec<_>, H>::default();
    for i in 0..7343 {
        let lon = (i as f64 * 137.507_764) % 360.0 - 180.0;
        let lat = (i as f64 * 31.731) % 180.0 - 90.0;
        cells
            .entry(((lon / 10.0).floor() as i32, (lat / 10.0).floor() as i32))
            .or_default()
            .push(i);
    }
    cells
}

fn probe<H: BuildHasher>(cells: &HashMap<(i32, i32), Vec<usize>, H>, radius: i32) -> usize {
    let mut count = 0;
    for y in -radius..=radius {
        for x in -radius * 2..=radius * 2 {
            if let Some(items) = cells.get(&(x, y)) {
                count += items.len();
            }
        }
    }
    count
}

fn bench_city_grid(c: &mut Criterion) {
    let std = populate::<std::collections::hash_map::RandomState>();
    let fast = populate::<foldhash::fast::RandomState>();
    let mut grid = SpatialGrid::new(10.0);
    for i in 0..7343 {
        grid.insert(
            (i as f64 * 137.507_764) % 360.0 - 180.0,
            (i as f64 * 31.731) % 180.0 - 90.0,
            i,
        );
    }
    let mut group = c.benchmark_group("city_lookup");
    for radius in [1, 9] {
        assert_eq!(probe(&std, radius), probe(&fast, radius));
        group.bench_function(BenchmarkId::new("std", radius), |b| {
            b.iter(|| black_box(probe(black_box(&std), black_box(radius))))
        });
        group.bench_function(BenchmarkId::new("foldhash", radius), |b| {
            b.iter(|| black_box(probe(black_box(&fast), black_box(radius))))
        });
    }
    group.bench_function("production_world", |b| {
        b.iter(|| black_box(grid.query_bbox(-180.0, -90.0, 180.0, 90.0)))
    });
    group.bench_function("production_radius", |b| {
        b.iter(|| black_box(grid.query_radius(0.0, 0.0, 3.0)))
    });
    group.finish();
}

fn bench_effects(c: &mut Criterion) {
    let mut group = c.benchmark_group("effect_compositor");
    group.sample_size(10);
    for (width, height) in [(100u16, 30u16), (200, 50)] {
        let area = Rect::new(1, 1, width - 2, height - 3);
        let projection = Projection::Mercator(Viewport::new(
            0.0,
            0.0,
            4.0,
            area.width as usize * 2,
            area.height as usize * 4,
        ));
        for count in [1, 8, 32] {
            let explosions: Vec<_> = (0..count)
                .map(|i| ExplosionRender { seed: 0,
                    x: 10 + (i * 17) as u16 % (width - 20),
                    y: 10 + (i * 7) as u16 % (height - 20),
                    frame: 15,
                    radius: 8,
                    weapon_type: [WeaponType::Nuke, WeaponType::Emp, WeaponType::Life][i % 3],
                    lon: 0.0,
                    lat: 0.0,
                    radius_km: 400.0,
                })
                .collect();
            let world = Interactions::default();
            let mut compositor = Compositor::default();
            let mut buffer = Buffer::empty(Rect::new(0, 0, width, height));
            group.bench_function(format!("{width}x{height}_{count}_effects"), |b| {
                b.iter(|| {
                    buffer.reset();
                    compositor.render(
                        black_box(&explosions),
                        &world,
                        &projection,
                        area,
                        15,
                        &mut buffer,
                    );
                    black_box(&buffer);
                });
            });
        }
    }
    group.finish();
}

fn bench_interactions(c: &mut Criterion) {
    let mut group = c.benchmark_group("effect_interactions");
    group.sample_size(20);
    for count in [1, 8, 24] {
        for water in [false, true] {
            let mut world = Interactions::default();
            for i in 0..count {
                world.launch(&tui_map::app::Explosion { seed: 0,
                    lon: 179.0 + i as f64 * 0.02,
                    lat: 0.0,
                    radius_km: 300.0,
                    frame: 20,
                    weapon_type: WeaponType::Life,
                });
                if water {
                    world.launch(&tui_map::app::Explosion { seed: 0,
                        lon: 179.0 + i as f64 * 0.02,
                        lat: 0.0,
                        radius_km: 300.0,
                        frame: 20,
                        weapon_type: WeaponType::Water,
                    });
                }
            }
            let label = if water { "wet_growth" } else { "dry_growth" };
            group.bench_function(BenchmarkId::new(label, count), |b| {
                b.iter_batched(
                    || {
                        (
                            Interactions {
                                fields: world.fields.clone(),
                                reactions: world.reactions.clone(),
                            },
                            Vec::new(),
                        )
                    },
                    |(mut world, mut fires)| {
                        black_box(world.update(&mut fires));
                        black_box((&world, &fires));
                    },
                    criterion::BatchSize::SmallInput,
                );
            });
        }
    }
    group.finish();
}

fn bench_clouds(c: &mut Criterion) {
    use tui_map::ui::weapons::{gas_clouds, GasCloudRender};
    let mut group = c.benchmark_group("interacting_clouds");
    group.sample_size(20);
    for count in [0, 8, 48] {
        let mut world = Interactions::default();
        for i in 0..count {
            world.launch(&tui_map::app::Explosion { seed: 0,
                lon: (i % 8) as f64 - 4.0,
                lat: (i / 8) as f64 - 3.0,
                frame: 15,
                radius_km: 800.0,
                weapon_type: WeaponType::Emp,
            });
        }
        for (width, height) in [(100, 30), (200, 50)] {
            let area = Rect::new(0, 0, width, height);
            let projection = Projection::Mercator(Viewport::new(
                0.0,
                0.0,
                8.0,
                width as usize * 2,
                height as usize * 4,
            ));
            let clouds = [
                GasCloudRender {
                    lon: 0.0,
                    lat: 0.0,
                    radius_km: 3000.0,
                    intensity: 1800,
                    weapon_type: WeaponType::Bio,
                },
                GasCloudRender {
                    lon: 1.0,
                    lat: 1.0,
                    radius_km: 3000.0,
                    intensity: 1800,
                    weapon_type: WeaponType::Chem,
                },
            ];
            let mut buf = Buffer::empty(area);
            let mut density = vec![(0.0, 0.0); width as usize * height as usize];
            group.bench_function(format!("{width}x{height}_{count}_fields"), |b| {
                b.iter(|| {
                    buf.reset();
                    gas_clouds::render_interacting(
                        black_box(&clouds),
                        &mut density,
                        area,
                        15,
                        &mut buf,
                        &projection,
                        &world,
                    );
                    black_box((&buf, &density));
                })
            });
        }
    }
    group.finish();
}

fn bench_nuke(c: &mut Criterion) {
    let mut group = c.benchmark_group("nuke_sampling");
    group.sample_size(20);
    let area = Rect::new(0, 0, 100, 50);
    for radius in [8, 24] {
        for frame in [5, 30, 60] {
            let exp = ExplosionRender { seed: 0,
                x: 50,
                y: 40,
                frame,
                radius,
                weapon_type: WeaponType::Nuke,
                lon: 0.0,
                lat: 0.0,
                radius_km: 400.0,
            };
            let mut buffer = Buffer::empty(area);
            group.bench_function(format!("radius_{radius}_age_{frame}"), |b| {
                b.iter(|| {
                    buffer.reset();
                    tui_map::ui::weapons::nuke::render(
                        black_box(&exp),
                        50,
                        40,
                        area,
                        15,
                        &mut buffer,
                        None,
                    );
                    black_box(&buffer);
                })
            });
        }
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_city_grid,
    bench_effects,
    bench_interactions,
    bench_clouds,
    bench_nuke
);
criterion_main!(benches);
