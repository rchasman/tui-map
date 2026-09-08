use super::{fast_pseudo_angle, GasCloudRender};
use crate::app::WeaponType;
use crate::hash::{hash2, hash3};
use crate::interactions::Interactions;
use crate::map::{globe::lonlat_to_vec3, Projection};
use glam::DVec3;
use ratatui::{buffer::Buffer, layout::Rect, style::Color};

struct CloudShape {
    center: DVec3,
    east: DVec3,
    north: DVec3,
    radius: f64,
    cutoff: f64,
    intensity: f64,
    lobes: [f64; 12],
    weapon: WeaponType,
}

impl CloudShape {
    fn new(cloud: &GasCloudRender, frame: u64) -> Self {
        let center = lonlat_to_vec3(cloud.lon, cloud.lat);
        let east = DVec3::new(
            -cloud.lon.to_radians().sin(),
            cloud.lon.to_radians().cos(),
            0.0,
        );
        let radius = (cloud.radius_km / 6371.0).clamp(0.000001, std::f64::consts::PI);
        let id = hash2(cloud.lon.to_bits(), cloud.lat.to_bits());
        let intensity = (cloud.intensity as f64 / 2000.0).min(1.0);
        let mut lobes = [0.0; 12];
        let t = (frame % 180) as f64 / 180.0;
        let smooth = t * t * (3.0 - 2.0 * t);
        for (i, lobe) in lobes.iter_mut().enumerate() {
            let a = (hash3(id, i as u64, frame / 180) & 255) as f64 / 255.0;
            let b = (hash3(id, i as u64, frame / 180 + 1) & 255) as f64 / 255.0;
            *lobe = (0.65 + 0.3 * (a + (b - a) * smooth)) * (0.4 + intensity * 0.6);
        }
        Self {
            center,
            east,
            north: center.cross(east),
            radius,
            cutoff: radius.cos(),
            intensity,
            lobes,
            weapon: cloud.weapon_type,
        }
    }

    fn density(&self, point: DVec3) -> f64 {
        let dot = self.center.dot(point).clamp(-1.0, 1.0);
        if dot < self.cutoff {
            return 0.0;
        }
        // Tangent coordinates anchor shape and texture to geography, including
        // at the date line and when the cloud center is beyond the viewport.
        let east = self.east.dot(point);
        let north = self.north.dot(point);
        let angle = fast_pseudo_angle(east as f32, north as f32) as f64 * 3.0;
        let index = angle.floor() as usize % 12;
        let t = angle.fract();
        let t = t * t * (3.0 - 2.0 * t);
        let lobe = self.lobes[index] * (1.0 - t) + self.lobes[(index + 1) % 12] * t;
        let distance = dot.acos() / (self.radius * lobe);
        if distance >= 1.0 {
            return 0.0;
        }
        let texture =
            0.88 + 0.12 * (east / self.radius * 23.0 + (north / self.radius * 17.0).sin()).sin();
        (1.0 - distance).powi(2) * self.intensity * texture
    }
}

pub fn render_merged(
    clouds: &[GasCloudRender],
    density_buf: &mut [(f32, f32)],
    area: Rect,
    frame: u64,
    buf: &mut Buffer,
    projection: &Projection,
) {
    render_interacting(
        clouds,
        density_buf,
        area,
        frame,
        buf,
        projection,
        &Interactions::default(),
    );
}

pub fn render_interacting(
    clouds: &[GasCloudRender],
    density_buf: &mut [(f32, f32)],
    area: Rect,
    frame: u64,
    buf: &mut Buffer,
    projection: &Projection,
    interactions: &Interactions,
) {
    density_buf.fill((0.0, 0.0));
    if clouds.is_empty() || area.is_empty() {
        return;
    }
    let shapes: Vec<_> = clouds.iter().map(|c| CloudShape::new(c, frame)).collect();
    let cloud_response = interactions.prepare_cloud_response();
    for row in 0..area.height {
        for col in 0..area.width {
            let Some((lon, lat)) = projection.unproject(col as i32 * 2, row as i32 * 4) else {
                continue;
            };
            let point = lonlat_to_vec3(lon, lat);
            // Fixed-point accumulation makes reversing cloud order exactly stable.
            let (mut bio, mut chem) = (0u64, 0u64);
            for shape in &shapes {
                let density = (shape.density(point) * 65536.0) as u64;
                match shape.weapon {
                    WeaponType::Bio => bio += density,
                    WeaponType::Chem => chem += density,
                    _ => {}
                }
            }
            if bio + chem == 0 {
                continue;
            }
            let (displacement, electric) = cloud_response(point);
            let bio = bio as f32 / 65536.0 * displacement;
            let chem = chem as f32 / 65536.0 * displacement;
            density_buf[row as usize * area.width as usize + col as usize] = (bio, chem);
            if bio + chem < 0.05 {
                continue;
            }
            let (r, g, b, ch) = mixed_color(bio, chem, lon, lat, frame, electric);
            buf[(area.x + col, area.y + row)]
                .set_char(ch)
                .set_fg(Color::Rgb(r, g, b));
        }
    }
}

