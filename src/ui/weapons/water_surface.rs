//! A shared, geographic wave surface. Sum displacement before shading rather
//! than adding the light from independent pools. This is a damped analytic wave
//! model, not a terrain-aware fluid solver.
use crate::{
    app::WeaponType,
    interactions::Field,
    map::{globe::lonlat_to_vec3, Projection},
};
use glam::DVec3;
use ratatui::{buffer::Buffer, layout::Rect, style::Color};

const FIXED: f64 = 1_000_000.0;

struct Source {
    center: DVec3,
    radius: f64,
    front: f64,
    cutoff: f64,
    age: f64,
    fade: f64,
}

impl Source {
    fn new(field: &Field) -> Option<Self> {
        if field.weapon != WeaponType::Water
            || field.age == 0
            || field.age >= 180
            || field.radius_km <= 0.0
        {
            return None;
        }
        let front = field.front_km();
        let tail = ((180.0 - field.age as f64) / 70.0).clamp(0.0, 1.0);
        Some(Self {
            center: lonlat_to_vec3(field.lon, field.lat),
            radius: field.radius_km,
            front,
            cutoff: (front / 6371.0).cos(),
            age: field.age as f64,
            fade: tail * tail * (field.age as f64 / 5.0).min(1.0),
        })
    }

    fn sample(&self, point: DVec3) -> Option<Sample> {
        let dot = self.center.dot(point).clamp(-1.0, 1.0);
        if dot < self.cutoff {
            return None;
        }
        let distance = dot.acos() * 6371.0;
        let r = distance / self.radius;
        let behind = (self.front - distance) / self.radius;
        // A soft advancing edge; interiors combine by union, not added opacity.
        let coverage = (behind / 0.14).clamp(0.0, 1.0);
        let envelope = self.fade * coverage / (1.0 + r * 0.65);
        // Two wavelengths travel at different speeds: long swells with finer
        // capillary ripples. All impacts share phase at birth.
        let phase = r * 17.0 - self.age * 0.25;
        let fine = r * 29.0 - self.age * 0.38;
        let height = (phase.sin() + 0.24 * fine.sin()) * envelope;
        let slope = (phase.cos() + 0.41 * fine.cos()) * envelope;
        let direction = (point * dot - self.center).normalize_or_zero();
        let rim = (-((behind - 0.055) / 0.045).powi(2)).exp() * self.fade;
        Some(Sample {
            height,
            gradient: direction * slope,
            energy: envelope,
            coverage,
            fade: self.fade,
            rim,
        })
    }
}

struct Sample {
    height: f64,
    gradient: DVec3,
    energy: f64,
    coverage: f64,
    fade: f64,
    rim: f64,
}

#[derive(Default)]
struct Surface {
    height: i64,
    gradient: [i64; 3],
    energy: i64,
    slopes: i64,
    coverage: f64,
    fade: f64,
    rim: f64,
    contacts: u32,
}

impl Surface {
    fn add(&mut self, s: Sample) {
        self.height += (s.height * FIXED).round() as i64;
        for (sum, value) in self.gradient.iter_mut().zip(s.gradient.to_array()) {
            *sum += (value * FIXED).round() as i64;
        }
        self.energy += (s.energy * FIXED).round() as i64;
        self.slopes += (s.gradient.length() * FIXED).round() as i64;
        self.coverage = self.coverage.max(s.coverage);
        self.fade = self.fade.max(s.fade);
        self.rim = self.rim.max(s.rim);
        self.contacts += 1;
    }

    fn color(&self, point: DVec3, time: f64) -> Option<[u8; 3]> {
        if self.contacts == 0 {
            return None;
        }
        // Bound exposure without erasing constructive/destructive interference.
        let norm = (self.energy as f64 / FIXED).max(1.0).sqrt();
        let height = self.height as f64 / FIXED / norm;
        let gradient = DVec3::from_array(self.gradient.map(|v| v as f64 / FIXED / norm));
        let slope = gradient.length();
        let compression = ((self.slopes as f64 / FIXED / norm - slope) * 0.8).max(0.0);
        let crest = ((height - 0.12) / 0.9).clamp(0.0, 1.0);
        // Use world coordinates for foam breakup, so camera movement never
        // reseeds the pattern. Advection is continuous rather than frame noise.
        let texture = (point.x * 311.0 + point.y * 193.0 - time * 0.028).sin()
            * (point.z * 271.0 - point.y * 137.0 + time * 0.019).sin();
        let breaking = (compression * crest + (slope - 0.72).max(0.0) * crest).min(1.0);
        // An edge submerged inside another pool is no longer a shoreline.
        let outer_rim = self.rim * (1.0 - self.coverage).sqrt();
        let foam = (breaking * (0.65 + texture * 0.35) + outer_rim * (0.55 + texture * 0.45))
            .clamp(0.0, 1.0);
        let glint = (gradient.dot(DVec3::new(0.35, -0.45, 0.8)) * 0.5 + crest * 0.45).max(0.0);
        let light = (0.18 * self.fade + glint * 0.85).min(1.0);
        // Negative displacement makes dark troughs, not another bright band.
        if foam < 0.08 && (height < -0.1 || light < 0.11) {
            return None;
        }
        Some([
            (20.0 * light * (1.0 - foam) + 205.0 * foam).min(255.0) as u8,
            (145.0 * light * (1.0 - foam) + 240.0 * foam).min(255.0) as u8,
            (235.0 * light * (1.0 - foam) + 255.0 * foam).min(255.0) as u8,
        ])
    }
}

