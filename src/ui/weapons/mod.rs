pub mod nuke;
pub mod bio;
pub mod emp;
pub mod chem;
pub mod gas_clouds;

use crate::app::WeaponType;
use crate::map::GlobeViewport;
use ratatui::{buffer::Buffer, layout::Rect, style::Color};

/// Screen-space explosion ready for rendering
pub struct ExplosionRender {
    pub x: u16,
    pub y: u16,
    pub frame: u8,
    pub radius: u16,
    pub weapon_type: WeaponType,
    pub lon: f64,
    pub lat: f64,
    pub radius_km: f64,
}

/// Screen-space gas cloud ready for rendering
pub struct GasCloudRender {
    pub x: u16,
    pub y: u16,
    pub radius: u16,
    pub intensity: u16,
    pub weapon_type: WeaponType,
    pub lon: f64,
    pub lat: f64,
    pub radius_km: f64,
}

/// Fast pseudo-angle using diamond angle technique.
/// Returns a value in [0, 4) that varies monotonically with angle,
/// suitable for turbulence seeding. Replaces atan2 (~10x faster).
#[inline(always)]
pub fn fast_pseudo_angle(dx: f32, dy: f32) -> f32 {
    let ax = dx.abs();
    let ay = dy.abs();
    let s = ax + ay;
    if s < 1e-6 { return 0.0; }
    let d = dy / s;
    if dx >= 0.0 { 1.0 - d } else { 3.0 + d }
}

/// Map weapon type to its signature color
pub fn weapon_color(weapon: WeaponType) -> Color {
    match weapon {
        WeaponType::Nuke => Color::Red,
        WeaponType::Bio => Color::Rgb(0, 255, 50),
        WeaponType::Emp => Color::Rgb(0, 200, 255),
        WeaponType::Chem => Color::Rgb(200, 0, 200),
    }
}

/// Dispatch explosion rendering to the appropriate weapon renderer
pub fn render_explosion(exp: &ExplosionRender, x: u16, y: u16, area: Rect, global_frame: u64, buf: &mut Buffer, globe: Option<&GlobeViewport>) {
    match exp.weapon_type {
        WeaponType::Nuke => nuke::render(exp, x, y, area, global_frame, buf, globe),
        WeaponType::Bio => bio::render(exp, x, y, area, global_frame, buf, globe),
        WeaponType::Emp => emp::render(exp, x, y, area, global_frame, buf, globe),
        WeaponType::Chem => chem::render(exp, x, y, area, global_frame, buf, globe),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fast_pseudo_angle_range() {
        for &(dx, dy) in &[
            (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (-1.0, 1.0),
            (-1.0, 0.0), (-1.0, -1.0), (0.0, -1.0), (1.0, -1.0),
        ] {
            let a = fast_pseudo_angle(dx, dy);
            assert!(a >= 0.0 && a < 4.0, "angle {a} out of range for ({dx}, {dy})");
        }
    }

    #[test]
    fn fast_pseudo_angle_zero() {
        assert_eq!(fast_pseudo_angle(0.0, 0.0), 0.0);
    }

    #[test]
    fn fast_pseudo_angle_monotonic_quadrant1() {
        let a0 = fast_pseudo_angle(1.0, 0.0);
        let a1 = fast_pseudo_angle(1.0, 0.5);
        let a2 = fast_pseudo_angle(1.0, 1.0);
        let a3 = fast_pseudo_angle(0.5, 1.0);
        assert!(a0 > a1, "not monotonic: {a0} <= {a1}");
        assert!(a1 > a2, "not monotonic: {a1} <= {a2}");
        assert!(a2 > a3, "not monotonic: {a2} <= {a3}");
    }
}
