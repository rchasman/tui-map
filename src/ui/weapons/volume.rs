//! Raised effects live in an east/north/up frame anchored to the planet.
//! Every sample rotates with Earth and is occluded by the solid sphere.
use super::{organic::noise, ExplosionRender};
use crate::{
    app::WeaponType,
    hash::rand_simple,
    map::{globe::lonlat_to_vec3, GlobeViewport},
};
use glam::DVec3;
use ratatui::{buffer::Buffer, layout::Rect, style::Color};

pub fn is_raised(weapon: WeaponType) -> bool {
    matches!(
        weapon,
        WeaponType::Nuke | WeaponType::Bio | WeaponType::Chem | WeaponType::Life
            | WeaponType::Tornado | WeaponType::Frost | WeaponType::Meteor
    )
}

struct Volume<'a> {
    center: DVec3,
    east: DVec3,
    north: DVec3,
    scale: f64,
    globe: &'a GlobeViewport,
    area: Rect,
}

impl Volume<'_> {
    fn dot(&self, x: f32, y: f32, z: f32, color: [f32; 3], size: i32, buf: &mut Buffer) {
        let surface = (self.center
            + self.east * (x as f64 * self.scale)
            + self.north * (y as f64 * self.scale))
            .normalize();
        let point = surface * (1.0 + z.max(0.0) as f64 * self.scale);
        let Some((px, py)) = self.globe.project_elevated(point) else {
            return;
        };
        for dy in -size..=size {
            for dx in -size..=size {
                let (sx, sy) = (px + dx, py + dy);
                if sx < 0
                    || sy < 0
                    || sx >= self.area.width as i32 * 2
                    || sy >= self.area.height as i32 * 4
                {
                    continue;
                }
                if !self.globe.elevated_sample_visible(point, sx, sy) {
                    continue;
                }
                let cell = &mut buf[(self.area.x + (sx / 2) as u16, self.area.y + (sy / 4) as u16)];
                let old = cell.symbol().chars().next().unwrap_or(' ') as u32;
                let bits = if (0x2800..=0x28ff).contains(&old) {
                    old - 0x2800
                } else {
                    0
                };
                let bit = [[1, 2, 4, 64], [8, 16, 32, 128]][(sx % 2) as usize][(sy % 4) as usize];
                let mut rgb = color.map(|c| c.clamp(0.0, 255.0) as u8);
                if bits != 0 {
                    if let Color::Rgb(r, g, b) = cell.fg {
                        rgb = [rgb[0].max(r), rgb[1].max(g), rgb[2].max(b)];
                    }
                }
                cell.set_char(char::from_u32(0x2800 + (bits | bit)).unwrap())
                    .set_fg(Color::Rgb(rgb[0], rgb[1], rgb[2]));
            }
        }
    }
}

