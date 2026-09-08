//! Live overlays stay outside the expensive, cached geographic base layers.
use crate::{app::App, feeds::Kind, map::Projection};
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    widgets::{Clear, Paragraph, Wrap},
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
pub fn render(frame: &mut Frame, app: &App, area: Rect) {
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
    let enabled: Vec<_> = feeds.layers.iter().filter(|l| l.enabled).collect();
    if !enabled.is_empty() && area.height > 8 {
        for (row, layer) in enabled.iter().enumerate() {
            let age = layer
                .updated
                .map(|t| format!("{}s ago", (feeds.now - t).max(0.) as u64))
                .unwrap_or_else(|| "never".into());
            let count = if layer.visible(feeds.now) {
                layer.markers.len()
            } else {
                0
            };
            let region = if layer.kind == Kind::Aircraft {
                " 250nm"
            } else {
                ""
            };
            frame.render_widget(
                Paragraph::new(format!(
                    "{} {} {} · {count}{region} · {age}",
                    layer.kind.key(),
                    layer.kind.source(),
                    layer.state(feeds.now)
                ))
                .style(Style::default().fg(color(layer.kind)).bg(Color::Black)),
                Rect::new(area.x, area.y + row as u16, area.width.min(62), 1),
            );
        }
        let line = if feeds.inspect {
            "INSPECT · click marker / i to release effects"
        } else {
            "7 Quakes 8 Hazards 9 Aircraft t Satellites i Inspect"
        };
        frame.render_widget(
            Paragraph::new(line).style(Style::default().fg(Color::Gray).bg(Color::Black)),
            Rect::new(area.x, area.y + enabled.len() as u16, area.width.min(62), 1),
        );
    }
    if feeds.inspect && area.height > 8 {
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
        let content = if let Some((layer, m)) = selected {
            let observed = sgp4::chrono::DateTime::from_timestamp(m.observed as i64, 0)
                .map(|d| d.format("%Y-%m-%d %H:%M UTC").to_string())
                .unwrap_or_default();
            format!(
                "{} · {}\n{}\n{:.3}, {:.3} · {}\n{}",
                m.label,
                layer.kind.source(),
                m.detail,
                m.lat,
                m.lon,
                observed,
                m.url
            )
        } else {
            let status = enabled
                .iter()
                .map(|l| {
                    format!(
                        "{}: {}{}",
                        l.kind.source(),
                        l.state(feeds.now),
                        l.error
                            .as_ref()
                            .map(|e| format!(" ({e})"))
                            .unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            format!("Click a marker to inspect its source and time.\n{status}")
        };
        let panel = Rect::new(area.x, area.bottom() - 5, area.width.min(100), 5);
        frame.render_widget(Clear, panel);
        frame.render_widget(
            Paragraph::new(content)
                .wrap(Wrap { trim: true })
                .style(Style::default().fg(Color::White).bg(Color::Black)),
            panel,
        );
    }
}
