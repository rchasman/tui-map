//! Live overlays stay outside the expensive, cached geographic base layers.
use crate::{app::App, feeds::Kind, map::Projection};
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    widgets::Paragraph,
    Frame,
};

fn color(kind: Kind) -> Color {
    match kind {
        Kind::Quakes => Color::LightRed,
        Kind::Hazards => Color::LightYellow,
        Kind::Aircraft => Color::LightCyan,
        Kind::Satellites => Color::LightMagenta,
    }
}

fn glyph(kind: Kind, detail: &str) -> &'static str {
    match kind {
        Kind::Aircraft => "✈",
        // Single-column characters avoid terminal emoji-width differences.
        Kind::Satellites => "▥◆▥",
        Kind::Quakes => "⌁",
        Kind::Hazards => match detail.split('|').next().unwrap_or("").trim() {
            "Wildfires" => "♨",
            "Volcanoes" => "▲",
            "Severe Storms" => "☁",
            "Floods" => "≋",
            "Drought" => "☀",
            "Snow" | "Sea and Lake Ice" => "❄",
            "Landslides" => "◩",
            _ => "⚠",
        },
    }
}

fn glyph_area(area: Rect, x: u16, y: u16, symbol: &str) -> Rect {
    let width = (symbol.chars().count() as u16).min(area.width);
    let left = x
        .saturating_sub(width / 2)
        .max(area.x)
        .min(area.right() - width);
    Rect::new(left, y, width, 1)
}

fn marker_glyph(frame: &mut Frame, area: Rect, x: i32, y: i32, symbol: &str, tint: Color) {
    let bounds = glyph_area(area, area.x + x as u16 / 2, area.y + y as u16 / 4, symbol);
    frame.render_widget(
        Paragraph::new(symbol).style(Style::default().fg(tint).bg(Color::Rgb(8, 14, 22))),
        bounds,
    );
}
fn point(frame: &mut Frame, area: Rect, x: i32, y: i32, color: Color) {
    if x < 0 || y < 0 || x >= i32::from(area.width) * 2 || y >= i32::from(area.height) * 4 {
        return;
    }
    let cell = &mut frame.buffer_mut()[(area.x + x as u16 / 2, area.y + y as u16 / 4)];
    const DOTS: [[u32; 4]; 2] = [[1, 2, 4, 64], [8, 16, 32, 128]];
    let old = cell.symbol().chars().next().unwrap_or(' ') as u32;
    // Keep existing city labels, marker symbols and the cursor intact.
    if old != 32 && old != '·' as u32 && !(0x2800..=0x28ff).contains(&old) {
        return;
    }
    let bits = if (0x2800..=0x28ff).contains(&old) {
        old - 0x2800
    } else {
        0
    };
    cell.set_char(char::from_u32(0x2800 | bits | DOTS[x as usize % 2][y as usize % 4]).unwrap())
        .set_fg(color);
}
fn path(
    frame: &mut Frame,
    area: Rect,
    projection: &Projection,
    points: &[(f64, f64)],
    color: Color,
) {
    let mut previous = None;
    for &(lon, lat) in points {
        let current = projection.project_point(lon, lat);
        if let (Some((ax, ay)), Some((bx, by))) = (previous, current) {
            let dx: i32 = bx - ax;
            let dy: i32 = by - ay;
            let steps = dx.abs().max(dy.abs());
            // Never bridge the map seam or a clipped hemisphere edge.
            if steps > 0 && steps < i32::from(area.width).max(8) / 2 {
                for i in 0..=steps {
                    point(frame, area, ax + dx * i / steps, ay + dy * i / steps, color);
                }
            }
        }
        previous = current;
    }
}

