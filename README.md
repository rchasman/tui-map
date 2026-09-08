# tui-map

High-performance terminal map visualization using Braille Unicode characters.

## Browser app

Play at **https://tui-map.vercel.app** (personal Vercel workspace).

The browser runs the shared Rust simulation as WebAssembly in a Web Worker.
A canvas paints the Ratatui cell buffer at up to 30 FPS. Drag to rotate or pan,
scroll or use the zoom buttons, and click to release any of the nine effects.
Water is selected on startup and after reset.
The full-screen terminal includes clickable Ratatui controls for effects, layers,
projection switching, crosshair selection, reset, and help (`?`). Controls wrap to fit narrow
screens; the browser only hosts the canvas and forwards input.

Requires Node.js 24+ and rustup. The repo pins Rust 1.98.1 and wasm-pack 0.15.0:

```bash
npm ci
npm run build:web
npm run test:web
npm run dev:web
```

Open `http://localhost:4173`. Rebuild after changing Rust or browser sources.
The browser bundle is generated in `web/dist`. The aircraft layer uses a small
Node endpoint under `api/aircraft`; `npm run dev:web` serves it locally and Vercel
runs it as a function. The other feeds are fetched directly from their providers.
Natural Earth coastlines and cities load first, followed by higher-resolution
coastlines, land, borders, states, and US counties. The build compacts local
Natural Earth assets when available and otherwise downloads version 5.1.2.
Optional local GADM datasets are not included in the browser bundle.

Deploy to the **personal** Vercel workspace with an explicit scope:

```bash
vercel link --cwd web/dist --yes --project tui-map --scope roey-chasmans-projects
vercel deploy --cwd web/dist --prod --yes --scope roey-chasmans-projects
```

Only deploy `web/dist`. Its `.vercelignore` excludes environment files and the
local project link. Generated bundles and account settings are not committed.

## Build

Rustup installs the pinned Rust 1.98.1 toolchain (including the WASM target).

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
- `q` - Quit (terminal)
- `Esc` / `i` - Return to crosshair selection
- `7` / `8` / `9` - Select Tornado / Frost / Meteor (terminal and browser)

## Live data layers

Toggle layers using the browser controls or the same keys in the terminal:

| Key | Layer | Coverage / refresh |
| --- | --- | --- |
| `e` | Earthquakes — USGS | Past 24 hours, every minute |
| `d` | Natural hazards — NASA EONET | Up to 200 open events from the past 30 days, every 15 minutes |
| `a` | Aircraft — adsb.lol | Observed aircraft within 250 nautical miles of the viewport center, every 15 seconds |
| `t` | Satellites — CelesTrak | Stations catalog (including ISS), orbital elements every 2 hours; SGP4 positions every second |
| `i` | Crosshair | Click a marker for source, observation time, coordinates and details |

All feeds start off and require no API keys. The default crosshair selects markers;
clicking does not release an effect until you choose one with `1`–`9`. Press `Esc` (or `i`)
to return to crosshair selection. Browser clicks and Space select markers;
terminal left/right clicks and Space do the same. Reset returns to the crosshair
while preserving feed settings, snapshots and polling deadlines. Feed status remains below the map.
Selected marker details appear in a compact square at the top right in the browser,
with a source link that opens in a new tab. Terminal details remain below the map. The existing
`L` Labels control covers cities and all four live layers, with labels on by
default. Crowded labels are placed around their markers without overlapping
other labels; zoom in to reveal more. Clicking a label also selects its marker.

Earthquakes have magnitude-sized, age-dimmed markers. Aircraft have short observed
trails and heading ticks. Satellite tracks cover approximately 30 minutes either
side of the current time; positions are **estimates from orbital elements**, not
live telemetry. Elements older than seven days are rejected. Hazard markers use
the latest supported event location, not the affected area's extent.

