//! Sparse sub-cell shock fronts and coherent particle trails.
//! Work is capped per explosion, independent of its projected screen radius.
use super::ExplosionRender;
use crate::{app::WeaponType, hash::rand_simple, map::GlobeViewport};
use ratatui::{buffer::Buffer, layout::Rect, style::Color};
use std::f32::consts::TAU;

struct Ink<'a> {
    area: Rect,
    buf: &'a mut Buffer,
    globe: Option<&'a GlobeViewport>,
    origin: (f32, f32),
}

impl Ink<'_> {
    // Coordinates use horizontal character widths, with terminal aspect correction.
    fn dot(&mut self, dx: f32, dy: f32, color: (u8, u8, u8), light: f32) {
        if light < 0.04 {
            return;
        }
        let sx = ((self.origin.0 + dx) * 2.0).floor() as i32;
        let sy = (self.origin.1 * 4.0 + dy * 2.0).floor() as i32;
        let cx = sx.div_euclid(2);
        let cy = sy.div_euclid(4);
        if cx < self.area.x as i32
            || cy < self.area.y as i32
            || cx >= self.area.right() as i32
            || cy >= self.area.bottom() as i32
        {
            return;
        }
        if let Some(g) = self.globe {
            if g.pixel_to_sphere_point(sx - self.area.x as i32 * 2, sy - self.area.y as i32 * 4)
                .is_none()
            {
                return;
            }
        }
        let cell = &mut self.buf[(cx as u16, cy as u16)];
        let old = cell.symbol().chars().next().unwrap_or(' ') as u32;
        let mask =
            [[1, 2, 4, 64], [8, 16, 32, 128]][sx.rem_euclid(2) as usize][sy.rem_euclid(4) as usize];
        let bits = if (0x2800..=0x28ff).contains(&old) {
            old - 0x2800
        } else {
            0
        };
        let light = light.clamp(0.0, 1.0);
        let mut rgb = (
            (color.0 as f32 * light) as u8,
            (color.1 as f32 * light) as u8,
            (color.2 as f32 * light) as u8,
        );
        // Multiple samples in one cell must not dim an earlier trail head.
        if bits != 0 {
            if let Color::Rgb(r, g, b) = cell.fg {
                rgb = (rgb.0.max(r), rgb.1.max(g), rgb.2.max(b));
            }
        }
        cell.set_char(char::from_u32(0x2800 + (bits | mask)).unwrap())
            .set_fg(Color::Rgb(rgb.0, rgb.1, rgb.2));
    }
}