/// Geographic flowing wisps retain both material colors. A narrow luminous seam
/// marks balanced mixtures; ionization traces local density contours.
fn mixed_color(
    bio: f32,
    chem: f32,
    lon: f64,
    lat: f64,
    frame: u64,
    electric: f32,
) -> (u8, u8, u8, char) {
    let total = bio + chem;
    let flow = ((lon.to_radians().sin() * 32.0
        + lat.to_radians().sin() * 41.0
        + (lat.to_radians() * 9.0).sin() * 2.0
        - frame as f64 * 0.025)
        .sin()
        * 0.5
        + 0.5) as f32;
    let share = bio / total.max(0.0001);
    let (mut r, mut g, mut b, mut ch) = if flow < share {
        bio_density_color(total, flow)
    } else {
        chem_density_color(total, flow)
    };
    if bio > 0.035 && chem > 0.035 {
        ch = if flow < share { '⣜' } else { '⢣' };
        let seam =
            (1.0 - (share - 0.5).abs() * 7.0).max(0.0) * (1.0 - (flow - 0.5).abs() * 5.0).max(0.0);
        r = (r as f32 + seam * 100.0).min(235.0) as u8;
        g = (g as f32 + seam * 105.0).min(245.0) as u8;
        b = (b as f32 + seam * 110.0).min(245.0) as u8;
    }
    let contour = (total * 27.0 + flow * 3.0 - frame as f32 * 0.12).sin();
    let arc = electric * ((contour - 0.65) / 0.35).max(0.0);
    if arc > 0.08 {
        r = (r as f32 * (1.0 - arc) + 130.0 * arc) as u8;
        g = (g as f32 * (1.0 - arc) + 245.0 * arc) as u8;
        b = (b as f32 * (1.0 - arc) + 255.0 * arc) as u8;
        ch = if contour > 0.94 { '⠿' } else { '⢎' };
    }
    (r, g, b, ch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::Viewport;

    fn cloud(weapon_type: WeaponType, lon: f64) -> GasCloudRender {
        GasCloudRender {
            intensity: 1800,
            weapon_type,
            lon,
            lat: 0.0,
            radius_km: 3000.0,
        }
    }

    #[test]
    fn cloud_order_and_offscreen_centers_do_not_change_density() {
        let area = Rect::new(3, 2, 80, 40);
        let projection = Projection::Mercator(Viewport::new(0.0, 0.0, 8.0, 160, 160));
        let mut clouds = vec![cloud(WeaponType::Bio, 25.0), cloud(WeaponType::Chem, 24.0)];
        let mut a = Buffer::empty(area);
        let mut b = a.clone();
        let mut da = vec![(0.0, 0.0); 3200];
        let mut db = da.clone();
        render_merged(&clouds, &mut da, area, 50, &mut a, &projection);
        clouds.reverse();
        render_merged(&clouds, &mut db, area, 50, &mut b, &projection);
        assert_eq!(da, db);
        assert_eq!(a, b);
        assert!(da.iter().any(|&(bio, chem)| bio > 0.05 && chem > 0.05));
        assert!(a.content.iter().any(|c| c.symbol() != " "));
    }

    #[test]
    fn mixed_wisps_keep_both_hues_and_emp_lights_contours() {
        let mut green = false;
        let mut violet = false;
        let mut arc = false;
        for frame in 0..200 {
            let plain = mixed_color(0.4, 0.4, 0.0, 0.0, frame, 0.0);
            let charged = mixed_color(0.4, 0.4, 0.0, 0.0, frame, 1.0);
            green |= plain.1 > plain.0 && plain.1 > plain.2;
            violet |= plain.0 > plain.1 && plain.2 > plain.1;
            arc |= charged.2 > plain.2 && charged.1 > plain.1;
        }
        assert!(green && violet && arc);
    }
}

/// Map accumulated bio density to color — overlap produces super-dense visuals
pub fn bio_density_color(d: f32, shade: f32) -> (u8, u8, u8, char) {
    if d > 1.0 {
        let extra = (d - 1.0).min(1.0);
        (
            (15.0 + extra * 25.0 + shade * 10.0) as u8,
            (220.0 + extra * 35.0).min(255.0) as u8,
            (40.0 + extra * 20.0 + shade * 10.0) as u8,
            '█',
        )
    } else if d > 0.5 {
        (
            (10.0 + shade * 15.0) as u8,
            (180.0 + shade * 40.0) as u8,
            (30.0 + shade * 15.0) as u8,
            '▓',
        )
    } else if d > 0.2 {
        (
            0,
            (100.0 + shade * 40.0) as u8,
            (15.0 + shade * 10.0) as u8,
            '▒',
        )
    } else {
        (
            0,
            (45.0 + shade * 25.0) as u8,
            (5.0 + shade * 5.0) as u8,
            '░',
        )
    }
}

/// Map accumulated chem density to color
pub fn chem_density_color(d: f32, shade: f32) -> (u8, u8, u8, char) {
    if d > 1.0 {
        let extra = (d - 1.0).min(1.0);
        (
            (160.0 + extra * 50.0).min(255.0) as u8,
            (10.0 + extra * 15.0) as u8,
            (200.0 + extra * 55.0).min(255.0) as u8,
            '█',
        )
    } else if d > 0.5 {
        (
            (120.0 + shade * 40.0) as u8,
            (5.0 + shade * 10.0) as u8,
            (160.0 + shade * 40.0) as u8,
            '▓',
        )
    } else if d > 0.2 {
        (
            (65.0 + shade * 30.0) as u8,
            0,
            (100.0 + shade * 30.0) as u8,
            '▒',
        )
    } else {
        (
            (25.0 + shade * 15.0) as u8,
            0,
            (45.0 + shade * 20.0) as u8,
            '░',
        )
    }
}
