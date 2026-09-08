use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use ratatui::{backend::CrosstermBackend, layout::Rect, Terminal, TerminalOptions};
use std::{hint::black_box, io, path::Path};
use tui_map::{
    app::App,
    data,
    map::{globe::GlobeViewport, Projection},
    ui,
};

const FRAMES: u64 = 32;

// Observe every encoded byte so optimization cannot discard ANSI formatting.
struct AnsiSink;

impl io::Write for AnsiSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        black_box(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn spin(app: &mut App) {
    if let Projection::Globe(globe) = &mut app.projection {
        globe.apply_momentum(0.005, 0.0);
    }
}

fn bench_spinning(c: &mut Criterion) {
    let mut app = App::new(200, 50);
    data::load_all_geojson(&mut app.map_renderer, Path::new("data")).unwrap();
    if !app.map_renderer.has_data() {
        data::generate_simple_world(&mut app.map_renderer);
    }
    app.map_renderer.build_spatial_indexes();

    let mut group = c.benchmark_group("spinning");
    group.sample_size(10);
    group.throughput(Throughput::Elements(FRAMES));
    for (width, height, zoom) in [
        (100u16, 30u16, 1.0),
        (200, 50, 1.0),
        (200, 50, 4.0),
        (300, 90, 2.0),
    ] {
        let name = format!("{width}x{height}_{zoom}x");
        let make_globe = || {
            Projection::Globe(GlobeViewport::new(
                15.0,
                35.0,
                (width - 2) as f64 * 2.0 * 0.35 * zoom,
                (width - 2) as usize * 2,
                (height - 3) as usize * 4,
            ))
        };
        group.bench_function(BenchmarkId::new("map_fresh", &name), |b| {
            b.iter(|| {
                app.projection = make_globe();
                for _ in 0..FRAMES {
                    spin(&mut app);
                    // Require a fresh image on both sides of a comparison, so
                    // reusing a stale camera view cannot masquerade as a speedup.
                    app.map_renderer.invalidate_cache();
                    black_box(app.map_renderer.render(
                        (width - 2) as usize,
                        (height - 3) as usize,
                        &app.projection,
                    ));
                }
            });
        });
        let mut terminal = Terminal::with_options(
            CrosstermBackend::new(AnsiSink),
            TerminalOptions {
                viewport: ratatui::Viewport::Fixed(Rect::new(0, 0, width, height)),
            },
        )
        .unwrap();
        for (stage, cursor) in [
            ("ansi_fresh", None),
            ("ansi_cursor", Some((width / 2, height / 2))),
        ] {
            app.mouse_pos = cursor;
            group.bench_function(BenchmarkId::new(stage, &name), |b| {
                b.iter(|| {
                    app.projection = make_globe();
                    for _ in 0..FRAMES {
                        spin(&mut app);
                        app.update_explosions();
                        app.map_renderer.invalidate_cache();
                        terminal.draw(|frame| ui::render(frame, &mut app)).unwrap();
                    }
                });
            });
        }
    }
    group.finish();
}

criterion_group!(benches, bench_spinning);
criterion_main!(benches);
