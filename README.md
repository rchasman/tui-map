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
