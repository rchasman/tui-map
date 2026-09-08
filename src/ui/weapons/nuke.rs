//! A buoyant fireball rolls into a mushroom cap, then cools into billowing smoke.
use super::ExplosionRender;
use crate::{hash::rand_simple, map::GlobeViewport};
use super::organic::noise;
use ratatui::{buffer::Buffer, layout::Rect, style::Color};

fn smooth(start: f32, end: f32, value: f32) -> f32 {
    let t = ((value - start) / (end - start)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[derive(Clone, Copy)]
struct Lobe {
    x: f32,
    z: f32,
    rx: f32,
    rz: f32,
}

// Geometry shared by every horizontal sample at this height.
struct PlumeRow {
    z: f32,
    cap_vertical: f32,
    lobe_vertical: [f32; 5],
    drift: f32,
    stem_width: f32,
    stem_bottom: f32,
    stem_top: f32,
    skirt_vertical: f32,
}

struct Plume {
    t: f32,
    lift: f32,
    width: f32,
    growth: f32,
    mushroom: f32,
    lobes: [Lobe; 5],
    seed: u64,
}

impl Plume {
    fn new(exp: &ExplosionRender) -> Self {
        let t = exp.frame as f32 / exp.weapon_type.max_frames() as f32;
        let growth = 1.0 - (1.0 - (t / 0.38).min(1.0)).powi(3);
        let lift = 0.12 + 2.5 * (1.0 - (1.0 - t).powi(2));
        let width = 0.24 + growth * 1.04 + t * 0.22;
        let mushroom = smooth(0.10, 0.38, t);
        let seed = exp.lon.to_bits() ^ exp.lat.to_bits().rotate_left(21);
        let wind = (rand_simple(seed) as f32 - 0.5) * 0.35;
        let lobes = std::array::from_fn(|i| {
            let variation = rand_simple(seed.wrapping_add(i as u64 + 11)) as f32 - 0.5;
            let offset = i as f32 - 2.0;
            let roll = (t * 5.0 + offset * 1.7).sin() * 0.12 * growth;
            Lobe {
                x: offset * width * 0.36 * mushroom + wind*t + variation*0.12*growth,
                z: lift + ((0.32 - offset.abs() * 0.05 + variation*0.2) * growth + roll) * mushroom,
                rx: width * (0.42 + variation*0.12 + 0.035 * (t * 4.0 + offset).sin()),
                rz: 0.18 + growth * (0.40 - offset.abs() * 0.035),
            }
        });
        Self {
            t,
            lift,
            width,
            growth,
            mushroom,
            lobes,
            seed,
        }
    }

    fn row(&self, z: f32) -> PlumeRow {
        let cap_radius =
            self.width * 0.9 * (1.0 - self.mushroom) + (0.18 + self.growth * 0.30) * self.mushroom;
        PlumeRow {
            z,
            cap_vertical: ((z - self.lift) / cap_radius).powi(2),
            lobe_vertical: self.lobes.map(|lobe| ((z - lobe.z) / lobe.rz).powi(2)),
            drift: (z * 4.0 - self.t * 5.0).sin() * 0.06 * self.growth
                + (rand_simple(self.seed) as f32 - 0.5)*0.18*z*self.t,
            stem_width: 0.11 + self.growth * 0.1 + smooth(0.3, self.lift.max(0.31), z) * 0.08,
            stem_bottom: (z - 0.015) * 5.0,
            stem_top: (self.lift - z) * 5.0,
            skirt_vertical: ((z - 0.055) / (0.09 + self.growth * 0.07)).powi(2),
        }
    }

    #[cfg(test)]
    fn sample(&self, x: f32, z: f32) -> Option<([f32; 3], f32)> {
        self.sample_row(x, &self.row(z))
    }

    fn sample_row(&self, x: f32, row: &PlumeRow) -> Option<([f32; 3], f32)> {
        let z = row.z;
        let mut cap = 1.0 - (x / self.width).powi(2) - row.cap_vertical;
        for (lobe, vertical) in self.lobes.iter().zip(row.lobe_vertical) {
            cap = cap.max(1.0 - ((x - lobe.x) / lobe.rx).powi(2) - vertical);
        }
        let stem = (1.0 - ((x - row.drift) / row.stem_width).powi(2))
            .min(row.stem_bottom)
            .min(row.stem_top);
        let stem = if self.t > 0.08 { stem } else { -10.0 };
        // A low rolling skirt connects the column to the ground flash.
        let skirt = 1.0
            - (x / (0.22 + self.growth * 0.38)).powi(2)
            - row.skirt_vertical
            - (smooth(0.3, 0.65, self.t) * 1.3);
        let shape = cap.max(stem).max(skirt);
        if shape < -0.25 {
            return None;
        }
        let broad = noise(x * 3.5 - self.t * 0.4, z * 3.5 - self.t * 2.8, self.seed);
        let fine = noise(
            x * 9.0 + self.t * 0.7,
            z * 9.0 - self.t * 5.0,
            self.seed ^ 0x9e3779b9,
        );
        let roil = broad * 0.75 + fine * 0.25;
        let density = shape + (roil - 0.5) * 0.38 - smooth(0.73, 1.0, self.t) * 0.5;
        if density <= 0.0 {
            return None;
        }
        let opacity = (density * 2.8).min(1.0) * (1.0 - smooth(0.72, 1.0, self.t));
        if opacity < 0.045 {
            return None;
        }
        let hot = 1.0 - smooth(0.12, 0.78, self.t);
        let underside = (1.0 - (z - self.lift + 0.4) / 0.9).clamp(0.0, 1.0);
        let ground = if skirt > cap && skirt > stem {
            0.72
        } else {
            1.0
        };
        let heat = (hot
            * ground
            * (0.35 + roil * 0.65 + density.min(1.0) * 0.22)
            * (0.65 + underside * 0.35))
            .min(1.0);
        let shade = 0.5 + roil * 0.45 + (-x / self.width).clamp(-1.0, 1.0) * 0.05;
        let smoke = [100.0 * shade, 96.0 * shade, 102.0 * shade];
        let fire = if heat > 0.8 {
            let t = (heat - 0.8) / 0.2;
            [255.0, 190.0 + t * 60.0, 55.0 + t * 165.0]
        } else if heat > 0.45 {
            let t = (heat - 0.45) / 0.35;
            [210.0 + t * 45.0, 65.0 + t * 125.0, 12.0 + t * 43.0]
        } else {
            let t = heat / 0.45;
            [110.0 + t * 100.0, 28.0 + t * 37.0, 14.0 - t * 2.0]
        };
        let burning = smooth(0.04, 0.4, heat);
        let flash = 1.0 - smooth(0.01, 0.09, self.t);
        let color = std::array::from_fn(|i| {
            let material = smoke[i] * (1.0 - burning) + fire[i] * burning;
            (material * (1.0 - flash) + [255.0, 253.0, 235.0][i] * flash) * opacity.sqrt()
        });
        Some((color, opacity))
    }
}

/// Sample all eight Braille dots per cell: rounded silhouettes retain sub-cell
/// detail, while dense interiors read as solid fire and shaded smoke.
pub fn render(
    exp: &ExplosionRender,
    x: u16,
    y: u16,
    area: Rect,
    _global_frame: u64,
    buf: &mut Buffer,
    globe: Option<&GlobeViewport>,
) {
    if exp.radius == 0 || exp.frame >= exp.weapon_type.max_frames() {
        return;
    }
    let clip = area.intersection(buf.area);
    if clip.is_empty() {
        return;
    }
    let plume = Plume::new(exp);
    let radius = exp.radius as f32;
    let reach = radius * (plume.width * 1.3 + 0.15);
    let left = ((x as f32 - reach).floor() as i32).max(clip.x as i32);
    let right = ((x as f32 + reach + 1.0).ceil() as i32).min(clip.right() as i32);
    let top = ((y as f32 - radius * (plume.lift + 1.05) / 2.0).floor() as i32).max(clip.y as i32);
    let bottom = ((y as f32 + radius * 0.2 + 1.0).ceil() as i32).min(clip.bottom() as i32);
    for py in top..bottom {
        let rows: [_; 4] = std::array::from_fn(|sy| {
            // Terminal cells are twice as tall as they are wide.
            let z = (y as f32 + 0.5 - py as f32 - (sy as f32 + 0.5) / 4.0) * 2.0 / radius;
            plume.row(z)
        });
        for px in left..right {
            let mut bits = 0u32;
            let mut sum = [0.0f32; 3];
            let mut coverage = 0.0;
            for sy in 0..4 {
                for sx in 0..2 {
                    let bx = px * 2 + sx;
                    let by = py * 4 + sy;
                    if globe.is_some_and(|g| {
                        g.pixel_to_sphere_point(bx - area.x as i32 * 2, by - area.y as i32 * 4)
                            .is_none()
                    }) {
                        continue;
                    }
                    let dx = (px as f32 + (sx as f32 + 0.5) / 2.0 - x as f32 - 0.5) / radius;
                    if let Some((rgb, opacity)) = plume.sample_row(dx, &rows[sy as usize]) {
                        bits |= [[1, 2, 4, 64], [8, 16, 32, 128]][sx as usize][sy as usize];
                        for i in 0..3 {
                            sum[i] += rgb[i];
                        }
                        coverage += opacity;
                    }
                }
            }
            let count = bits.count_ones() as f32;
            if count == 0.0 {
                continue;
            }
            let ch = if bits == 255 && coverage > 6.5 && plume.t < 0.48 {
                '█'
            } else if bits == 255 && coverage > 4.5 {
                '▓'
            } else if bits == 255 && coverage > 2.5 {
                '▒'
            } else {
                char::from_u32(0x2800 + bits).unwrap()
            };
            buf[(px as u16, py as u16)].set_char(ch).set_fg(Color::Rgb(
                (sum[0] / count) as u8,
                (sum[1] / count) as u8,
                (sum[2] / count) as u8,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::WeaponType;
    fn explosion(frame: u8, radius: u16) -> ExplosionRender {
        ExplosionRender {
            x: 40,
            y: 30,
            frame,
            radius,
            weapon_type: WeaponType::Nuke,
            lon: 12.0,
            lat: 30.0,
            radius_km: 500.0,
        }
    }

    #[test]
    fn mushroom_cap_is_wider_than_stem_and_cools_into_smoke() {
        let early = Plume::new(&explosion(5, 12));
        let width = (-100..=100)
            .filter(|i| early.sample(*i as f32 / 100.0, early.lift).is_some())
            .count();
        let height = (-100..=100)
            .filter(|i| early.sample(0.0, early.lift + *i as f32 / 100.0).is_some())
            .count();
        assert!(
            (0.75..1.3).contains(&(width as f32 / height as f32)),
            "initial fireball must be rounded"
        );
        let middle = Plume::new(&explosion(40, 12));
        let cap = (0..100)
            .filter(|i| middle.sample(*i as f32 / 50.0 - 1.0, middle.lift).is_some())
            .count();
        let stem = (0..100)
            .filter(|i| {
                middle
                    .sample(*i as f32 / 50.0 - 1.0, middle.lift * 0.4)
                    .is_some()
            })
            .count();
        assert!(cap > stem * 2, "cap {cap}, stem {stem}");
        let late = Plume::new(&explosion(72, 12));
        let (rgb, _) = late.sample(0.0, late.lift).unwrap();
        assert!(
            (rgb[0] - rgb[2]).abs() < 20.0,
            "smoke should be nearly neutral: {rgb:?}"
        );
    }

    #[test]
    fn rendering_is_age_driven_and_translation_invariant() {
        let exp = explosion(30, 8);
        let mut a = Buffer::empty(Rect::new(0, 0, 80, 45));
        let mut b = a.clone();
        render(&exp, 40, 30, a.area, 0, &mut a, None);
        render(&exp, 40, 30, b.area, 10000, &mut b, None);
        assert_eq!(a, b);
        b.reset();
        render(&exp, 43, 32, b.area, 1, &mut b, None);
        for y in 0..43 {
            for x in 0..77 {
                assert_eq!(a[(x, y)], b[(x + 3, y + 2)]);
            }
        }
    }

    #[test]
    fn globe_clipping_preserves_individual_edge_dots() {
        let area = Rect::new(5, 3, 50, 25);
        let globe = GlobeViewport::new(0.0, 0.0, 35.0, 100, 100);
        let mut buf = Buffer::empty(area);
        render(&explosion(30, 12), 45, 19, area, 0, &mut buf, Some(&globe));
        assert!(buf.content.iter().any(|c| c.symbol() != " "));
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                let ch = buf[(x, y)].symbol().chars().next().unwrap();
                if ch == ' ' {
                    continue;
                }
                let bits = if ('\u{2800}'..='\u{28ff}').contains(&ch) {
                    ch as u32 - 0x2800
                } else {
                    255
                };
                for sx in 0..2 {
                    for sy in 0..4 {
                        if bits & [[1, 2, 4, 64], [8, 16, 32, 128]][sx][sy] != 0 {
                            assert!(globe
                                .pixel_to_sphere_point(
                                    (x - area.x) as i32 * 2 + sx as i32,
                                    (y - area.y) as i32 * 4 + sy as i32
                                )
                                .is_some());
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn every_phase_clips_to_offset_and_tiny_buffers() {
        for radius in [1, 3, 12, u16::MAX] {
            for frame in 0..=90 {
                let exp = explosion(frame, radius);
                let mut buf = Buffer::empty(Rect::new(0, 0, 24, 16));
                let area = Rect::new(5, 4, 10, 8);
                render(&exp, 5, 4, area, 0, &mut buf, None);
                for y in 0..16 {
                    for x in 0..24 {
                        if !area.contains((x, y).into()) || frame == 90 {
                            assert_eq!(buf[(x, y)].symbol(), " ");
                        }
                    }
                }
            }
        }
        let mut empty = Buffer::empty(Rect::default());
        render(
            &explosion(20, 8),
            0,
            0,
            Rect::default(),
            0,
            &mut empty,
            None,
        );
    }
}