/// Sample arcs in 3D and depth-test every dot. A visible pair of endpoints
/// must never create a line through Earth or hide the rest of an orbit when
/// its satellite is on the far side. None preserves propagation gaps.
fn orbit_path(
    frame: &mut Frame,
    area: Rect,
    globe: &crate::map::GlobeViewport,
    points: &[Option<glam::DVec3>],
    color: Color,
) {
    for pair in points.windows(2) {
        let (Some(a), Some(b)) = (pair[0], pair[1]) else {
            continue;
        };
        let steps = ((a - b).length() * globe.radius * 1.5)
            .ceil()
            .clamp(1.0, 256.0) as usize;
        for step in 0..=steps {
            let t = step as f64 / steps as f64;
            let direction = a.normalize().lerp(b.normalize(), t).normalize_or_zero();
            let p = direction * (a.length() * (1.0 - t) + b.length() * t);
            if let Some((x, y)) = globe.project_elevated(p) {
                if globe.elevated_sample_visible(p, x, y) {
                    point(frame, area, x, y, color);
                }
            }
        }
    }
}
pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    #[cfg(not(target_arch = "wasm32"))]
    render_active_layers(frame, app, area);
    let feeds = &app.feeds;
    for layer in &feeds.layers {
        if !layer.visible(feeds.now) {
            continue;
        }
        for marker in &layer.markers {
            if layer.kind == Kind::Aircraft && feeds.now - marker.observed > 120. {
                continue;
            }
            if let Projection::Globe(globe) = &app.projection {
                let mut trail = marker.space_trail.clone();
                if layer.kind == Kind::Aircraft {
                    trail.push(marker.space_position);
                }
                orbit_path(frame, area, globe, &trail, color(layer.kind));
            }
            let Some((x, y)) = marker.project(&app.projection) else {
                continue;
            };
            if x < 0 || y < 0 || x >= i32::from(area.width) * 2 || y >= i32::from(area.height) * 4 {
                continue;
            }
            let selected = feeds
                .selected
                .as_ref()
                .is_some_and(|(k, id)| *k == layer.kind && id == &marker.id);
            let tint = if selected {
                Color::White
            } else if layer.kind == Kind::Quakes && feeds.now - marker.observed > 43200. {
                Color::Red
            } else {
                color(layer.kind)
            };
            let mut trail = marker.trail.clone();
            if layer.kind == Kind::Aircraft {
                trail.push((marker.lon, marker.lat));
            }
            if (layer.kind != Kind::Satellites
                && marker.space_position.is_none()
                && marker.space_trail.iter().all(Option::is_none))
                || !matches!(app.projection, Projection::Globe(_))
            {
                path(frame, area, &app.projection, &trail, tint);
            }
            marker_glyph(frame, area, x, y, glyph(layer.kind, &marker.detail), tint);
            if let Some(heading) = marker.heading {
                // Project a short geodesic heading segment, including at the poles.
                let bearing = heading.to_radians();
                let lat = marker.lat.to_radians();
                let lon = marker.lon.to_radians();
                let distance = 0.003_f64;
                let lat2 = (lat.sin() * distance.cos()
                    + lat.cos() * distance.sin() * bearing.cos())
                .asin();
                let lon2 = lon
                    + (bearing.sin() * distance.sin() * lat.cos())
                        .atan2(distance.cos() - lat.sin() * lat2.sin());
                if let (Projection::Globe(globe), Some(position)) =
                    (&app.projection, marker.space_position)
                {
                    let target =
                        crate::map::globe::lonlat_to_vec3(lon2.to_degrees(), lat2.to_degrees())
                            * position.length();
                    orbit_path(frame, area, globe, &[Some(position), Some(target)], tint);
                } else {
                    path(
                        frame,
                        area,
                        &app.projection,
                        &[
                            (marker.lon, marker.lat),
                            (
                                (lon2.to_degrees() + 180.).rem_euclid(360.) - 180.,
                                lat2.to_degrees(),
                            ),
                        ],
                        tint,
                    );
                }
            }
        }
    }
    let mut hits = Vec::new();
    if app.map_renderer.settings.show_labels {
        // Share the city-label setting and avoid their already-painted text.
        let mut candidates = Vec::new();
        let mut occupied = std::collections::HashSet::new();
        for layer in &feeds.layers {
            if !layer.visible(feeds.now) {
                continue;
            }
            for marker in &layer.markers {
                if layer.kind == Kind::Aircraft && feeds.now - marker.observed > 120. {
                    continue;
                }
                if let Some((x, y)) = marker.project(&app.projection) {
                    if x < 0
                        || y < 0
                        || x >= i32::from(area.width) * 2
                        || y >= i32::from(area.height) * 4
                    {
                        continue;
                    }
                    let x = area.x + x as u16 / 2;
                    let y = area.y + y as u16 / 4;
                    let icon = glyph_area(area, x, y, glyph(layer.kind, &marker.detail));
                    occupied.extend((icon.x..icon.right()).map(|col| (col, y)));
                    let selected = feeds
                        .selected
                        .as_ref()
                        .is_some_and(|(k, id)| *k == layer.kind && id == &marker.id);
                    candidates.push((!selected, layer.kind, marker, x, y));
                }
            }
        }
        candidates.sort_by_key(|(unselected, _, _, _, _)| *unselected);
        for (_, kind, marker, x, y) in candidates {
            let full_label = if kind == Kind::Quakes {
                marker
                    .label
                    .split_once(" of ")
                    .map(|(_, place)| format!("M{:.1} {place}", marker.magnitude))
                    .unwrap_or_else(|| marker.label.clone())
            } else {
                marker.label.clone()
            };
            let mut label = full_label.chars().take(28).collect::<String>();
            if label.chars().count() < full_label.chars().count() {
                label.pop();
                label.push('…');
            }
            while ratatui::text::Line::from(label.as_str()).width()
                > area.width.saturating_sub(3) as usize
            {
                label.pop();
            }
            let width = ratatui::text::Line::from(label.as_str()).width() as u16;
            if width == 0 {
                continue;
            }
            let positions = [
                (i32::from(x) + 2, i32::from(y)),
                (i32::from(x) - i32::from(width) - 1, i32::from(y)),
                (i32::from(x) + 1, i32::from(y) - 1),
                (i32::from(x) + 1, i32::from(y) + 1),
            ];
            for (left, row) in positions {
                if left < i32::from(area.x)
                    || row < i32::from(area.y)
                    || left + i32::from(width) > i32::from(area.right())
                    || row >= i32::from(area.bottom())
                {
                    continue;
                }
                let left = left as u16;
                let row = row as u16;
                let blocked = (left.saturating_sub(1).max(area.x)
                    ..(left + width + 1).min(area.right()))
                    .any(|col| {
                        occupied.contains(&(col, row))
                            || frame.buffer_mut()[(col, row)]
                                .symbol()
                                .chars()
                                .any(|c| c.is_alphanumeric())
                    });
                if blocked {
                    continue;
                }
                frame.render_widget(
                    Paragraph::new(label.as_str()).style(Style::default().fg(color(kind))),
                    Rect::new(left, row, width, 1),
                );
                occupied.extend((left..left + width).map(|col| (col, row)));
                hits.push((left, row, width, kind, marker.id.clone()));
                break;
            }
        }
    }
    app.feeds.label_hits = hits;
}