pub fn render(exp: &ExplosionRender, globe: &GlobeViewport, area: Rect, buf: &mut Buffer) {
    if !is_raised(exp.weapon_type)
        || exp.frame >= exp.weapon_type.max_frames()
        || exp.radius_km <= 0.0
    {
        return;
    }
    let area = area.intersection(buf.area);
    if area.is_empty() {
        return;
    }
    let center = lonlat_to_vec3(exp.lon, exp.lat);
    let east = DVec3::new(-exp.lon.to_radians().sin(), exp.lon.to_radians().cos(), 0.0);
    let volume = Volume {
        center,
        east,
        north: center.cross(east),
        scale: exp.radius_km / 6371.0,
        globe,
        area,
    };
    if !globe.volume_may_be_visible(center, volume.scale * 6.5) {
        return;
    }
    if matches!(exp.weapon_type, WeaponType::Tornado | WeaponType::Frost | WeaponType::Meteor) {
        super::weather::samples(exp, |x, y, z, color| volume.dot(x, y, z, color, 0, buf));
        return;
    }
    let t = exp.frame as f32 / exp.weapon_type.max_frames() as f32;
    let seed = exp.lon.to_bits() ^ exp.lat.to_bits().rotate_left(17);
    let random = |i| rand_simple(seed.wrapping_add(i)) as f32;
    let pixels = volume.scale * globe.radius;
    if exp.weapon_type == WeaponType::Life {
        let fade = 1.0 - ((t - 0.76) / 0.24).clamp(0.0, 1.0);
        for plant in 0..9u64 {
            let angle = random(plant * 5) * std::f32::consts::TAU;
            let spread = random(plant * 5 + 1).sqrt();
            let (x, y) = (angle.cos() * spread, angle.sin() * spread);
            let growth = ((t - random(plant * 5 + 2) * 0.1) / 0.25).clamp(0.0, 1.0);
            let height = (0.5 + random(plant * 5 + 3) * 1.8) * (1.0 - (1.0 - growth).powi(2));
            let bend = (random(plant * 5 + 4) - 0.5) * 0.5;
            for step in 0..48 {
                let age = step as f32 / 47.0;
                let z = height * age;
                volume.dot(
                    x + bend * age,
                    y + (age * 3.0 + t * 2.0 + angle).sin() * 0.06 * age,
                    z,
                    [45.0 * fade, 175.0 * fade, 65.0 * fade],
                    0,
                    buf,
                );
            }
            for branch in 0..3 {
                let z = height * (0.4 + branch as f32 * 0.25);
                let turn = angle + branch as f32 * 2.4;
                for step in 0..24 {
                    let age = step as f32 / 23.0;
                    let reach = 0.25 * growth * age;
                    let leaf = (age * std::f32::consts::PI).sin() * 0.07;
                    for side in [-1.0, 0.0, 1.0] {
                        volume.dot(
                            x + bend * z / height.max(0.001) + turn.cos() * reach
                                - turn.sin() * leaf * side,
                            y + turn.sin() * reach + turn.cos() * leaf * side,
                            z + reach * 0.35,
                            [70.0 * fade, 210.0 * fade, 80.0 * fade],
                            0,
                            buf,
                        );
                    }
                }
            }
            if t > 0.2 {
                volume.dot(
                    x + bend,
                    y,
                    height,
                    [235.0 * fade, 210.0 * fade, 100.0 * fade],
                    0,
                    buf,
                );
            }
        }
        return;
    }
    // Particle tails use the same world-space frame and horizon depth test.
    for particle in 0..20u64 {
        let angle = random(particle * 3 + 100) * std::f32::consts::TAU;
        let speed = 0.5 + random(particle * 3 + 101);
        for tail in 0..4 {
            let age = t - tail as f32 * 0.012 - random(particle * 3 + 102) * 0.12;
            if age <= 0.0 {
                continue;
            }
            let distance = age * speed * 1.7;
            let (height, color) = match exp.weapon_type {
                WeaponType::Nuke => (
                    (age * speed * 4.0 - age * age * 2.2).max(0.0),
                    [255.0, 160.0, 65.0],
                ),
                WeaponType::Bio => (0.08 + age * 0.6, [110.0, 225.0, 125.0]),
                _ => (
                    (0.7 * age - age * age * 0.45).max(0.0),
                    [190.0, 90.0, 225.0],
                ),
            };
            let light = (1.0 - t) * (1.0 - tail as f32 * 0.2) * 0.65;
            volume.dot(
                angle.cos() * distance,
                angle.sin() * distance,
                height,
                color.map(|c| c * light),
                0,
                buf,
            );
        }
    }
    let n = (pixels * 2.0).ceil().clamp(16.0, 36.0) as usize;
    let step = 3.6 / n as f32;
    let splat = ((pixels * step as f64 * 0.45).floor() as i32).clamp(0, 2);
    let fade = 1.0 - ((t - 0.70) / 0.30).clamp(0.0, 1.0);
    let growth = 1.0 - (1.0 - (t / 0.35).min(1.0)).powi(3);
    let lift = 0.10 + 2.5 * (1.0 - (1.0 - t).powi(2));
    for iz in 0..n {
        let z = (iz as f32 + 0.5) * step;
        for iy in 0..n {
            for ix in 0..n {
                let x = (ix as f32 + 0.5) * step - 1.8;
                let y = (iy as f32 + 0.5) * step - 1.8;
                let material = noise(x * 3.5 - t, y * 3.5 + z * 2.5 - t * 2.0, seed);
                let r2 = x * x + y * y;
                let (shape, color) = if exp.weapon_type == WeaponType::Nuke {
                    let width = 0.24 + growth * 1.04 + t * 0.22;
                    let mut cap =
                        1.0 - r2 / (width * width) - ((z - lift) / (0.22 + growth * 0.40)).powi(2);
                    for lobe in 0..5u64 {
                        let a = lobe as f32 * 1.256 + random(lobe + 20) * 0.5;
                        let ring = width * 0.55;
                        let q = ((x - a.cos() * ring) / (width * 0.52)).powi(2)
                            + ((y - a.sin() * ring) / (width * 0.52)).powi(2)
                            + ((z - lift - (random(lobe + 30) - 0.4) * 0.2)
                                / (0.25 + growth * 0.32))
                                .powi(2);
                        cap = cap.max(1.0 - q);
                    }
                    let stem = (1.0 - r2 / (0.14 + growth * 0.10).powi(2)).min((lift - z) * 5.0);
                    let heat = (1.0 - t) * 0.8 + material * 0.25;
                    let color = if t > 0.65 {
                        [105.0, 100.0, 105.0]
                    } else {
                        [245.0, 65.0 + heat * 150.0, 20.0 + heat * 45.0]
                    };
                    (cap.max(stem) + (material - 0.5) * 0.45, color)
                } else {
                    let chemical = exp.weapon_type == WeaponType::Chem;
                    let width = 0.15 + growth * 1.4;
                    let height = if chemical { 0.85 } else { 0.35 };
                    let lean = (random(77) - 0.5) * t * 0.5;
                    let shape = 1.0
                        - ((x - lean) / width).powi(2)
                        - (y / (width * 0.8)).powi(2)
                        - ((z - height * 0.35) / (height * growth.max(0.05))).powi(2)
                        + (material - 0.5) * 0.85;
                    (
                        shape,
                        if chemical {
                            [165.0, 45.0, 220.0]
                        } else {
                            [55.0, 205.0, 85.0]
                        },
                    )
                };
                let alpha = shape.clamp(0.0, 1.0) * fade;
                if alpha < 0.10 {
                    continue;
                }
                let light = (0.5 + material * 0.5) * alpha.sqrt();
                volume.dot(x, y, z, color.map(|c| c * light), splat, buf);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ui_keeps_effects_whose_surface_anchor_is_hidden() {
        use crate::{
            app::{App, Explosion},
            map::Projection,
            ui,
        };
        use ratatui::{backend::TestBackend, Terminal};
        let mut app = App::new(100, 55);
        app.projection = Projection::Globe(GlobeViewport::new(0.0, 0.0, 65.0, 196, 208));
        let mut terminal = Terminal::new(TestBackend::new(100, 55)).unwrap();
        terminal.draw(|frame| ui::render(frame, &mut app)).unwrap();
        let before = terminal.backend().buffer().clone();
        app.explosions.push(Explosion {
            lon: 100.0,
            lat: 0.0,
            frame: 90,
            radius_km: 1000.0,
            weapon_type: WeaponType::Life,
        });
        terminal.draw(|frame| ui::render(frame, &mut app)).unwrap();
        let after = terminal.backend().buffer();
        assert_ne!(
            &before, after,
            "UI culling must keep the raised part of a hidden anchor"
        );
    }

    #[test]
    fn hidden_anchors_keep_visible_tops_and_far_side_stays_hidden() {
        let area = Rect::new(4, 3, 100, 50);
        let globe = GlobeViewport::new(0.0, 0.0, 65.0, 200, 200);
        for weapon_type in [
            WeaponType::Nuke,
            WeaponType::Bio,
            WeaponType::Chem,
            WeaponType::Life,
            WeaponType::Tornado,
            WeaponType::Frost,
            WeaponType::Meteor,
        ] {
            let mut exp = ExplosionRender {
                x: 0,
                y: 0,
                frame: 30,
                radius: 12,
                weapon_type,
                lon: 92.0,
                lat: 0.0,
                radius_km: 1000.0,
            };
            if weapon_type == WeaponType::Life {
                exp.frame = 90;
            }
            assert!(globe.project(exp.lon, exp.lat).is_none());
            let mut buf = Buffer::empty(Rect::new(0, 0, 110, 60));
            render(&exp, &globe, area, &mut buf);
            assert!(buf.content.iter().any(|c| c.symbol() != " "));
            for y in 0..60 {
                for x in 0..110 {
                    if !area.contains((x, y).into()) {
                        assert_eq!(buf[(x, y)].symbol(), " ");
                    }
                }
            }
            exp.lon = 180.0;
            let mut hidden = Buffer::empty(area);
            render(&exp, &globe, area, &mut hidden);
            assert!(hidden.content.iter().all(|c| c.symbol() == " "));
        }
    }
}
