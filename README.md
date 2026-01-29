# tui-map

High-performance terminal map visualization using Braille Unicode characters.

## Build

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