/// Active feed status occupies compact rows at the map's top left.
#[cfg(not(target_arch = "wasm32"))]
fn render_active_layers(frame: &mut Frame, app: &App, area: Rect) {
    let feeds = &app.feeds;
    for (row, layer) in feeds
        .layers
        .iter()
        .filter(|l| l.enabled)
        .take(area.height as usize)
        .enumerate()
    {
        let count = if layer.visible(feeds.now) {
            layer.markers.len()
        } else {
            0
        };
        let source = match layer.kind {
            Kind::Quakes => "USGS",
            Kind::Hazards => "EONET",
            Kind::Aircraft => "adsb.lol",
            Kind::Satellites => "CelesTrak",
        };
        frame.render_widget(
            Paragraph::new(format!(
                "{} · {source} · {count} {}",
                layer.kind.label(),
                layer.state(feeds.now)
            ))
            .style(Style::default().fg(color(layer.kind))),
            Rect::new(area.x, area.y + row as u16, area.width.min(48), 1),
        );
    }
}

/// Only native terminal details reserve footer rows; browser details use HTML.
pub fn height(app: &App, _width: u16) -> u16 {
    if !cfg!(target_arch = "wasm32") && app.feeds.selected.is_some() {
        2
    } else {
        0
    }
}
pub fn render_info(_frame: &mut Frame, _app: &App, _area: Rect) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (frame, app, area) = (_frame, _app, _area);
        let feeds = &app.feeds;
        let selected = feeds.selected.as_ref().and_then(|(k, id)| {
            let layer = &feeds.layers[k.index()];
            layer
                .visible(feeds.now)
                .then(|| {
                    layer
                        .markers
                        .iter()
                        .find(|m| &m.id == id)
                        .map(|m| (layer, m))
                })
                .flatten()
        });
        if let Some((layer, m)) = selected {
            let time = sgp4::chrono::DateTime::from_timestamp(m.observed as i64, 0)
                .map(|d| d.format("%Y-%m-%d %H:%M UTC").to_string())
                .unwrap_or_default();
            let lines = [
                format!(
                    "{} · {} · {:.3}, {:.3} · {time}",
                    m.label,
                    layer.kind.source(),
                    m.lat,
                    m.lon
                ),
                format!("{} · {}", m.detail, m.url),
            ];
            for (i, line) in lines.into_iter().enumerate() {
                if (i as u16) < area.height {
                    frame.render_widget(
                        Paragraph::new(line).style(Style::default().fg(Color::White)),
                        Rect::new(area.x, area.y + i as u16, area.width, 1),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};
    #[test]
    fn satellite_panels_fit_map_edges_and_survive_crossing_trails() {
        let mut terminal = Terminal::new(TestBackend::new(12, 3)).unwrap();
        terminal
            .draw(|frame| {
                let area = Rect::new(2, 1, 8, 1);
                let symbol = glyph(Kind::Satellites, "");
                marker_glyph(frame, area, 0, 0, symbol, Color::Magenta);
                marker_glyph(frame, area, 15, 0, symbol, Color::Magenta);
                for x in 0..16 {
                    point(frame, area, x, 0, Color::Cyan);
                }
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        for start in [2, 7] {
            let text = (start..start + 3)
                .map(|x| buffer[(x, 1)].symbol())
                .collect::<String>();
            assert_eq!(text, "▥◆▥");
        }
        assert_eq!(buffer[(1, 1)].symbol(), " ");
        assert_eq!(buffer[(10, 1)].symbol(), " ");
    }
    #[test]
    fn far_side_orbit_arcs_stay_outside_earth_and_gaps_are_not_joined() {
        use crate::map::{globe::lonlat_to_vec3, GlobeViewport};
        let globe = GlobeViewport::new(0., 0., 70., 200, 180);
        let points: Vec<_> = (92..=268)
            .step_by(4)
            .map(|lon| Some(lonlat_to_vec3(lon as f64, 0.) * 1.07))
            .collect();
        let mut terminal = Terminal::new(TestBackend::new(100, 45)).unwrap();
        terminal
            .draw(|frame| orbit_path(frame, frame.area(), &globe, &points, Color::Magenta))
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(buf.content.iter().any(|c| c.symbol() != " "));
        // Entire orbit segment is behind Earth; only its shoulders can show.
        for y in 8..37 {
            for x in 20..80 {
                assert_eq!(buf[(x, y)].symbol(), " ");
            }
        }
        terminal
            .draw(|frame| {
                orbit_path(
                    frame,
                    frame.area(),
                    &globe,
                    &[points[0], None, *points.last().unwrap()],
                    Color::Magenta,
                )
            })
            .unwrap();
        assert!(terminal
            .backend()
            .buffer()
            .content
            .iter()
            .all(|c| c.symbol() == " "));
    }
    #[test]
    fn live_paths_preserve_city_labels() {
        let mut terminal = Terminal::new(TestBackend::new(20, 4)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                frame.render_widget(Paragraph::new("London"), Rect::new(2, 1, 6, 1));
                for x in 0..40 {
                    point(frame, area, x, 4, Color::Cyan);
                }
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let text = (2..8).map(|x| buffer[(x, 1)].symbol()).collect::<String>();
        assert_eq!(text, "London");
    }
}
