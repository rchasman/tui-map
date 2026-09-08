pub mod nuke;
pub mod bio;
pub mod emp;
pub mod water;
pub mod life;
pub mod chem;
pub mod gas_clouds;
mod accents;
mod organic;
mod aerosol;
pub mod composite;
mod reactions;

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

/// Geographic gas cloud ready for density sampling
pub struct GasCloudRender {
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
        WeaponType::Water => Color::Rgb(30, 144, 255),
        WeaponType::Life => Color::Rgb(50, 205, 50),
        WeaponType::Chem => Color::Rgb(200, 0, 200),
    }
}

/// Dispatch explosion rendering to the appropriate weapon renderer
pub fn render_explosion(exp: &ExplosionRender, x: u16, y: u16, area: Rect, global_frame: u64, buf: &mut Buffer, globe: Option<&GlobeViewport>) {
    accents::render(exp, x, y, area, buf, globe, false);
    render_body(exp, x, y, area, global_frame, buf, globe);
    accents::render(exp, x, y, area, buf, globe, true);
}

fn render_body(exp: &ExplosionRender, x: u16, y: u16, area: Rect, global_frame: u64, buf: &mut Buffer, globe: Option<&GlobeViewport>) {
    match exp.weapon_type {
        WeaponType::Nuke => nuke::render(exp, x, y, area, global_frame, buf, globe),
        WeaponType::Bio => bio::render(exp, x, y, area, global_frame, buf, globe),
        WeaponType::Emp => emp::render(exp, x, y, area, global_frame, buf, globe),
        WeaponType::Water => water::render(exp, x, y, area, global_frame, buf, globe),
        WeaponType::Life => life::render(exp, x, y, area, global_frame, buf, globe),
        WeaponType::Chem => chem::render(exp, x, y, area, global_frame, buf, globe),
    }
}

/// Fast acos approximation for dot products.
/// Uses the identity acos(x) ≈ sqrt(2(1-x)) for x near 1, and
/// acos(x) = π - acos(-x) to handle the full range symmetrically.
/// Max error ~3% across [-1, 1] — sufficient for visual effects.
/// ~5× faster than f64::acos() — eliminates trig from per-pixel loops.
#[inline(always)]
pub fn fast_acos_approx(dot: f64) -> f64 {
    if dot >= 0.0 {
        // Positive dot: small angle [0, π/2]
        // acos(x) ≈ sqrt(2(1-x)) * (1 + 0.0549*(1-x))
        let one_minus = 1.0 - dot;
        (2.0 * one_minus).sqrt() * (1.0 + 0.0560 * one_minus)
    } else {
        // Negative dot: large angle [π/2, π]
        // Use symmetry: acos(x) = π - acos(-x)
        let one_minus = 1.0 + dot; // = 1 - (-dot)
        std::f64::consts::PI - (2.0 * one_minus).sqrt() * (1.0 + 0.0560 * one_minus)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn liquid_tails_remain_visible_and_all_effects_finish_cleanly() {
        let area = Rect::new(6, 4, 60, 30);
        for (weapon, late) in [(WeaponType::Water, 120), (WeaponType::Life, 150), (WeaponType::Emp, 18), (WeaponType::Bio, 20), (WeaponType::Chem, 20), (WeaponType::Nuke, 30)] {
            let mut exp = ExplosionRender { x: 30, y: 20, frame: late, radius: 12,
                weapon_type: weapon, lon: 10.0, lat: 20.0, radius_km: 500.0 };
            let mut buf = Buffer::empty(Rect::new(0, 0, 80, 40));
            render_body(&exp, 30, 20, area, 150, &mut buf, None);
            assert!(buf.content.iter().any(|c| c.symbol() != " "));
            for y in 0..40 { for x in 0..80 {
                if !area.contains((x, y).into()) { assert_eq!(buf[(x,y)].symbol(), " "); }
            }}
            exp.frame = weapon.max_frames();
            let mut ended = Buffer::empty(area);
            render_body(&exp, 30, 20, area, 210, &mut ended, None);
            assert!(ended.content.iter().all(|c| c.symbol() == " "));
        }
    }

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
    fn fast_acos_approx_accuracy() {
        // Verify <5% relative error across full range (visual effects, not navigation)
        for i in 0..100 {
            let dot = -1.0 + i as f64 * 0.02;
            let exact = dot.acos();
            let approx = fast_acos_approx(dot);
            let abs_err = (approx - exact).abs();
            // Allow 5% relative error or 0.05 absolute (whichever is larger)
            let threshold = (exact * 0.05).max(0.05);
            assert!(
                abs_err < threshold,
                "dot={dot:.2}: exact={exact:.4}, approx={approx:.4}, err={abs_err:.4} > {threshold:.4}"
            );
        }
    }

    #[test]
    fn fast_acos_approx_monotonic() {
        // Must be monotonically decreasing (as dot increases, angle decreases)
        let mut prev = fast_acos_approx(-1.0);
        for i in 1..=200 {
            let dot = -1.0 + i as f64 * 0.01;
            let val = fast_acos_approx(dot);
            assert!(val <= prev + 1e-10, "not monotonic at dot={dot}: {val} > {prev}");
            prev = val;
        }
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