pub(super) fn render(
    exp: &ExplosionRender,
    x: u16,
    y: u16,
    area: Rect,
    buf: &mut Buffer,
    globe: Option<&GlobeViewport>,
    particles: bool,
) {
    if matches!(exp.weapon_type, WeaponType::Tornado | WeaponType::Frost | WeaponType::Meteor) { return; }
    let t = exp.frame as f32 / exp.weapon_type.max_frames() as f32;
    if t >= 1.0 || exp.radius == 0 {
        return;
    }
    let radius = exp.radius as f32;
    let fade = ((1.0 - t) * 2.5).min(1.0);
    let color = match exp.weapon_type {
        WeaponType::Nuke => (255, 176, 70),
        WeaponType::Emp => (145, 235, 255),
        WeaponType::Water => (100, 205, 255),
        WeaponType::Life => (190, 255, 110),
        WeaponType::Bio => (105, 255, 155),
        WeaponType::Chem => (245, 140, 255),
        WeaponType::Tornado => (175, 200, 220),
        WeaponType::Frost => (160, 235, 255),
        WeaponType::Meteor => (255, 135, 55),
    };
    let mut ink = Ink {
        area: area.intersection(buf.area),
        buf,
        globe,
        origin: (x as f32 + 0.5, y as f32 + 0.5),
    };
    if !particles {
        let t = exp.frame as f32 / exp.weapon_type.front_frames() as f32;
        if t >= 1.0 { return; }
        // A fast leading front followed by delayed echoes, drawn below the body.
        let rings = if matches!(exp.weapon_type, WeaponType::Water | WeaponType::Emp) {
            3
        } else {
            1
        };
        for ring in 0..rings {
            let age = t - ring as f32 * 0.10;
            if age <= 0.0 {
                continue;
            }
            let r = radius * 1.9 * (1.0 - (1.0 - age).powi(3));
            let samples = ((r * 16.0) as usize).clamp(32, 512);
            for i in 0..samples {
                let a = TAU * i as f32 / samples as f32;
                let warp = match exp.weapon_type {
                    WeaponType::Emp => {
                        let phase = (exp.lon + exp.lat) as f32;
                        if (a*5.0+phase+t*3.0).sin() < -0.2 { continue; }
                        1.0 + 0.08*(a*7.0+phase).sin() + 0.035*(a*17.0).sin()
                    },
                    WeaponType::Water => 1.0 + 0.035*(a*5.0+t*4.0).sin(),
                    WeaponType::Life => 1.0 + 0.09 * (a * 6.0 + t * 5.0).sin(),
                    WeaponType::Bio | WeaponType::Chem => 1.0 + 0.06 * (a * 5.0 - t * 8.0).sin(),
                    _ => 1.0,
                };
                ink.dot(
                    a.cos() * r * warp,
                    a.sin() * r * warp * 0.65,
                    color,
                    fade * (1.0 - age) * 0.8,
                );
            }
        }
        return;
    }
    // Seed trajectories by impact location, never by frame: sparks travel instead
    // of teleporting. Reconstruct short tails without storing particle state.
    let seed = exp.lon.to_bits() ^ exp.lat.to_bits().rotate_left(23);
    for i in 0..24u64 {
        let a = rand_simple(seed.wrapping_add(i * 3)) as f32 * TAU;
        let speed = 0.65 + rand_simple(seed.wrapping_add(i * 3 + 1)) as f32 * 0.8;
        let delay = rand_simple(seed.wrapping_add(i * 3 + 2)) as f32 * 0.16;
        for tail in (0..6).rev() {
            let age = t - delay - tail as f32 * 0.012;
            if age <= 0.0 {
                continue;
            }
            let travel = radius * speed * age;
            let (dx, dy) = match exp.weapon_type {
                WeaponType::Tornado | WeaponType::Frost | WeaponType::Meteor => unreachable!(),
                WeaponType::Nuke => (
                    a.cos() * travel * 2.0,
                    -a.sin().abs() * travel * 3.5 + radius * age * age * 1.8,
                ),
                WeaponType::Water => (
                    a.cos() * travel * 2.4,
                    -a.sin().abs() * travel * 2.8 + radius * age * age * 3.0,
                ),
                WeaponType::Emp => {
                    let bend = (age * 35.0 + a * 4.0).sin() * radius * 0.045;
                    (
                        a.cos() * travel * 2.2 - a.sin() * bend,
                        a.sin() * travel * 1.4 + a.cos() * bend,
                    )
                }
                WeaponType::Life => {
                    let turn = a + age * 3.5;
                    (
                        turn.cos() * travel * 1.8,
                        turn.sin() * travel - radius * age * 1.4,
                    )
                }
                WeaponType::Bio | WeaponType::Chem => {
                    let turn = a - age * 2.0;
                    (
                        turn.cos() * travel * 1.7 + radius * age * 0.4,
                        turn.sin() * travel * 0.8 - radius * age * 1.8,
                    )
                }
            };
            ink.dot(dx, dy, color, fade * (1.0 - tail as f32 / 7.0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accents_clip_to_offset_viewport_and_finish_cleanly() {
        let area = Rect::new(5, 3, 20, 12);
        for weapon_type in [
            WeaponType::Nuke,
            WeaponType::Emp,
            WeaponType::Water,
            WeaponType::Life,
            WeaponType::Bio,
            WeaponType::Chem,
        ] {
            let mut exp = ExplosionRender { seed: 0,
                x: 5,
                y: 3,
                frame: 12,
                radius: 20,
                weapon_type,
                lon: 10.0,
                lat: 20.0,
                radius_km: 500.0,
            };
            let mut buf = Buffer::empty(Rect::new(0, 0, 30, 20));
            render(&exp, 5, 3, area, &mut buf, None, false);
            render(&exp, 5, 3, area, &mut buf, None, true);
            assert!(buf.content.iter().any(|c| c.symbol() != " "));
            for y in 0..20 {
                for x in 0..30 {
                    if !area.contains((x, y).into()) {
                        assert_eq!(buf[(x, y)].symbol(), " ");
                    }
                }
            }
            exp.frame = weapon_type.max_frames();
            let mut ended = Buffer::empty(area);
            render(&exp, 5, 3, area, &mut ended, None, false);
            render(&exp, 5, 3, area, &mut ended, None, true);
            assert!(ended.content.iter().all(|c| c.symbol() == " "));
        }
    }
}
