use crate::hash::{hash2, hash3};
use crate::map::GlobeViewport;
use crate::map::globe::lonlat_to_vec3;
use super::ExplosionRender;
use ratatui::{buffer::Buffer, layout::Rect, style::Color};

/// Chem: dense dome/sphere expanding in ALL directions — purple palette, dripping
pub fn render(exp: &ExplosionRender, x: u16, y: u16, area: Rect, global_frame: u64, buf: &mut Buffer, globe: Option<&GlobeViewport>) {
    let progress = if exp.frame < 20 {
        (exp.frame as f32 / 20.0).powf(0.6)
    } else if exp.frame < 40 {
        1.0 + ((exp.frame - 20) as f32 / 20.0) * 0.3
    } else {
        1.3
    };
    let max_r = exp.radius as f32 * progress;

    // Spherical: equal radius in all directions (above AND below)
    let sphere_r = (max_r * 1.5) as i16;
    let sphere_r_f32 = sphere_r as f32;

    let flash_phase = exp.frame < 6;
    let fireball_phase = exp.frame < 22;
    let cooling_phase = exp.frame < 45;

    let radius_i16 = (exp.radius as f32 * 1.5) as i16;
    let frame_seed_component = global_frame + exp.frame as u64;

    // Globe: geographic → screen distance mapping
    let center_vec = lonlat_to_vec3(exp.lon, exp.lat);
    let geo_scale = {
        let max_angle = exp.radius_km / 6371.0;
        // Scale maps geographic angle to screen units matching sphere_r_f32
        (exp.radius as f64 * 1.5) / max_angle
    };

    // Drip zone: extra chars trailing below the sphere
    let drip_extra = (max_r * 0.3) as i16;

    // Clamp loop bounds to viewport
    let dy_lo = (-sphere_r).max(-(y as i16));
    let dy_hi = (sphere_r + drip_extra).min((area.y + area.height - 1) as i16 - y as i16);
    let dx_lo = (-radius_i16).max(-(x as i16));
    let dx_hi = radius_i16.min((area.x + area.width - 1) as i16 - x as i16);

    for dy in dy_lo..=dy_hi {
        let py = (y as i16 + dy) as u16;

        let dy_sq = dy * dy;
        let is_drip_zone = dy > sphere_r;

        for dx in dx_lo..=dx_hi {
            let px = (x as i16 + dx) as u16;

            // Distance: geographic on globe, screen-space on Mercator
            let dist: f32 = if let Some(g) = globe {
                let bx = (px as i32 - area.x as i32) * 2;
                let by = (py as i32 - area.y as i32) * 4;
                match g.pixel_to_sphere_point(bx, by) {
                    None => continue, // outside globe disk
                    Some(p) => {
                        let dot = p.dot(center_vec).clamp(-1.0, 1.0);
                        (dot.acos() * geo_scale) as f32
                    }
                }
            } else {
                ((dx * dx + dy_sq) as f32).sqrt()
            };

            // Dense sphere check (less turbulence = more solid fill)
            let turb_seed = hash3(dx as u64, dy as u64, frame_seed_component);
            let turbulence = ((turb_seed & 0xFF) as f32 / 255.0 - 0.5) * 0.25; // Low turbulence

            let effective_r = sphere_r_f32 * (1.0 + turbulence);

            let in_sphere = if is_drip_zone {
                // Drip effect: narrow vertical trails below sphere (screen-space)
                let drip_seed = hash2(dx as u64, global_frame / 3);
                let drip_chance = (drip_seed & 0x7) < 2; // ~25% of columns drip
                let drip_progress = (dy - sphere_r) as f32 / drip_extra as f32;
                drip_chance && dx.abs() < radius_i16 / 2 && drip_progress < (1.0 - (dx.abs() as f32 / radius_i16 as f32))
            } else {
                dist <= effective_r
            };

            if in_sphere {
                let dist_norm = if is_drip_zone {
                    0.8 + 0.2 * ((dy - sphere_r) as f32 / drip_extra.max(1) as f32)
                } else {
                    (dist / effective_r).min(1.0)
                };

                let seed = hash3(px as u64, py as u64, global_frame + exp.frame as u64);
                let flicker = ((seed & 0xFF) as f32) / 255.0;

                let (r, g, b, ch) = if is_drip_zone {
                    // Dripping trails
                    ((60.0 + flicker * 20.0) as u8, 0, (80.0 + flicker * 20.0) as u8, '░')
                } else if flash_phase {
                    if dist_norm < 0.4 { (240, 200, 255, '█') }
                    else if dist_norm < 0.7 { (200, 100, 255, '█') }
                    else { (160, 60, 200, '▓') }
                } else if fireball_phase {
                    let p = (exp.frame - 6) as f32 / 16.0;
                    if dist_norm < 0.3 { (200, (50.0 * (1.0 - p)) as u8, 200, '█') }
                    else if dist_norm < 0.5 { ((150.0 + p * 20.0) as u8, 0, (200.0 - p * 40.0) as u8, '▓') }
                    else if dist_norm < 0.7 { ((120.0 - p * 30.0) as u8, 0, (160.0 - p * 40.0) as u8, '▒') }
                    else { ((80.0 - p * 20.0) as u8, 0, (120.0 - p * 30.0) as u8, '░') }
                } else if cooling_phase {
                    let p = (exp.frame - 22) as f32 / 23.0;
                    if dist_norm < 0.15 {
                        let pulse = if (exp.frame / 3) % 2 == 0 { 200 } else { 120 };
                        (pulse, 0, (200.0 - p * 40.0) as u8, '☠')
                    } else if dist_norm < 0.4 {
                        ((80.0 + flicker * 30.0 - p * 20.0) as u8, 0, (120.0 - p * 30.0) as u8, '▓')
                    } else if dist_norm < 0.7 {
                        ((60.0 - p * 15.0) as u8, 0, (80.0 - p * 20.0) as u8, '▒')
                    } else {
                        ((40.0 - p * 10.0) as u8, (10.0 * (1.0 - p)) as u8, (60.0 - p * 20.0) as u8, '░')
                    }
                } else {
                    let p = (exp.frame - 45) as f32 / 15.0;
                    let ch = if dist_norm > 0.5 { '░' } else { '▒' };
                    ((40.0 - p * 20.0) as u8, (20.0 - p * 10.0) as u8, (50.0 - p * 25.0) as u8, ch)
                };

                buf[(px, py)].set_char(ch).set_fg(Color::Rgb(r, g, b));
            }
        }
    }
}
