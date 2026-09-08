use crate::{
    app::WeaponType,
    hash::rand_simple,
    interactions::{destination, Field, Reaction, ReactionKind},
    map::{Projection, WRAP_OFFSETS},
};
use ratatui::{buffer::Buffer, layout::Rect, style::Color};
use std::f64::consts::TAU;

/// Project each sub-cell sample independently: clipping a center must not hide
/// a visible wave edge, and Mercator copies must cross the date line naturally.
fn dot(
    lon: f64,
    lat: f64,
    projection: &Projection,
    area: Rect,
    buf: &mut Buffer,
    color: (u8, u8, u8),
    light: f64,
) {
    if light < 0.03 {
        return;
    }
    let mut plot = |sx: i32, sy: i32| {
        if sx < 0 || sy < 0 || sx >= area.width as i32 * 2 || sy >= area.height as i32 * 4 {
            return;
        }
        let cell = &mut buf[(area.x + (sx / 2) as u16, area.y + (sy / 4) as u16)];
        let old = cell.symbol().chars().next().unwrap_or(' ') as u32;
        let bits = if (0x2800..=0x28ff).contains(&old) {
            old - 0x2800
        } else {
            0
        };
        let bit = [[1, 2, 4, 64], [8, 16, 32, 128]][(sx % 2) as usize][(sy % 4) as usize];
        let light = light.clamp(0.0, 1.0);
        let mut rgb = (
            (color.0 as f64 * light) as u8,
            (color.1 as f64 * light) as u8,
            (color.2 as f64 * light) as u8,
        );
        if bits != 0 {
            if let Color::Rgb(r, g, b) = cell.fg {
                rgb = (rgb.0.max(r), rgb.1.max(g), rgb.2.max(b));
            }
        }
        cell.set_char(char::from_u32(0x2800 + (bits | bit)).unwrap())
            .set_fg(Color::Rgb(rgb.0, rgb.1, rgb.2));
    };
    match projection {
        Projection::Globe(g) => {
            if let Some((x, y)) = g.project(lon, lat) {
                plot(x, y);
            }
        }
        Projection::Mercator(v) => {
            for offset in WRAP_OFFSETS {
                let ((x, y), _) = v.project_wrapped(lon, lat, offset);
                plot(x, y);
            }
        }
    }
}

pub(super) fn wave(field: &Field, projection: &Projection, area: Rect, buf: &mut Buffer) {
    let t = field.progress();
    if t >= 1.0 {
        return;
    }
    let color = match field.weapon {
        WeaponType::Nuke => (255, 176, 70),
        WeaponType::Water => (100, 205, 255),
        WeaponType::Emp => (145, 235, 255),
        WeaponType::Life => (190, 255, 110),
        WeaponType::Bio => (105, 255, 155),
        WeaponType::Chem => (245, 140, 255),
        WeaponType::Tornado => (175, 200, 220),
        WeaponType::Frost => (160, 235, 255),
        WeaponType::Meteor => (255, 135, 55),
    };
    let rings = if matches!(field.weapon, WeaponType::Water | WeaponType::Emp) {
        3
    } else {
        1
    };
    let projected = projection.deg_to_pixels(field.front_km() / 111.0);
    let samples = ((projected * 7.0) as usize).clamp(64, 768);
    for ring in 0..rings {
        let age = t - ring as f64 * 0.1;
        if age <= 0.0 {
            continue;
        }
        let radius = field.radius_km * 1.9 * (1.0 - (1.0 - age).powi(3));
        for sample in 0..samples {
            let bearing = TAU * sample as f64 / samples as f64;
            let phase = field.lon + field.lat;
            let warp = if field.weapon == WeaponType::Emp {
                if (bearing*5.0+phase+t*3.0).sin() < -0.2 { continue; }
                1.0 + 0.08*(bearing*7.0+phase).sin() + 0.035*(bearing*17.0).sin()
            } else if field.weapon == WeaponType::Water {
                1.0 + 0.035*(bearing*5.0+t*4.0).sin()
            } else { 1.0 };
            let (lon, lat) = destination(field.lon, field.lat, radius*warp, bearing);
            dot(
                lon,
                lat,
                projection,
                area,
                buf,
                color,
                (1.0 - t) * (1.0 - age) * 0.85,
            );
        }
    }
}

pub(super) fn residue(reaction: &Reaction, projection: &Projection, area: Rect, buf: &mut Buffer) {
    let t = reaction.age as f64 / reaction.lifetime() as f64;
    let seed = reaction.lon.to_bits() ^ reaction.lat.to_bits().rotate_left(19);
    let strength = reaction.strength as f64 / 255.0;
    for i in 0..12u64 {
        let phase = rand_simple(seed.wrapping_add(i * 3));
        let angle = phase * TAU;
        let spread = 0.3 + rand_simple(seed.wrapping_add(i * 3 + 1));
        let (distance, bearing, color) = match reaction.kind {
            ReactionKind::Steam => {
                // Initial embers yield to a widening, northward-curling pale veil.
                let color = if t < 0.16 && i % 4 == 0 {
                    (255, 160, 65)
                } else {
                    (185, 220, 230)
                };
                let east =
                    angle.cos() * spread * (0.6 + t * 0.3) + (angle + t * 5.0).sin() * t * 0.8;
                let north = angle.sin() * spread * 0.6 + t * 2.8;
                (
                    reaction.radius_km * east.hypot(north),
                    east.atan2(north),
                    color,
                )
            }
            ReactionKind::Bloom => {
                let color = if i % 3 == 0 {
                    (255, 225, 110)
                } else {
                    (100, 235, 120)
                };
                (
                    reaction.radius_km * (spread * 0.7 + t * 1.5),
                    angle + t * 2.5,
                    color,
                )
            }
            ReactionKind::Scorch => {
                let color = if i % 3 == 0 {
                    (105, 210, 90)
                } else if t < 0.45 {
                    (255, 115, 40)
                } else {
                    (125, 90, 65)
                };
                (reaction.radius_km * (spread + t * 1.8), angle - t, color)
            }
        };
        let (lon, lat) = destination(reaction.lon, reaction.lat, distance, bearing);
        let fade = (1.0 - t).max(0.0).sqrt() * strength * (0.6 + phase * 0.4);
        dot(lon, lat, projection, area, buf, color, fade);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::Explosion,
        map::{GlobeViewport, Viewport},
    };

    #[test]
    fn waves_survive_offscreen_centers_and_clip_in_both_projections() {
        let area = Rect::new(4, 3, 80, 30);
        let field = Field::new(&Explosion { seed: 0,
            lon: 95.0,
            lat: 0.0,
            frame: 30,
            radius_km: 1500.0,
            weapon_type: WeaponType::Water,
        });
        let globe = GlobeViewport::new(0.0, 0.0, 70.0, 160, 120);
        assert!(globe.project(field.lon, field.lat).is_none());
        let projections = [
            Projection::Globe(globe),
            Projection::Mercator(Viewport::new(75.0, 0.0, 8.0, 160, 120)),
        ];
        for projection in projections {
            let mut buf = Buffer::empty(Rect::new(0, 0, 90, 40));
            wave(&field, &projection, area, &mut buf);
            assert!(buf.content.iter().any(|c| c.symbol() != " "));
            for y in 0..40 {
                for x in 0..90 {
                    if !area.contains((x, y).into()) {
                        assert_eq!(buf[(x, y)].symbol(), " ");
                    }
                }
            }
        }
    }
}
