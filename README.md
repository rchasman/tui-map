# tui-map

High-performance terminal map visualization using Braille Unicode characters.

## Build

Requires Rust 1.88 or newer.

```bash
cargo build --release
```

## Run

```bash
cargo run --release
```

## Controls

- `h`/`←` - Pan left
- `l`/`→` - Pan right
- `k`/`↑` - Pan up
- `j`/`↓` - Pan down
- `+`/`=` - Zoom in
- `-` - Zoom out
- `r`/`0` - Reset view
- `q`/`Esc` - Quit

## Effect interactions

Effects blend light independently of drawing order and react in geographic space,
so contact stays anchored while panning, zooming, or switching projections.

- **Water + fire:** the advancing wave extinguishes heat and leaves drifting steam
  and dying embers. Moisture briefly prevents reignition.
- **EMP + gas:** cyan filaments illuminate density contours, then discharge.
- **Water + life:** overlapping wet and growing areas leave green/gold blooms and pollen.
- **Fire + life:** hot fire chars new growth; weaker embers yield to green shoots.
- **Bio + chem:** moving green/violet wisps retain both colors, with a bright mixing seam.
- **Shockwave + cloud:** passing fronts hollow out fog and brighten its compressed rim;
  density returns as the disturbance settles. Crossing waves continue independently.

Try all six combinations in the interactive lab:

```bash
cargo run --release --example effect_lab
```

Use `1`–`6` to switch scenes, `Space` to pause, `r` to restart, and `q` to quit.
To export an animated preview with a timeline and scene selector:

```bash
cargo run --release --example effect_lab -- --export /tmp/effect-lab.html
```

## Architecture

Built with Ratatui and crossterm. Each terminal character displays a 2x4 Braille dot matrix, giving effective resolution of 2x horizontal and 4x vertical per character cell.

## Data

Falls back to built-in simplified continent outlines. Place `data/natural-earth.json` (GeoJSON) for detailed coastlines.

## Benchmarks

Run `cargo bench --bench hot_paths`. The suite measures projection, rasterization,
spatial queries, fire grids, uncached map rendering (`full_render`), and cached
rendering (`render_cache`) separately. Detailed-data benchmarks use the local
`data/` files when available.

For before/after comparisons, save a baseline before changing the code:

```bash
cargo bench --bench hot_paths -- --save-baseline before
# Make the change, then compare on the same machine and dataset:
cargo bench --bench hot_paths -- --baseline before
```

Avoid concurrent builds or CPU-heavy tasks during measurements. Criterion stores
results under the ignored `target/criterion/` directory. Land-grid build benchmarks
bypass the disk cache to measure construction work.

Run `cargo bench --bench spinning` for deterministic 32-frame globe rotation
traces at several terminal sizes and zoom levels. `map_fresh` measures map
rasterization; `ansi_fresh` adds animation updates, UI composition, buffer diffing,
and ANSI encoding; `ansi_cursor` also includes the targeting reticle used after
dragging. These traces force fresh layers so stale cached views cannot appear as
performance wins. Throughput counts frames; reported batch times cover 32 frames.
ANSI output goes to a sink, so terminal-emulator painting and output backpressure
need a separate interactive check.
