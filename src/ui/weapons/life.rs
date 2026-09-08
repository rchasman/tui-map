//! Uneven shoots branch into leaves and buds, then gently settle back to earth.
use super::ExplosionRender;
use crate::{hash::rand_simple, map::GlobeViewport};
use ratatui::{buffer::Buffer, layout::Rect, style::Color};

pub fn render(
    exp: &ExplosionRender,
    x: u16,
    y: u16,
    area: Rect,
    _frame: u64,
    buf: &mut Buffer,
    globe: Option<&GlobeViewport>,
) {
    if exp.radius == 0 || exp.frame >= exp.weapon_type.max_frames() {
        return;
    }
    let t = exp.frame as f32 / exp.weapon_type.max_frames() as f32;
    let seed = exp.lon.to_bits() ^ exp.lat.to_bits().rotate_left(17);
    let random = |i| rand_simple(seed.wrapping_add(i)) as f32;
    let fade = 1.0 - ((t - 0.76) / 0.24).clamp(0.0, 1.0);
    let radius = exp.radius as f32;
    let plants: Vec<_> = (0..9u64)
        .map(|i| {
            let foot = (i as f32 - 4.0) * 0.24 + (random(i * 5) - 0.5) * 0.13;
            let growth = ((t - random(i * 5 + 1) * 0.10) / 0.25).clamp(0.0, 1.0);
            let height = (0.45 + random(i * 5 + 2) * 1.4) * (1.0 - (1.0 - growth).powi(2));
            let lean = (random(i * 5 + 3) - 0.5) * 0.5 + (t * 3.0 + i as f32).sin() * 0.07;
            (foot, height.max(0.001), lean, random(i * 5 + 4))
        })
        .collect();
    let clip = area.intersection(buf.area);
    let left = (x as i32 - (radius * 1.65).ceil() as i32).max(clip.x as i32);
    let right = (x as i32 + (radius * 1.65).ceil() as i32 + 1).min(clip.right() as i32);
    let top = (y as i32 - radius.ceil() as i32).max(clip.y as i32);
    let bottom = (y as i32 + (radius * 0.2).ceil() as i32 + 1).min(clip.bottom() as i32);
    for py in top..bottom {
        for px in left..right {
            let mut bits = 0u32;
            let mut green = 0.0f32;
            let mut gold = 0.0f32;
            for sy in 0..4 {
                for sx in 0..2 {
                    if globe.is_some_and(|g| {
                        g.pixel_to_sphere_point(
                            px * 2 + sx - area.x as i32 * 2,
                            py * 4 + sy - area.y as i32 * 4,
                        )
                        .is_none()
                    }) {
                        continue;
                    }
                    let dx = (px as f32 + (sx as f32 + 0.5) * 0.5 - x as f32 - 0.5) / radius;
                    let z = (y as f32 + 0.5 - py as f32 - (sy as f32 + 0.5) * 0.25) * 2.0 / radius;
                    let mut leaf = 0.0f32;
                    let mut bud = 0.0f32;
                    for &(foot, height, lean, phase) in &plants {
                        if z < 0.0 || z > height + 0.12 {
                            continue;
                        }
                        let trunk = foot
                            + lean * (z / height).clamp(0.0, 1.0)
                            + (z * 4.0 + phase * 6.0).sin() * 0.035;
                        if z < height {
                            leaf = leaf.max((1.0 - (dx - trunk).abs() / 0.035).max(0.0) * 0.65);
                        }
                        for branch in 0..3 {
                            let level = (0.35 + branch as f32 * 0.25) * height;
                            let side = if (branch + (phase * 10.0) as i32) % 2 == 0 {
                                1.0
                            } else {
                                -1.0
                            };
                            let reach = (0.12 + phase * 0.14) * (height / 0.5).min(1.0);
                            let center = foot + lean * level / height + side * reach;
                            let ellipse = ((dx - center) / (0.10 + phase * 0.09)).powi(2)
                                + ((z - level) / (0.07 + phase * 0.045)).powi(2);
                            leaf = leaf.max((1.0 - ellipse).max(0.0));
                            let along = ((z - (level - 0.13)) / 0.13).clamp(0.0, 1.0);
                            if z > level - 0.13 && z < level {
                                leaf = leaf.max(
                                    (1.0 - (dx - (trunk + side * reach * along)).abs() / 0.025)
                                        .max(0.0)
                                        * 0.5,
                                );
                            }
                        }
                        let tip = foot + lean;
                        let flower = ((dx - tip) / 0.075).powi(2) + ((z - height) / 0.075).powi(2);
                        bud =
                            bud.max((1.0 - flower).max(0.0) * ((t - 0.16) / 0.14).clamp(0.0, 1.0));
                    }
                    // Patchy moss joins the stems, without a circular carpet.
                    let moss = super::organic::noise(dx * 7.0, z * 10.0, seed);
                    if z.abs() < 0.13 && dx.abs() < 1.25 && moss > 0.45 {
                        leaf = leaf.max((moss - 0.45) * 1.5 * (t / 0.15).min(1.0));
                    }
                    let light = leaf.max(bud) * fade;
                    if light < 0.06 {
                        continue;
                    }
                    bits |= [[1, 2, 4, 64], [8, 16, 32, 128]][sx as usize][sy as usize];
                    green = green.max(leaf * fade);
                    gold = gold.max(bud * fade);
                }
            }
            if bits != 0 {
                buf[(px as u16, py as u16)]
                    .set_char(char::from_u32(0x2800 + bits).unwrap())
                    .set_fg(Color::Rgb(
                        (green * 55.0 + gold * 200.0).min(255.0) as u8,
                        (green * 190.0 + gold * 100.0).min(245.0) as u8,
                        (green * 65.0 + gold * 60.0).min(150.0) as u8,
                    ));
            }
        }
    }
}
