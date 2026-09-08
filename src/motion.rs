//! Deterministic motion shared by simulation and rendering.
use crate::{hash::rand_simple, map::globe::lonlat_to_vec3};
use glam::DVec3;

pub fn variation(seed: u64, channel: u64) -> f64 {
    rand_simple(seed.wrapping_add(channel.wrapping_mul(7919)))
}

pub fn meteor_impact_frame(seed: u64) -> u8 {
    30 + (variation(seed, 1) * 18.0) as u8
}

pub fn tornado_position(lon: f64, lat: f64, radius: f64, seed: u64, age: u16) -> (f64, f64) {
    let t = age as f64 / 210.0;
    let heading = variation(seed, 2) * std::f64::consts::TAU;
    let phase = variation(seed, 3) * std::f64::consts::TAU;
    let forward = t * (1.4 + variation(seed, 4) * 1.5);
    let side = ((t * 7.0 + phase).sin() - phase.sin()) * 0.45
        + ((t * 13.0 + phase).cos() - phase.cos()) * 0.12;
    let east = heading.cos() * forward - heading.sin() * side;
    let north = heading.sin() * forward + heading.cos() * side;
    let center = lonlat_to_vec3(lon, lat);
    let e = DVec3::new(-lon.to_radians().sin(), lon.to_radians().cos(), 0.0);
    let p = (center + (e * east + center.cross(e) * north) * (radius / 6371.0)).normalize();
    (
        p.y.atan2(p.x).to_degrees(),
        p.z.clamp(-1.0, 1.0).asin().to_degrees(),
    )
}

pub struct WaterFlow {
    pub along: f64,
    pub across: f64,
    pub coverage: f64,
    pub edge: f64,
    pub forward: DVec3,
}

/// A lobed sheet surges in from one side, fans out, then rocks back and forth.
/// Rendering and extinguishing share this footprint, including at the poles.
pub fn water_flow(
    lon: f64,
    lat: f64,
    radius: f64,
    seed: u64,
    age: u16,
    point: DVec3,
) -> Option<WaterFlow> {
    WaterSheet::new(lon, lat, radius, seed, age)?.sample(point)
}

pub struct WaterSheet {
    center: DVec3,
    forward: DVec3,
    sideways: DVec3,
    scale: f64,
    phase: f64,
    surge: f64,
    slosh: f64,
    age: f64,
    cutoff: f64,
}

impl WaterSheet {
    pub fn new(lon: f64, lat: f64, radius: f64, seed: u64, age: u16) -> Option<Self> {
        if age == 0 || radius <= 0.0 {
            return None;
        }
        let center = lonlat_to_vec3(lon, lat);
        let east = DVec3::new(-lon.to_radians().sin(), lon.to_radians().cos(), 0.0);
        let north = center.cross(east);
        let heading = variation(seed, 30) * std::f64::consts::TAU;
        let t = age as f64;
        Some(Self {
            center,
            forward: east * heading.cos() + north * heading.sin(),
            sideways: north * heading.cos() - east * heading.sin(),
            scale: 6371.0 / radius,
            phase: variation(seed, 31) * std::f64::consts::TAU,
            surge: 1.0 - (-t / 20.0).exp(),
            slosh: (t / 14.0).sin() * 0.22 * (t / 30.0).min(1.0) * (-t / 180.0).exp(),
            age: t,
            cutoff: (radius * 2.6 / 6371.0)
                .min(std::f64::consts::FRAC_PI_2)
                .cos(),
        })
    }

    pub fn sample(&self, point: DVec3) -> Option<WaterFlow> {
        let dot = self.center.dot(point);
        if dot <= self.cutoff {
            return None;
        }
        let along = point.dot(self.forward) * self.scale / dot;
        let across = point.dot(self.sideways) * self.scale / dot;
        let phase = self.phase;
        let head = -1.1
            + 2.7 * self.surge
            + self.slosh
            + 0.22 * (across * 3.3 + phase).sin()
            + 0.10 * (across * 7.1 - self.age * 0.025).sin();
        let rear = -1.85 + 0.12 * (across * 4.0 + phase).sin() + self.slosh * 0.4;
        let width = (0.18 + self.surge * 0.88) * (0.82 + 0.15 * (along * 2.7 + phase).sin());
        let edge = head - along;
        let side = width - (across + 0.15 * (along * 2.0 + phase).sin()).abs();
        let back = along - rear;
        if edge < 0.0 || side < 0.0 || back < 0.0 {
            return None;
        }
        Some(WaterFlow {
            along,
            across,
            edge,
            forward: self.forward,
            coverage: (edge / 0.14)
                .min(side / 0.18)
                .min(back / 0.3)
                .clamp(0.0, 1.0),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{Explosion, WeaponType},
        interactions::{destination, Interactions},
    };

    #[test]
    fn roaming_field_matches_render_anchor_across_date_line_and_poles() {
        for lat in [0.0, 89.0, -89.0] {
            let mut world = Interactions::default();
            world.launch(&Explosion {
                seed: 42,
                lon: 179.0,
                lat,
                frame: 0,
                radius_km: 600.0,
                weapon_type: WeaponType::Tornado,
            });
            for age in 1..200 {
                world.update(&mut Vec::new());
                let expected = tornado_position(179.0, lat, 600.0, 42, age);
                assert_eq!((world.fields[0].lon, world.fields[0].lat), expected);
                assert!((-180.0..=180.0).contains(&expected.0));
                assert!((-90.0..=90.0).contains(&expected.1));
            }
            assert_ne!(
                tornado_position(179.0, lat, 600.0, 42, 100),
                tornado_position(179.0, lat, 600.0, 43, 100)
            );
        }
    }

    #[test]
    fn water_surges_from_one_side_then_fans_out_instead_of_forming_a_disk() {
        let seed = 42;
        let heading = variation(seed, 30) * std::f64::consts::TAU;
        let at = |distance, bearing| {
            let (lon, lat) = destination(179.0, 0.0, distance, bearing);
            lonlat_to_vec3(lon, lat)
        };
        // Geographic bearing is measured from north, the sheet heading from east.
        let bearing = std::f64::consts::FRAC_PI_2 - heading;
        let front = at(600.0, bearing);
        let side = at(600.0, bearing + std::f64::consts::FRAC_PI_2);
        assert!(water_flow(179.0, 0.0, 600.0, seed, 1, front).is_none());
        assert!(water_flow(179.0, 0.0, 600.0, seed, 65, front).is_some());
        assert!(water_flow(179.0, 0.0, 600.0, seed, 65, side).is_none());
        assert!(water_flow(179.0, 0.0, 600.0, seed, 0, front).is_none());
    }
}
