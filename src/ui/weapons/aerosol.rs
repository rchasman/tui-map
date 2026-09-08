//! Low spore wisps and heavier chemical plumes share continuous material flow.
use super::{organic::noise, ExplosionRender};
use crate::{app::WeaponType, hash::rand_simple, map::GlobeViewport};
use ratatui::{buffer::Buffer, layout::Rect, style::Color};

pub(super) fn render(
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
    let chemical = exp.weapon_type == WeaponType::Chem;
    let t = exp.frame as f32 / exp.weapon_type.max_frames() as f32;
    let seed = exp.lon.to_bits() ^ exp.lat.to_bits().rotate_left(23);
    let wind = (rand_simple(seed) as f32 - 0.5) * 0.8;
    let spread = 0.15 + 1.5 * (1.0 - (-t * 6.0).exp());
    let fade = (1.0 - ((t - 0.55) / 0.45).clamp(0.0, 1.0)).powi(2);
    let radius = exp.radius as f32;
    let clip = area.intersection(buf.area);
    let left = (x as i32 - (radius * 2.5).ceil() as i32).max(clip.x as i32);
    let right = (x as i32 + (radius * 2.5).ceil() as i32 + 1).min(clip.right() as i32);
    let top = (y as i32 - (radius * 1.3).ceil() as i32).max(clip.y as i32);
    let bottom = (y as i32 + (radius * 0.9).ceil() as i32 + 1).min(clip.bottom() as i32);
    for py in top..bottom {
        for px in left..right {
            let mut bits = 0u32;
            let mut sum = [0.0f32; 3];
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
                    let dy = (py as f32 + (sy as f32 + 0.5) * 0.25 - y as f32 - 0.5) * 2.0 / radius;
                    let u = dx - wind * t;
                    let curl = noise(u * 2.1 - t * 0.9, dy * 2.1 + t * 0.4, seed) - 0.5;
                    let v = dy + if chemical { 0.30 * t } else { 0.1 * t };
                    let height = if chemical { 0.9 } else { 0.42 };
                    // Unequal overlapping lobes form a plume, not a perfect dome.
                    let mut shape = -10.0f32;
                    for i in 0..3u64 {
                        let offset = (i as f32 - 1.0) * spread * 0.55;
                        let lift = (rand_simple(seed.wrapping_add(i + 7)) as f32 - 0.5) * 0.35;
                        let q = ((u - offset) / (spread * (0.65 + i as f32 * 0.08))).powi(2)
                            + ((v + lift * t + curl * 0.22) / (spread * height)).powi(2);
                        shape = shape.max(1.0 - q);
                    }
                    let material =
                        noise(u * 4.0 - t * 1.8, v * 4.0 + curl + t * 0.55, seed ^ 0xA317);
                    let fine = noise(u * 10.0 - t * 2.2, v * 8.0 + t, seed ^ 0xF013);
                    let mut density = shape + (material - 0.5) * 0.9 + (fine - 0.5) * 0.22;
                    if chemical && dy > 0.0 {
                        // Thin descending rivulets remain attached to the lower edge.
                        let rivulet = noise(u * 12.0, 0.0, seed ^ 0xD21);
                        density = density.max((rivulet - 0.72) * 4.0 - (dy - t * 0.7).abs() * 2.0);
                    }
                    let alpha = (density * 0.8).clamp(0.0, 1.0) * fade;
                    if alpha < 0.09 {
                        continue;
                    }
                    bits |= [[1, 2, 4, 64], [8, 16, 32, 128]][sx as usize][sy as usize];
                    let shade = (0.45 + material * 0.55) * alpha.sqrt();
                    let flash = (1.0 - t / 0.10).max(0.0) * shape.max(0.0) * 0.45;
                    let color = if chemical {
                        [170.0, 48.0, 225.0]
                    } else {
                        [65.0, 210.0, 95.0]
                    };
                    for channel in 0..3 {
                        sum[channel] += color[channel] * shade + flash * 70.0;
                    }
                }
            }
            if bits != 0 {
                let count = bits.count_ones() as f32;
                buf[(px as u16, py as u16)]
                    .set_char(char::from_u32(0x2800 + bits).unwrap())
                    .set_fg(Color::Rgb(
                        (sum[0] / count).min(255.0) as u8,
                        (sum[1] / count).min(255.0) as u8,
                        (sum[2] / count).min(255.0) as u8,
                    ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plumes_follow_material_age_and_vary_by_impact() {
        let area = Rect::new(0, 0, 80, 40);
        for weapon_type in [WeaponType::Bio, WeaponType::Chem] {
            let mut exp = ExplosionRender { seed: 0,
                x: 40,
                y: 20,
                frame: 24,
                radius: 10,
                weapon_type,
                lon: 10.0,
                lat: 20.0,
                radius_km: 500.0,
            };
            let mut a = Buffer::empty(area);
            let mut b = Buffer::empty(area);
            render(&exp, 40, 20, area, 24, &mut a, None);
            render(&exp, 40, 20, area, 999, &mut b, None);
            assert_eq!(a, b, "global time must not reshuffle the plume");
            exp.lon = 55.0;
            let mut different = Buffer::empty(area);
            render(&exp, 40, 20, area, 24, &mut different, None);
            assert_ne!(
                a, different,
                "different impacts should not share a silhouette"
            );
            let globe = GlobeViewport::new(0.0, 0.0, 25.0, 160, 160);
            let mut edge = Buffer::empty(area);
            render(&exp, 52, 20, area, 24, &mut edge, Some(&globe));
            for y in 0..40 {
                for x in 0..80 {
                    if globe.pixel_to_sphere_point(x * 2, y * 4).is_none()
                        && globe.pixel_to_sphere_point(x * 2 + 1, y * 4 + 3).is_none()
                    {
                        // A cell may straddle the globe, so inspect every dot.
                        let any_inside = (0..2).any(|sx| {
                            (0..4).any(|sy| {
                                globe
                                    .pixel_to_sphere_point(x * 2 + sx, y * 4 + sy)
                                    .is_some()
                            })
                        });
                        if !any_inside {
                            assert_eq!(edge[(x as u16, y as u16)].symbol(), " ");
                        }
                    }
                }
            }
        }
    }
}
