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
            let random = |i| crate::motion::variation(exp.seed, i) as f32;
            let phase = random(10) * TAU;
            let growth = (t / 0.14).min(1.0);
            let height = (2.0 + random(11) * 1.6) * growth;
            let width = 0.45 + random(12) * 0.9;
            let curl = 1.5 + random(13) * 2.0;
            // Each storm can be rope-like, broad or crooked. The spine flexes
            // continuously while torn ribbons and low cloud wrap around it.
            for ribbon in 0..7 {
                for step in 0..130 {
                    let h = step as f32 / 129.0;
                    let a = h * TAU * curl - t * TAU * (7.0 + random(14) * 5.0)
                        + ribbon as f32 * TAU / 7.0
                        + phase;
                    if (h * 23.0 + a * 1.3 + phase).sin() < -0.8 {
                        continue;
                    }
                    let radius =
                        (0.05 + random(15) * 0.12 + h.powf(0.8 + random(16) * 1.8) * width)
                            * growth
                            * (1.0 + (h * 12.0 - t * 17.0 + phase).sin() * 0.13);
                    let bend = h * h * (0.18 + random(17) * 0.55);
                    let bx = (h * 3.0 + t * 7.0 + phase).sin() * bend;
                    let by = (h * 4.0 - t * 5.0 + phase).cos() * bend;
                    let light = fade * (0.42 + 0.48 * a.sin().abs());
                    dot(
                        a.cos() * radius + bx,
                        a.sin() * radius + by,
                        h * height,
                        [175.0 * light, 195.0 * light, 210.0 * light],
                    );
                }
            }
            // A revolving wall cloud and an uneven airborne debris skirt make
            // the vortex read as a storm, rather than an isolated perfect cone.
            for i in 0..300u64 {
                let a = random(i + 100) * TAU - t * 9.0;
                let r = (0.4 + random(i + 500).sqrt() * 1.2) * width * growth;
                let z = height + (a * 3.0 + phase).sin() * 0.13 + random(i + 900) * 0.18;
                dot(
                    a.cos() * r,
                    a.sin() * r,
                    z,
                    [85.0 * fade, 108.0 * fade, 125.0 * fade],
                );
            }
            for i in 0..180u64 {
                let age = (t * (1.8 + random(i + 1100)) + random(i + 1400)).fract();
                let a = random(i + 1800) * TAU - age * 12.0;
                let r = (0.25 + random(i + 2100) * 1.2) * (1.0 - age * 0.6) * growth;
                let light = fade * (std::f32::consts::PI * age).sin();
                dot(
                    a.cos() * r,
                    a.sin() * r,
                    age * (0.3 + random(i + 2500)),
                    [155.0 * light, 135.0 * light, 110.0 * light],
                );
            }
        }
        WeaponType::Frost => {
            let random = |i| crate::motion::variation(exp.seed, i) as f32;
            let arms = 8 + (random(60) * 6.0) as u64;
            let growth = (t / 0.25).min(1.0);
            for arm in 0..arms {
                let a = arm as f32 * TAU / arms as f32
                    + random(61) * TAU
                    + (random(arm + 62) - 0.5) * 0.18;
                for step in 0..65 {
                    let r = step as f32 / 64.0 * (1.1 + random(arm + 80) * 0.9) * growth;
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
            let random = |i| crate::motion::variation(exp.seed, i) as f32;
            let impact = crate::motion::meteor_impact_frame(exp.seed) as f32;
            let heading = random(20) * TAU;
            let length = 2.2 + random(21) * 2.0;
            let altitude = 3.5 + random(22) * 1.5;
            if (exp.frame as f32) < impact {
                let progress = exp.frame as f32 / impact;
                for fragment in 0..(2 + (random(23) * 4.0) as u64) {
                    for tail in 0..36 {
                        let previous = exp.frame as f32 - tail as f32 * 0.45;
                        if previous < 0.0 {
                            continue;
                        }
                        let left = (1.0 - previous / impact).max(0.0);
                        let split = if fragment == 0 {
                            0.0
                        } else {
                            (progress - 0.45).max(0.0) * (random(fragment + 50) - 0.5) * 0.9
                        };
                        let along = length * left;
                        let x = heading.cos() * along - heading.sin() * split;
                        let y = heading.sin() * along + heading.cos() * split;
                        let z = altitude * left.powf(0.8) + split.abs() * 0.5;
                        let light = (1.0 - tail as f32 / 37.0).powi(2);
                        let color = if tail < 3 {
                            [255.0, 245.0, 210.0]
                        } else {
                            [255.0, 125.0 + random(24) * 65.0, 40.0]
                        };
                        dot(x, y, z, color.map(|c| c * light));
                        if fragment == 0 && tail < 3 {
                            for side in [-0.035, 0.035] {
                                dot(x + side, y, z + side, color);
                            }
                        }
                    }
                }
                return;
            }
            let t = (exp.frame as f32 - impact) / (exp.weapon_type.max_frames() as f32 - impact);
            let flash = (1.0 - t / 0.07).max(0.0);
            for ray in 0..(32 + (random(25) * 25.0) as u64) {
                let a = random(ray + 100) * TAU;
                let speed = 0.6 + random(ray + 300) * 1.4;
                for tail in 0..18 {
                    let age = (t - tail as f32 * 0.006).max(0.0);
                    let r = age * speed * 2.2;
                    let bias = 1.0 + 0.35 * (a - heading).cos();
                    let z = (age * speed * 6.0 - age * age * 8.0).max(0.0);
                    let light = fade * (1.0 - tail as f32 / 22.0);
                    dot(
                        a.cos() * r * bias,
                        a.sin() * r * bias,
                        z,
                        [255.0 * light, (155.0 - t * 70.0) * light, 45.0 * light],
                    );
                }
            }
            for i in 0..220 {
                let a = i as f32 * TAU / 220.0;
                let r = (t / 0.15).min(1.0)
                    * (0.7 + random(26) * 0.3 + 0.08 * (a * 9.0 + random(27) * TAU).sin());
                let x = a.cos() * r;
                let y = a.sin() * r * (0.65 + random(28) * 0.3);
                dot(
                    x * heading.cos() - y * heading.sin(),
                    x * heading.sin() + y * heading.cos(),
                    0.02,
                    [
                        220.0 * fade,
                        (85.0 + 160.0 * flash) * fade,
                        (35.0 + 210.0 * flash) * fade,
                    ],
                );
                if flash > 0.0 {
                    let r = i as f32 / 220.0 * (0.12 + (1.0 - flash) * 0.7);
                    dot(
                        a.cos() * r,
                        a.sin() * r,
                        r * 0.3,
                        [255.0 * flash, 250.0 * flash, 225.0 * flash],
                    );
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn effect(weapon_type: WeaponType, seed: u64, frame: u8) -> ExplosionRender {
        ExplosionRender {
            seed,
            frame,
            weapon_type,
            lon: 0.0,
            lat: 0.0,
            radius_km: 600.0,
            x: 40,
            y: 25,
            radius: 12,
        }
    }

    fn points(exp: &ExplosionRender) -> Vec<(f32, f32, f32, [f32; 3])> {
        let mut points = Vec::new();
        samples(exp, |x, y, z, color| points.push((x, y, z, color)));
        points
    }

    #[test]
    fn meteor_descends_before_ground_impact_and_varies_by_seed() {
        for seed in [0, 1, 42, 1234] {
            let impact = crate::motion::meteor_impact_frame(seed);
            let high = points(&effect(WeaponType::Meteor, seed, 3));
            let low = points(&effect(WeaponType::Meteor, seed, impact - 1));
            assert!(high.iter().all(|p| p.2 > 0.0));
            assert!(low.iter().all(|p| p.2 > 0.0));
            assert!(high[0].2 > low[0].2);
            let hit = points(&effect(WeaponType::Meteor, seed, impact));
            assert!(hit.iter().any(|p| p.2 == 0.0));
            assert!(points(&effect(
                WeaponType::Meteor,
                seed,
                WeaponType::Meteor.max_frames()
            ))
            .is_empty());
        }
        assert_ne!(
            points(&effect(WeaponType::Meteor, 1, 15)),
            points(&effect(WeaponType::Meteor, 42, 15))
        );
    }

    #[test]
    fn storms_have_repeatable_variations_and_evolve_without_reseeding() {
        let a = effect(WeaponType::Tornado, 1, 90);
        assert_eq!(points(&a), points(&a));
        assert_ne!(points(&a), points(&effect(WeaponType::Tornado, 42, 90)));
        assert_ne!(points(&a), points(&effect(WeaponType::Tornado, 1, 91)));
    }
}