The overlay shows loading, live/estimated, partial, empty, stale, expired and
offline states plus time since the last successful fetch. Requests time out, retry with backoff,
and retain the last good snapshot on failure. Aircraft expire after two minutes;
other snapshots expire after one day. Disabled layers stop scheduling requests.
Snapshots are cached in memory for the session; the aircraft proxy additionally
coalesces requests and caches responses for 15 seconds. Coverage is provider-
dependent: a blank region does not establish that no aircraft or hazards exist.

Source attribution and API documentation:

- [USGS Earthquake GeoJSON](https://earthquake.usgs.gov/earthquakes/feed/v1.0/geojson.php): data courtesy of the U.S. Geological Survey.
- [NASA EONET](https://eonet.gsfc.nasa.gov/docs/v3): NASA Earth Observatory Natural Event Tracker; individual event source links are preserved.
- [adsb.lol](https://www.adsb.lol/docs/open-data/api/): © adsb.lol contributors, [ODbL 1.0](https://opendatacommons.org/licenses/odbl/1-0/).
- [CelesTrak](https://celestrak.org/NORAD/documentation/gp-data-formats.php): CelesTrak / Dr. T. S. Kelso, using GP JSON and the Rust `sgp4` propagator.

The feed integrations are independently implemented; no Godseye application code
or bundled datasets are copied. Third-party data retains its own terms.

## Effect interactions

Effects blend light independently of drawing order and react in geographic space,
so contact stays anchored while panning, zooming, or switching projections.

On the globe, Nuke, Bio, Chem, Life, Tornado, Frost, and Meteor occupy volumes above the surface.
Their height rotates with Earth: raised tops can remain visible beyond the horizon
while the planet hides their bases. Particle trails use the same depth test.

- **Water:** expanding swells settle into six seconds of flowing ripples. Overlapping
  splashes share one geographic surface: crests reinforce, troughs cancel, and
  opposing crests break into patchy foam. Submerged pool edges disappear instead
  of drawing rings through neighboring water. This is a damped wave model; it
  does not simulate terrain runoff or coast reflections.
- **Life:** uneven shoots branch into leaves and buds, then settle over seven seconds.
- **EMP:** broken, uneven arcs and intermittent branching sparks replace uniform concentric bands.
- **Bio:** low drifting wisps spread through continuous, porous plumes.
- **Chem:** heavier, irregular plumes leave attached rivulets and turbulent pockets.
- **Nuke:** impact-specific billows rise at uneven heights and lean with the plume.
- **Tornado (`7`):** a rotating funnel lifts spiraling ribbons above a debris skirt.
  Its winds carry existing fires and Bio/Chem clouds around the center, including
  across the date line. A sustained hollow core compresses gas into a bright rim.
  Overlapping winds add independently of launch order.
  Wind creates no new fire or fallout; water and frost still quench transported fire.
- **Frost (`8`):** branching ice crystals spread across the surface. The advancing
  cold front quenches fire into steam and leaves a brief cooling field, including
  against new meteor fires. Frost + Life produces blooms like Water + Life.
- **Meteor (`9`):** an immediate impact ejects fiery ballistic trails above a
  cooling crater. It damages cities and ignites land without radioactive fallout.
  Water and frost suppress its fire and thermal glow.
- **Water + fire:** the advancing wave extinguishes heat and leaves drifting steam
  and dying embers. Moisture briefly prevents reignition.
- **EMP + gas:** cyan filaments illuminate density contours, then discharge.
- **Water + life:** overlapping wet and growing areas leave green/gold blooms and pollen.
- **Fire + life:** hot fire chars new growth; weaker embers yield to green shoots.
- **Bio + chem:** moving green/violet wisps retain both colors, with a bright mixing seam.
- **Shockwave + cloud:** passing fronts hollow out fog and brighten its compressed rim;
  density returns as the disturbance settles. Crossing waves continue independently.

Try the interaction scenes in the interactive lab:

```bash
cargo run --release --example effect_lab
```

Use `1`–`6` for combinations, `7` for the standalone nuke, or `8`–`9` for
intersecting water and staggered splashes. Use `Space` to pause,
`r` to restart, and `q` to quit. The nuke grows from a brief white-hot flash into
a rounded mushroom cap, then cools into rolling smoke over 90 animation frames.
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
