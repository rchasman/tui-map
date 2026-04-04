use crate::hash::hash3;
use crate::map::GlobeViewport;
use crate::map::globe::lonlat_to_vec3;
use super::{ExplosionRender, fast_acos_approx};
use ratatui::{buffer::Buffer, layout::Rect, style::Color};

/// EMP: expanding concentric rings — electric blue/cyan, fast, short duration
pub fn render(exp: &ExplosionRender, x: u16, y: u16, area: Rect, global_frame: u64, buf: &mut Buffer, globe: Option<&GlobeViewport>) {
    // 3 rings expanding at staggered speeds, fills radius by frame 15
    let progress = (exp.frame as f32 / 15.0).min(1.0); // Full expansion by frame 15
    let fade = if exp.frame > 15 { (exp.frame - 15) as f32 / 15.0 } else { 0.0 };

    let max_r = exp.radius as f32 * progress;

    // 3 ring radii at different expansion speeds
    let ring_radii = [
        max_r * 1.0,            // Outer ring (fastest)
        max_r * 0.65,           // Middle ring
        max_r * 0.35,           // Inner ring
    ];
    let ring_thickness = 2.0_f32; // ~2 chars thick

    // Globe: geographic → screen distance mapping (angular distance × scale factor)
    let center_vec = lonlat_to_vec3(exp.lon, exp.lat);
    let geo_scale = {
        let max_angle = exp.radius_km / 6371.0;
        exp.radius as f64 / max_angle
    };

    // Scan area covers full circle, clamped to viewport
    let scan_r = (max_r as i16) + 3;
    let dy_lo = (-scan_r).max(-(y as i16));
    let dy_hi = scan_r.min((area.y + area.height - 1) as i16 - y as i16);
    let dx_lo = (-scan_r).max(-(x as i16));
    let dx_hi = scan_r.min((area.x + area.width - 1) as i16 - x as i16);

    for dy in dy_lo..=dy_hi {
        let py = (y as i16 + dy) as u16;

        for dx in dx_lo..=dx_hi {
            let px = (x as i16 + dx) as u16;

            // Distance: geographic on globe (conforms to curvature), screen-space on Mercator
            let dist: f32 = if let Some(g) = globe {
                let bx = (px as i32 - area.x as i32) * 2;
                let by = (py as i32 - area.y as i32) * 4;
                match g.pixel_to_sphere_point(bx, by) {
                    None => continue, // outside globe disk
                    Some(p) => {
                        let dot = p.dot(center_vec).clamp(-1.0, 1.0);
                        (fast_acos_approx(dot) * geo_scale) as f32
                    }
                }
            } else {
                ((dx * dx + dy * dy) as f32).sqrt()
            };

            // Check if this pixel is near any ring
            let mut best_ring: Option<(f32, usize)> = None; // (proximity to ring, ring_index)
            for (i, &ring_r) in ring_radii.iter().enumerate() {
                if ring_r < 1.0 { continue; }
                let proximity = (dist - ring_r).abs();
                if proximity <= ring_thickness {
                    if best_ring.is_none() || proximity < best_ring.unwrap().0 {
                        best_ring = Some((proximity, i));
                    }
                }
            }

            // Also add flickering arc sparks between rings
            let spark_seed = hash3(dx as u64, dy as u64, global_frame + exp.frame as u64);
            let is_spark = (spark_seed & 0x1F) == 0 && dist < max_r && dist > ring_radii[2] * 0.5;

            if let Some((proximity, ring_idx)) = best_ring {
                let ring_fade = proximity / ring_thickness; // 0 at center, 1 at edge
                let age_fade = 1.0 - fade;

                // Rapid pulse/flicker (frame-by-frame jitter)
                let jitter = ((spark_seed & 0x3) as f32) / 3.0;
                let brightness = ((1.0 - ring_fade) * age_fade * (0.7 + jitter * 0.3)).min(1.0);

                if brightness < 0.05 { continue; }

                // Color: inner rings brighter cyan, outer rings deeper blue
                let (r, g, b, ch) = match ring_idx {
                    0 => { // Outer ring — deep blue fading
                        let b_val = (200.0 * brightness) as u8;
                        (0, (80.0 * brightness) as u8, b_val, if brightness > 0.5 { '▓' } else { '░' })
                    }
                    1 => { // Middle ring — electric cyan
                        ((50.0 * brightness) as u8, (200.0 * brightness) as u8, (255.0 * brightness) as u8,
                         if brightness > 0.6 { '█' } else { '▒' })
                    }
                    _ => { // Inner ring — blinding white-cyan
                        let w = (brightness * 255.0) as u8;
                        (w, w, (255.0 * brightness) as u8, '█')
                    }
                };

                buf[(px, py)].set_char(ch).set_fg(Color::Rgb(r, g, b));
            } else if is_spark && fade < 0.5 {
                // Arc sparks between rings
                buf[(px, py)].set_char('·').set_fg(Color::Rgb(0, 255, 255));
            }
        }
    }
}
