//! Geographic effect fields and short-lived reaction residue. No viewport state.
use crate::{
    app::{Explosion, Fire, WeaponType},
    map::globe::lonlat_to_vec3,
};
use glam::DVec3;
use std::collections::BTreeMap;

const EARTH_KM: f64 = 6371.0;
const MAX_FIELDS: usize = 48;
const MAX_REACTIONS: usize = 1024;

#[derive(Clone)]
pub struct Field {
    pub lon: f64,
    pub lat: f64,
    pub radius_km: f64,
    pub weapon: WeaponType,
    pub age: u16,
    center: DVec3,
}

impl Field {
    pub fn new(exp: &Explosion) -> Self {
        Self {
            lon: exp.lon,
            lat: exp.lat,
            radius_km: exp.radius_km,
            weapon: exp.weapon_type,
            age: exp.frame as u16,
            center: lonlat_to_vec3(exp.lon, exp.lat),
        }
    }

    pub fn progress(&self) -> f64 {
        self.age as f64 / self.weapon.max_frames() as f64
    }

    pub fn front_km(&self) -> f64 {
        self.radius_km * 1.9 * (1.0 - (1.0 - self.progress().min(1.0)).powi(3))
    }

    pub fn contains(&self, point: DVec3) -> bool {
        self.age > 0 && self.center.dot(point) >= (self.front_km() / EARTH_KM).cos()
    }

    pub fn distance(&self, point: DVec3) -> f64 {
        self.center.dot(point).clamp(-1.0, 1.0).acos() * EARTH_KM
    }

