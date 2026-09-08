//! Bounded particle geometry shared by the flat map and raised globe renderer.
use super::ExplosionRender;
use crate::{app::WeaponType, map::GlobeViewport};
use ratatui::{buffer::Buffer, layout::Rect, style::Color};
use std::f32::consts::TAU;

pub(super) fn samples(exp: &ExplosionRender, mut dot: impl FnMut(f32, f32, f32, [f32; 3])) {
    if exp.frame >= exp.weapon_type.max_frames() || exp.radius_km <= 0.0 {
        return;
    }
    let t = exp.frame as f32 / exp.weapon_type.max_frames() as f32;
    let fade = ((1.0 - t) / 0.25).min(1.0);
    match exp.weapon_type {
        WeaponType::Tornado => {
            let growth = (t / 0.14).min(1.0);
            // Helical ribbons widen from a narrow foot to a ragged funnel top.
            for ribbon in 0..7 {
                for step in 0..130 {
                    let h = step as f32 / 129.0;
                    let angle = h * TAU * 3.0 - t * TAU * 9.0 + ribbon as f32 * TAU / 7.0;
                    let radius = (0.09 + h * h * 0.85) * growth;
                    let lean = (h * 3.0 + t * 8.0).sin() * h * 0.22;
                    let light = fade * (0.5 + 0.5 * angle.sin().abs());
                    dot(
                        angle.cos() * radius + lean,
                        angle.sin() * radius,
                        h * 2.8 * growth,
                        [175.0 * light, 200.0 * light, 220.0 * light],
                    );
                }
            }
            for i in 0..180 {
                let a = i as f32 * 2.4 - t * 25.0;
                let r = 0.2 + (i % 31) as f32 / 31.0 * 1.4;
                dot(
                    a.cos() * r * growth,
                    a.sin() * r * growth,
                    0.06,
                    [120.0 * fade, 145.0 * fade, 165.0 * fade],
                );
            }
        }
        WeaponType::Frost => {
            let growth = (t / 0.25).min(1.0);
            for arm in 0..12 {
                let a = arm as f32 * TAU / 12.0;
                for step in 0..65 {
                    let r = step as f32 / 64.0 * 1.8 * growth;
                    dot(
                        a.cos() * r,
                        a.sin() * r,
                        0.04,
                        [150.0 * fade, 220.0 * fade, 255.0 * fade],
                    );
                    for side in [-1.0, 1.0] {
                        let branch = ((step % 16) as f32 / 16.0) * 0.28 * growth;
                        let turn = a + side * TAU / 6.0;
                        dot(
                            a.cos() * r + turn.cos() * branch,
                            a.sin() * r + turn.sin() * branch,
                            branch * 0.5,
                            [190.0 * fade, 240.0 * fade, 255.0 * fade],
                        );
                    }
                }
            }
        }
        WeaponType::Meteor => {
            // The impact begins immediately, matching simulation damage. Ejecta
            // rise ballistically, then fall into a cooling, broken crater rim.
            for ray in 0..42 {
                let a = ray as f32 * 2.39996;
                let speed = 0.8 + (ray % 7) as f32 * 0.18;
                for tail in 0..18 {
                    let age = (t - tail as f32 * 0.006).max(0.0);
                    let r = age * speed * 2.2;
                    let z = (age * speed * 6.0 - age * age * 8.0).max(0.0);
                    let light = fade * (1.0 - tail as f32 / 22.0);
                    dot(
                        a.cos() * r,
                        a.sin() * r,
                        z,
                        [255.0 * light, (155.0 - t * 70.0) * light, 45.0 * light],
                    );
                }
            }
            for i in 0..220 {
                let a = i as f32 * TAU / 220.0;
                let r = (t / 0.15).min(1.0) * (0.8 + 0.08 * (a * 13.0).sin());
                dot(
                    a.cos() * r,
                    a.sin() * r,
                    0.02,
                    [220.0 * fade, 85.0 * fade, 35.0 * fade],
                );
            }
        }
        _ => {}
    }
}

pub(super) fn render(
    exp: &ExplosionRender,
    x: u16,
    y: u16,
    area: Rect,
    buf: &mut Buffer,
    _globe: Option<&GlobeViewport>,
) {
    if exp.radius == 0 {
        return;
    }
    let clip = area.intersection(buf.area);
    samples(exp, |east, north, up, color| {
        let sx = ((x as f32 + 0.5 + east * exp.radius as f32) * 2.0).floor() as i32;
        let sy =
            ((y as f32 + 0.5 - (north * 0.45 + up) * exp.radius as f32 * 0.5) * 4.0).floor() as i32;
        let (cx, cy) = (sx.div_euclid(2), sy.div_euclid(4));
        if cx < clip.x as i32
            || cy < clip.y as i32
            || cx >= clip.right() as i32
            || cy >= clip.bottom() as i32
        {
            return;
        }
        let cell = &mut buf[(cx as u16, cy as u16)];
        let old = cell.symbol().chars().next().unwrap_or(' ') as u32;
        let bits = if (0x2800..=0x28ff).contains(&old) {
            old - 0x2800
        } else {
            0
        };
        let bit =
            [[1, 2, 4, 64], [8, 16, 32, 128]][sx.rem_euclid(2) as usize][sy.rem_euclid(4) as usize];
        let mut rgb = color.map(|c| c.clamp(0.0, 255.0) as u8);
        if let Color::Rgb(r, g, b) = cell.fg {
            if bits != 0 {
                rgb = [rgb[0].max(r), rgb[1].max(g), rgb[2].max(b)];
            }
        }
        cell.set_char(char::from_u32(0x2800 + (bits | bit)).unwrap())
            .set_fg(Color::Rgb(rgb[0], rgb[1], rgb[2]));
    });
}
