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
pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    let feeds = &app.feeds;
    for layer in &feeds.layers {
        if !layer.visible(feeds.now) {
            continue;
        }
        for marker in &layer.markers {
            if layer.kind == Kind::Aircraft && feeds.now - marker.observed > 120. {
                continue;
            }
            let Some((x, y)) = app.projection.project_point(marker.lon, marker.lat) else {
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
            path(frame, area, &app.projection, &trail, tint);
            point(frame, area, x, y, tint);
            let radius = if layer.kind == Kind::Quakes {
                marker.magnitude.round().clamp(1., 5.) as i32
            } else {
                1
            };
            for d in 1..=radius {
                point(frame, area, x - d, y, tint);
                point(frame, area, x + d, y, tint);
                point(frame, area, x, y - d, tint);
                point(frame, area, x, y + d, tint);
            }
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
                if let Some((x, y)) = app.projection.project_point(marker.lon, marker.lat) {
                    if x < 0
                        || y < 0
                        || x >= i32::from(area.width) * 2
                        || y >= i32::from(area.height) * 4
                    {
                        continue;
                    }
                    let x = area.x + x as u16 / 2;
                    let y = area.y + y as u16 / 4;
                    occupied.insert((x, y));
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

/// Information lives in a dedicated footer, never over geographic cells.
pub fn height(app: &App, width: u16) -> u16 {
    let count = app.feeds.layers.iter().filter(|l| l.enabled).count() as u16;
    count.div_ceil((width / 24).max(1)) + if app.feeds.selected.is_some() { 2 } else { 0 }
}
pub fn render_info(frame: &mut Frame, app: &App, area: Rect) {
    if area.is_empty() {
        return;
    }
    let feeds = &app.feeds;
    let columns = (area.width / 24).max(1);
    let enabled: Vec<_> = feeds.layers.iter().filter(|l| l.enabled).collect();
    let summary_rows = (enabled.len() as u16).div_ceil(columns);
    let cell_width = area.width / columns;
    for (i, l) in enabled.iter().enumerate() {
        let row = i as u16 / columns;
        if row >= area.height {
            break;
        }
        let count = if l.visible(feeds.now) {
            l.markers.len()
        } else {
            0
        };
        let source = match l.kind {
            Kind::Quakes => "USGS",
            Kind::Hazards => "EONET",
            Kind::Aircraft => "adsb.lol",
            Kind::Satellites => "CelesTrak",
        };
        frame.render_widget(
            Paragraph::new(format!("{source} {count} {}", l.state(feeds.now)))
                .style(Style::default().fg(color(l.kind))),
            Rect::new(
                area.x + (i as u16 % columns) * cell_width,
                area.y + row,
                cell_width,
                1,
            ),
        );
    }
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
            if i as u16 + summary_rows < area.height {
                frame.render_widget(
                    Paragraph::new(line).style(Style::default().fg(Color::White)),
                    Rect::new(area.x, area.y + summary_rows + i as u16, area.width, 1),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};
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