pub(super) fn render(
    fields: &[Field],
    projection: &Projection,
    area: Rect,
    frame: u64,
    buf: &mut Buffer,
) {
    let sources: Vec<_> = fields.iter().filter_map(Source::new).collect();
    if sources.is_empty() {
        return;
    }
    let clip = area.intersection(buf.area);
    for y in clip.y..clip.bottom() {
        for x in clip.x..clip.right() {
            let mut bits = 0;
            let mut color = [0u8; 3];
            for sy in 0..4 {
                for sx in 0..2 {
                    let Some((lon, lat)) = projection
                        .unproject((x - area.x) as i32 * 2 + sx, (y - area.y) as i32 * 4 + sy)
                    else {
                        continue;
                    };
                    let point = lonlat_to_vec3(lon, lat);
                    let mut surface = Surface::default();
                    for source in &sources {
                        if let Some(sample) = source.sample(point) {
                            surface.add(sample);
                        }
                    }
                    if let Some(rgb) = surface.color(point, frame as f64) {
                        bits |= [[1, 2, 4, 64], [8, 16, 32, 128]][sx as usize][sy as usize];
                        for i in 0..3 {
                            color[i] = color[i].max(rgb[i]);
                        }
                    }
                }
            }
            if bits != 0 {
                buf[(x, y)]
                    .set_char(char::from_u32(0x2800 + bits).unwrap())
                    .set_fg(Color::Rgb(color[0], color[1], color[2]));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::Explosion,
        map::{GlobeViewport, Viewport},
    };

    fn field(lon: f64, age: u8) -> Field {
        Field::new(&Explosion {
            lon,
            lat: 0.0,
            radius_km: 600.0,
            frame: age,
            weapon_type: WeaponType::Water,
        })
    }

    #[test]
    fn signed_waves_cancel_before_shading_and_opposing_crests_compress() {
        let sample = |height, gradient| Sample {
            height,
            gradient,
            energy: 1.0,
            coverage: 1.0,
            fade: 1.0,
            rim: 0.0,
        };
        let mut cancelled = Surface::default();
        cancelled.add(sample(0.8, DVec3::X));
        cancelled.add(sample(-0.8, -DVec3::X));
        assert_eq!(cancelled.height, 0);
        assert_eq!(cancelled.gradient, [0; 3]);
        let mut crests = Surface::default();
        crests.add(sample(0.8, DVec3::X));
        crests.add(sample(0.8, -DVec3::X));
        assert!(
            crests.color(DVec3::Z, 0.0).unwrap()[0]
                > cancelled.color(DVec3::Z, 0.0).unwrap()[0] + 50
        );
    }

    #[test]
    fn pooled_water_is_order_independent_clipped_and_expires() {
        let area = Rect::new(3, 2, 80, 35);
        let mut fields = vec![field(-3.0, 32), field(3.0, 47), field(0.0, 20)];
        for projection in [
            Projection::Mercator(Viewport::new(0.0, 0.0, 8.0, 160, 140)),
            Projection::Globe(GlobeViewport::new(0.0, 0.0, 60.0, 160, 140)),
        ] {
            let mut a = Buffer::empty(Rect::new(0, 0, 88, 40));
            render(&fields, &projection, area, 47, &mut a);
            fields.reverse();
            let mut b = Buffer::empty(a.area);
            render(&fields, &projection, area, 47, &mut b);
            assert_eq!(a, b);
            assert!(a.content.iter().any(|c| c.symbol() != " "));
            for y in 0..40 {
                for x in 0..88 {
                    if !area.contains((x, y).into()) {
                        assert_eq!(a[(x, y)].symbol(), " ");
                    }
                }
            }
            let mut ended = fields.clone();
            for f in &mut ended {
                f.age = 180;
            }
            let mut empty = Buffer::empty(area);
            render(&ended, &projection, area, 180, &mut empty);
            assert!(empty.content.iter().all(|c| c.symbol() == " "));
        }
    }

    #[test]
    fn overlap_has_one_outer_edge_and_crosses_the_date_line() {
        let source = Source::new(&field(179.0, 35)).unwrap();
        assert!(source.sample(lonlat_to_vec3(-179.0, 0.0)).is_some());
        assert!(source.sample(lonlat_to_vec3(0.0, 0.0)).is_none());
        let mut surface = Surface::default();
        surface.add(Sample {
            height: 0.0,
            gradient: DVec3::ZERO,
            energy: 0.0,
            coverage: 0.1,
            fade: 1.0,
            rim: 1.0,
        });
        let exposed = surface.color(DVec3::Z, 0.0).unwrap();
        surface.add(Sample {
            height: 0.0,
            gradient: DVec3::ZERO,
            energy: 0.0,
            coverage: 1.0,
            fade: 1.0,
            rim: 0.0,
        });
        let submerged = surface.color(DVec3::Z, 0.0).unwrap();
        assert!(exposed[0] > submerged[0] + 50);
    }

    #[test]
    fn compositor_keeps_water_when_its_surface_anchor_is_hidden() {
        use crate::{interactions::Interactions, ui::weapons::composite::Compositor};
        let area = Rect::new(0, 0, 100, 45);
        let projection = Projection::Globe(GlobeViewport::new(0.0, 0.0, 80.0, 200, 180));
        let mut world = Interactions::default();
        let mut water = field(93.0, 35);
        water.radius_km = 1500.0;
        world.fields.push(water);
        assert!(projection.project_point(93.0, 0.0).is_none());
        let mut buf = Buffer::empty(area);
        Compositor::default().render(&[], &world, &projection, area, 35, &mut buf);
        assert!(buf.content.iter().any(|cell| cell.symbol() != " "));
    }
}
