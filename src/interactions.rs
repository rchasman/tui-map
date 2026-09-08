//! Geographic effect fields and short-lived reaction residue. No viewport state.
use crate::{
    app::{Explosion, Fire, GasCloud, WeaponType},
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
        self.age as f64 / self.weapon.front_frames() as f64
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
            WeaponType::Water => 360,
            WeaponType::Frost => 240,
            WeaponType::Tornado => self.weapon.max_frames() as u16,
            WeaponType::Life => 420,
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
            ReactionKind::Steam => 140,
            ReactionKind::Bloom => 180,
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
        // Sum wind from a snapshot before moving anything, so overlapping
        // vortices cannot depend on launch order. Quenching uses the new position.
        if self.fields.iter().any(|f| f.weapon == WeaponType::Tornado) {
            for fire in fires.iter_mut() {
                let (lon, lat) = self.wind_position(fire.lon, fire.lat);
                changed |= lon != fire.lon || lat != fire.lat;
                fire.lon = lon;
                fire.lat = lat;
            }
        }
        let reaches: Vec<_> = self
            .fields
            .iter()
            .filter(|f| f.weapon.is_restorative())
            .map(|f| (f, (f.front_km() / EARTH_KM).cos()))
            .collect();
        if reaches.is_empty() {
            return changed;
        }
        for fire in fires.iter_mut() {
            let point = lonlat_to_vec3(fire.lon, fire.lat);
            // Choose reactions from a snapshot: water always takes precedence over
            // growth, regardless of field insertion order.
            let wet = reaches
                .iter()
                .filter(|(f, reach)| {
                    matches!(f.weapon, WeaponType::Water | WeaponType::Frost)
                        && f.center.dot(point) >= *reach
                })
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
        if !reaches
            .iter()
            .any(|(f, _)| matches!(f.weapon, WeaponType::Water | WeaponType::Frost))
        {
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
                            matches!(f.weapon, WeaponType::Water | WeaponType::Frost)
                                && f.center.dot(point) >= *reach
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

    /// Transport around the sphere, including the date line and poles.
    /// Fixed-point vector addition makes overlapping winds order independent.
    fn wind_position(&self, lon: f64, lat: f64) -> (f64, f64) {
        let point = lonlat_to_vec3(lon, lat);
        let mut drift = [0i64; 3];
        for field in self
            .fields
            .iter()
            .filter(|f| f.weapon == WeaponType::Tornado)
        {
            if !field.contains(point) {
                continue;
            }
            let distance = field.distance(point);
            let weight = (1.0 - distance / field.front_km().max(1.0)).max(0.0);
            let fade = (1.0 - field.age as f64 / field.lifetime() as f64).min(0.3) / 0.3;
            let tangent = field.center.cross(point);
            let velocity = tangent * (0.08 * weight * fade);
            for (sum, value) in drift.iter_mut().zip(velocity.to_array()) {
                *sum += (value * 1e12).round() as i64;
            }
        }
        if drift == [0; 3] {
            return (lon, lat);
        }
        let moved = (point + DVec3::from_array(drift.map(|v| v as f64 / 1e12))).normalize();
        (
            moved.y.atan2(moved.x).to_degrees(),
            moved.z.clamp(-1.0, 1.0).asin().to_degrees(),
        )
    }

    pub fn advect_clouds(&self, clouds: &mut [GasCloud]) {
        if !self.fields.iter().any(|f| f.weapon == WeaponType::Tornado) {
            return;
        }
        for cloud in clouds {
            (cloud.lon, cloud.lat) = self.wind_position(cloud.lon, cloud.lat);
        }
    }

    /// Temporary cloud displacement and ionization at this world point.
    /// Fixed-point pressure and charge sums are independent of field order.
    pub fn cloud_response(&self, point: DVec3) -> (f32, f32) {
        resolve_cloud_response(self.fields.iter().filter_map(CloudField::new), point)
    }

    /// Snapshot per-field wave geometry once for all cloud cells in this render.
    pub(crate) fn prepare_cloud_response(&self) -> impl Fn(DVec3) -> (f32, f32) {
        let fields: Vec<_> = self.fields.iter().filter_map(CloudField::new).collect();
        move |point| resolve_cloud_response(fields.iter().copied(), point)
    }
}

#[derive(Clone, Copy)]
struct CloudField {
    center: DVec3,
    front: f64,
    width: f64,
    cutoff: f64,
    recovery: f64,
    charged: bool,
}

impl CloudField {
    fn new(field: &Field) -> Option<Self> {
        let t = field.progress();
        if field.weapon == WeaponType::Tornado {
            if field.age == 0 || field.age >= field.lifetime() {
                return None;
            }
            let front = field.front_km() * 0.5;
            let width = (field.radius_km * 0.3).max(1.0);
            return Some(Self {
                center: field.center,
                front,
                width,
                cutoff: ((front + width * 3.0) / EARTH_KM).cos(),
                recovery: ((field.lifetime() - field.age) as f64 / 50.0).min(1.0),
                charged: false,
            });
        }
        if t <= 0.0 || t >= 1.4 {
            return None;
        }
        let front = field.front_km();
        let width = (field.radius_km * 0.22).max(1.0);
        Some(Self {
            center: field.center,
            front,
            width,
            cutoff: ((front + width * 3.0) / EARTH_KM).cos(),
            recovery: if t > 0.8 {
                ((1.4 - t) / 0.6).max(0.0)
            } else {
                1.0
            },
            charged: field.weapon == WeaponType::Emp,
        })
    }
}

fn resolve_cloud_response(fields: impl Iterator<Item = CloudField>, point: DVec3) -> (f32, f32) {
    let mut pressure = 0i64;
    let mut charge = 0i64;
    for field in fields {
        let dot = field.center.dot(point);
        if dot < field.cutoff {
            continue;
        }
        let d = dot.clamp(-1.0, 1.0).acos() * EARTH_KM;
        let rim = (1.0 - ((d - field.front) / field.width).abs()).max(0.0);
        let hollow = if d < field.front {
            (1.0 - rim) * 0.72
        } else {
            0.0
        };
        pressure += (field.recovery * (rim * 1.8 - hollow) * 65536.0) as i64;
        if field.charged {
            let contact = (1.0 - ((d - field.front) / (field.width * 2.5)).abs()).max(0.0);
            charge += (contact * field.recovery * 65536.0) as i64;
        }
    }
    (
        (1.0 + pressure as f32 / 65536.0).clamp(0.08, 3.0),
        (charge as f32 / 65536.0).min(1.0),
    )
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
        Explosion { seed: 0,
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
    fn prepared_cloud_response_matches_scalar_through_field_lifetimes() {
        let mut world = Interactions::default();
        for weapon in [WeaponType::Emp, WeaponType::Water, WeaponType::Life] {
            world.launch(&pulse(weapon, 0));
        }
        for age in [0, 1, 15, 30, 45, 90, 180] {
            for field in &mut world.fields {
                field.age = age;
            }
            let prepared = world.prepare_cloud_response();
            for lat in [-88.0, -3.0, 0.0, 3.0, 88.0] {
                for lon in -180..180 {
                    let point = lonlat_to_vec3(lon as f64, lat);
                    assert_eq!(prepared(point), world.cloud_response(point));
                }
            }
        }
        world.fields.clear();
        assert_eq!(world.prepare_cloud_response()(DVec3::X), (1.0, 0.0));
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
        for _ in 0..700 {
            world.update(&mut Vec::new());
        }
        assert!(!world.active());
    }

    #[test]
    fn polar_destination_preserves_geographic_radius() {
        let field = Field::new(&Explosion { seed: 0,
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

    #[test]
    fn wind_transports_hazards_across_date_line_and_poles_in_any_order() {
        for lat in [0.0, 88.0, -88.0] {
            let mut world = Interactions::default();
            for lon in [179.0, -179.0] {
                let mut exp = pulse(WeaponType::Tornado, 50);
                exp.lon = lon;
                exp.lat = lat;
                world.launch(&exp);
            }
            let before = (179.9, lat + 0.5);
            let moved = world.wind_position(before.0, before.1);
            assert_ne!(moved, before);
            assert!((-180.0..=180.0).contains(&moved.0));
            assert!((-90.0..=90.0).contains(&moved.1));
            world.fields.reverse();
            assert_eq!(moved, world.wind_position(before.0, before.1));
            let mut clouds = vec![GasCloud {
                lon: before.0,
                lat: before.1,
                current_radius_km: 30.0,
                max_radius_km: 90.0,
                intensity: 1000,
                weapon_type: WeaponType::Chem,
            }];
            world.advect_clouds(&mut clouds);
            assert_eq!((clouds[0].lon, clouds[0].lat), moved);
            assert_eq!(clouds[0].intensity, 1000);
            let mut fires = vec![Fire {
                lon: before.0,
                lat: before.1,
                intensity: 200,
                weapon_type: WeaponType::Meteor,
            }];
            assert!(world.update(&mut fires));
            assert_ne!((fires[0].lon, fires[0].lat), before);
            assert_eq!(fires[0].intensity, 200);
            assert_eq!(world.wind_position(0.0, 0.0), (0.0, 0.0));
            for _ in 0..250 {
                world.update(&mut Vec::new());
            }
            assert!(!world.active());
            assert_eq!(world.wind_position(before.0, before.1), before);
        }
    }

    #[test]
    fn frost_quenches_windblown_meteor_fire_and_blooms_in_any_order() {
        let mut world = Interactions::default();
        for weapon in [WeaponType::Tornado, WeaponType::Frost, WeaponType::Life] {
            world.launch(&pulse(weapon, 30));
        }
        let mut reversed = Interactions::default();
        reversed.fields = world.fields.iter().rev().cloned().collect();
        let mut fires = vec![fire(-179.0, 230)];
        fires[0].weapon_type = WeaponType::Meteor;
        let mut other = fires.clone();
        world.update(&mut fires);
        reversed.update(&mut other);
        assert!(fires.is_empty() && other.is_empty());
        assert_eq!(
            world.reactions.keys().collect::<Vec<_>>(),
            reversed.reactions.keys().collect::<Vec<_>>()
        );
        assert!(world
            .reactions
            .values()
            .any(|r| r.kind == ReactionKind::Steam));
        assert!(world
            .reactions
            .values()
            .any(|r| r.kind == ReactionKind::Bloom));
        fires.push(fire(-179.0, 200));
        world.update(&mut fires);
        assert!(fires.is_empty(), "residual frost prevents reignition");
        for _ in 0..700 {
            world.update(&mut fires);
        }
        assert!(!world.active());
        fires.push(fire(-179.0, 200));
        world.update(&mut fires);
        assert_eq!(fires.len(), 1, "expired frost no longer quenches");
    }

    #[test]
    fn tornado_hollows_centered_gas_until_the_wind_expires() {
        let mut world = Interactions::default();
        world.launch(&pulse(WeaponType::Tornado, 150));
        let center = world.fields[0].center;
        let (lon, lat) = destination(179.0, 0.0, world.fields[0].front_km() * 0.5, 1.0);
        let rim = lonlat_to_vec3(lon, lat);
        assert!(world.cloud_response(center).0 < 0.4);
        assert!(world.cloud_response(rim).0 > 2.5);
        for point in [center, rim] {
            assert_eq!(
                world.prepare_cloud_response()(point),
                world.cloud_response(point)
            );
        }
        for _ in 0..70 {
            world.update(&mut Vec::new());
        }
        assert_eq!(world.cloud_response(center), (1.0, 0.0));
        assert_eq!(world.cloud_response(rim), (1.0, 0.0));
    }
}