    fn lifetime(&self) -> u16 {
        match self.weapon {
            WeaponType::Water => 150,
            WeaponType::Life => 180,
            _ => self.weapon.max_frames() as u16 + 24,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReactionKind {
    Steam,
    Bloom,
    Scorch,
}

#[derive(Clone)]
pub struct Reaction {
    pub lon: f64,
    pub lat: f64,
    pub radius_km: f64,
    pub age: u16,
    pub strength: u8,
    pub kind: ReactionKind,
}

impl Reaction {
    pub fn lifetime(&self) -> u16 {
        match self.kind {
            ReactionKind::Steam => 90,
            ReactionKind::Bloom => 110,
            ReactionKind::Scorch => 50,
        }
    }
}

#[derive(Default)]
pub struct Interactions {
    pub fields: Vec<Field>,
    pub reactions: BTreeMap<(ReactionKind, i32, i32), Reaction>,
}

impl Interactions {
    pub fn launch(&mut self, exp: &Explosion) {
        if self.fields.len() == MAX_FIELDS {
            self.fields.remove(0);
        }
        self.fields.push(Field::new(exp));
    }

    pub fn active(&self) -> bool {
        !self.fields.is_empty() || !self.reactions.is_empty()
    }

    pub fn update(&mut self, fires: &mut Vec<Fire>) -> bool {
        for field in &mut self.fields {
            field.age += 1;
        }
        self.fields.retain(|f| f.age < f.lifetime());
        self.reactions.retain(|_, r| {
            r.age += 1;
            r.age < r.lifetime()
        });
        let mut changed = false;
        let reaches: Vec<_> = self
            .fields
            .iter()
            .filter(|f| f.weapon.is_restorative())
            .map(|f| (f, (f.front_km() / EARTH_KM).cos()))
            .collect();
        if reaches.is_empty() {
            return false;
        }
        for fire in fires.iter_mut() {
            let point = lonlat_to_vec3(fire.lon, fire.lat);
            // Choose reactions from a snapshot: water always takes precedence over
            // growth, regardless of field insertion order.
            let wet = reaches
                .iter()
                .filter(|(f, reach)| f.weapon == WeaponType::Water && f.center.dot(point) >= *reach)
                .max_by(|a, b| a.0.radius_km.total_cmp(&b.0.radius_km));
            let life = reaches
                .iter()
                .any(|(f, reach)| f.weapon == WeaponType::Life && f.center.dot(point) >= *reach);
            if let Some((field, _)) = wet {
                emit(
                    &mut self.reactions,
                    fire.lon,
                    fire.lat,
                    field.radius_km / 12.0,
                    ReactionKind::Steam,
                    fire.intensity,
                );
                fire.intensity = 0;
                changed = true;
            } else if life {
                // Weak embers yield to growth; hot fire chars the new shoots.
                let kind = if fire.intensity < 100 {
                    ReactionKind::Bloom
                } else {
                    ReactionKind::Scorch
                };
                emit(
                    &mut self.reactions,
                    fire.lon,
                    fire.lat,
                    25.0,
                    kind,
                    fire.intensity.max(100),
                );
                fire.intensity = fire
                    .intensity
                    .saturating_sub(if kind == ReactionKind::Bloom { 12 } else { 2 });
                changed = true;
            }
        }
        fires.retain(|f| f.intensity > 0);

        // Sample the grown area in geographic space. Only overlapping wet/growth
        // footprints bloom, including water that arrived before the life pulse.
        // No water means no wet-growth contacts to sample. Fire reactions above
        // still run, and existing residue has already advanced its age.
        if !reaches.iter().any(|(f, _)| f.weapon == WeaponType::Water) {
            return changed;
        }
        for (life, reach) in reaches.iter().filter(|(f, _)| f.weapon == WeaponType::Life) {
            for ring in 1..=6 {
                for spoke in 0..24 {
                    let (lon, lat) = destination(
                        life.lon,
                        life.lat,
                        life.radius_km * 1.9 * ring as f64 / 6.0,
                        spoke as f64 * std::f64::consts::TAU / 24.0,
                    );
                    let point = lonlat_to_vec3(lon, lat);
                    if life.center.dot(point) >= *reach
                        && reaches.iter().any(|(f, reach)| {
                            f.weapon == WeaponType::Water && f.center.dot(point) >= *reach
                        })
                    {
                        emit(
                            &mut self.reactions,
                            lon,
                            lat,
                            life.radius_km / 14.0,
                            ReactionKind::Bloom,
                            200,
                        );
                    }
                }
            }
        }
        changed
    }

    /// Temporary cloud displacement and ionization at this world point.
    /// Fixed-point pressure and charge sums are independent of field order.
    pub fn cloud_response(&self, point: DVec3) -> (f32, f32) {
        let mut pressure = 0i64;
        let mut charge = 0i64;
        for field in &self.fields {
            let t = field.progress();
            if t <= 0.0 || t >= 1.4 {
                continue;
            }
            let front = field.front_km();
            let width = (field.radius_km * 0.22).max(1.0);
            if field.center.dot(point) < ((front + width * 3.0) / EARTH_KM).cos() {
                continue;
            }
            let d = field.distance(point);
            let recovery = if t > 0.8 {
                ((1.4 - t) / 0.6).max(0.0)
            } else {
                1.0
            };
            let rim = (1.0 - ((d - front) / width).abs()).max(0.0);
            let hollow = if d < front { (1.0 - rim) * 0.72 } else { 0.0 };
            pressure += (recovery * (rim * 1.8 - hollow) * 65536.0) as i64;
            if field.weapon == WeaponType::Emp {
                // Charge lingers behind the leading edge, then decays smoothly.
                let contact = (1.0 - ((d - front) / (width * 2.5)).abs()).max(0.0);
                charge += (contact * recovery * 65536.0) as i64;
            }
        }
        (
            (1.0 + pressure as f32 / 65536.0).clamp(0.08, 3.0),
            (charge as f32 / 65536.0).min(1.0),
        )
    }
}

fn emit(
    reactions: &mut BTreeMap<(ReactionKind, i32, i32), Reaction>,
    lon: f64,
    lat: f64,
    radius_km: f64,
    kind: ReactionKind,
    strength: u8,
) {
    // Merge dense fire samples into small geographic patches, limiting visual work.
    let cell_deg = (radius_km / 111.0).max(0.03);
    let lon = (lon + 180.0).rem_euclid(360.0) - 180.0;
    let qlon = (lon / cell_deg).round() * cell_deg;
    let qlat = (lat / cell_deg).round() * cell_deg;
    let key = (kind, (qlon * 1000.0) as i32, (qlat * 1000.0) as i32);
    if let Some(existing) = reactions.get_mut(&key) {
        existing.strength = existing.strength.max(strength);
    } else if reactions.len() < MAX_REACTIONS {
        reactions.insert(
            key,
            Reaction {
                lon: qlon,
                lat: qlat.clamp(-90.0, 90.0),
                radius_km: radius_km.max(3.0),
                age: 0,
                strength,
                kind,
            },
        );
    }
}

/// Great-circle destination. Rings and reaction particles share this geography.
pub fn destination(lon: f64, lat: f64, distance_km: f64, bearing: f64) -> (f64, f64) {
    let lat = lat.to_radians();
    let d = distance_km / EARTH_KM;
    let target_lat = (lat.sin() * d.cos() + lat.cos() * d.sin() * bearing.cos())
        .clamp(-1.0, 1.0)
        .asin();
    let target_lon = lon.to_radians()
        + (bearing.sin() * d.sin() * lat.cos()).atan2(d.cos() - lat.sin() * target_lat.sin());
    (
        (target_lon.to_degrees() + 180.0).rem_euclid(360.0) - 180.0,
        target_lat.to_degrees(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pulse(weapon: WeaponType, age: u8) -> Explosion {
        Explosion {
            lon: 179.0,
            lat: 0.0,
            radius_km: 300.0,
            frame: age,
            weapon_type: weapon,
        }
    }

    fn fire(lon: f64, intensity: u8) -> Fire {
        Fire {
            lon,
            lat: 0.0,
            intensity,
            weapon_type: WeaponType::Nuke,
        }
    }

    #[test]
    fn water_reaches_contact_before_consuming_heat_across_date_line() {
        let mut world = Interactions::default();
        world.launch(&pulse(WeaponType::Water, 0));
        let mut fires = vec![fire(-179.0, 240), fire(160.0, 240)];
        assert!(!world.update(&mut fires));
        assert_eq!(fires.len(), 2);
        for _ in 0..20 {
            world.update(&mut fires);
        }
        assert_eq!(fires.len(), 1);
        assert_eq!(fires[0].lon, 160.0);
        assert!(world
            .reactions
            .values()
            .any(|r| r.kind == ReactionKind::Steam));
        // Residual moisture also responds to newly arriving fire.
        fires.push(fire(-179.0, 200));
        world.update(&mut fires);
        assert_eq!(fires.len(), 1);
    }

    #[test]
    fn wet_growth_and_steam_are_independent_of_launch_order() {
        let mut a = Interactions::default();
        let mut b = Interactions::default();
        for weapon in [WeaponType::Water, WeaponType::Life] {
            a.launch(&pulse(weapon, 30));
        }
        for weapon in [WeaponType::Life, WeaponType::Water] {
            b.launch(&pulse(weapon, 30));
        }
        let mut fa = vec![fire(179.0, 230)];
        let mut fb = fa.clone();
        a.update(&mut fa);
        b.update(&mut fb);
        assert!(fa.is_empty() && fb.is_empty());
        assert_eq!(
            a.reactions.keys().collect::<Vec<_>>(),
            b.reactions.keys().collect::<Vec<_>>()
        );
        assert!(a.reactions.values().any(|r| r.kind == ReactionKind::Bloom));
        assert!(a.reactions.values().any(|r| r.kind == ReactionKind::Steam));
        assert!(!a.reactions.values().any(|r| r.kind == ReactionKind::Scorch));
    }

    #[test]
    fn growth_distinguishes_hot_fire_from_dying_embers() {
        let mut world = Interactions::default();
        world.launch(&pulse(WeaponType::Life, 30));
        let mut fires = vec![fire(179.0, 240), fire(178.0, 70)];
        world.update(&mut fires);
        assert_eq!(fires[0].intensity, 238);
        assert_eq!(fires[1].intensity, 58);
        assert!(world
            .reactions
            .values()
            .any(|r| r.kind == ReactionKind::Scorch));
        assert!(world
            .reactions
            .values()
            .any(|r| r.kind == ReactionKind::Bloom));
    }

    #[test]
    fn dry_growth_still_processes_fire_and_expires_residue() {
        let mut world = Interactions::default();
        world.launch(&pulse(WeaponType::Life, 30));
        let mut fires = vec![fire(179.0, 240)];
        assert!(world.update(&mut fires));
        assert_eq!(fires[0].intensity, 238);
        assert!(world
            .reactions
            .values()
            .all(|r| r.kind == ReactionKind::Scorch));
        fires.clear();
        assert!(!world.update(&mut fires));
        assert!(world.reactions.values().all(|r| r.age == 1));
        for _ in 0..60 {
            world.update(&mut fires);
        }
        assert!(world.reactions.is_empty());
        assert!(!world.fields.is_empty());
    }

    #[test]
    fn shock_hollows_cloud_and_piles_a_charged_rim_then_settles() {
        let mut world = Interactions::default();
        world.launch(&pulse(WeaponType::Emp, 15));
        let field = &world.fields[0];
        let (lon, lat) = destination(field.lon, field.lat, field.front_km(), 1.0);
        let rim = lonlat_to_vec3(lon, lat);
        assert!(world.cloud_response(field.center).0 < 0.4);
        let (density, charge) = world.cloud_response(rim);
        assert!(density > 2.5 && charge > 0.9);
        for _ in 0..30 {
            world.update(&mut Vec::new());
        }
        assert_eq!(world.cloud_response(rim), (1.0, 0.0));
    }

    #[test]
    fn combined_pressure_is_order_independent_and_residue_expires() {
        let mut world = Interactions::default();
        for weapon in [WeaponType::Emp, WeaponType::Water, WeaponType::Life] {
            world.launch(&pulse(weapon, 20));
        }
        let point = lonlat_to_vec3(-179.0, 0.0);
        let response = world.cloud_response(point);
        world.fields.reverse();
        assert_eq!(response, world.cloud_response(point));
        world.update(&mut vec![fire(179.0, 240)]);
        assert!(!world.reactions.is_empty());
        for _ in 0..400 {
            world.update(&mut Vec::new());
        }
        assert!(!world.active());
    }

    #[test]
    fn polar_destination_preserves_geographic_radius() {
        let field = Field::new(&Explosion {
            lon: 179.0,
            lat: 88.0,
            radius_km: 500.0,
            frame: 20,
            weapon_type: WeaponType::Water,
        });
        for bearing in [0.0, 1.0, 3.0, 5.0] {
            let (lon, lat) = destination(field.lon, field.lat, field.front_km(), bearing);
            assert!((field.distance(lonlat_to_vec3(lon, lat)) - field.front_km()).abs() < 0.001);
            assert!((-180.0..180.0).contains(&lon));
        }
    }
}
