use crate::app::WeaponType;
use crate::hash::{hash2, hash3};
use crate::map::Projection;
use crate::map::globe::lonlat_to_vec3;
use super::{GasCloudRender, fast_pseudo_angle, fast_acos_approx};
use ratatui::{buffer::Buffer, layout::Rect, style::Color};

/// Gas cloud: slow billowing noxious fog — neon green (Bio) or purple (Chem).
/// On globe: uses geographic distance (great-circle) so the cloud conforms to the sphere.
/// On mercator: uses screen-space distance (correct for flat projection).
pub fn render_merged(clouds: &[GasCloudRender], density_buf: &mut [(f32, f32)], area: Rect, global_frame: u64, buf: &mut Buffer, projection: &Projection) {
    if clouds.is_empty() { return; }
    let w = area.width as usize;
    let h = area.height as usize;
    if w == 0 || h == 0 { return; }

    let globe = match projection {
        Projection::Globe(g) => Some(g),
        _ => None,
    };
    let time_slow = global_frame / 180;
    let time_glacial = global_frame / 300;

    for cloud in clouds {
        let cx = area.x + cloud.x;
        let cy = area.y + cloud.y;
        let r = cloud.radius as i16;
        if r < 2 { continue; }

        let intensity_norm = (cloud.intensity as f32 / 2000.0).min(1.0);
        let intensity_scale = 0.3 + intensity_norm * 0.7;

        let cloud_id = hash2(
            (cloud.lon * 1000.0).to_bits(),
            (cloud.lat * 1000.0).to_bits(),
        );

        let radius_rad = cloud.radius_km / 6371.0;

        let cloud_vec3 = globe.map(|_| lonlat_to_vec3(cloud.lon, cloud.lat));

        const N_LOBES: usize = 12;
        let mut lobe_factor = [0.0f32; N_LOBES];
        for i in 0..N_LOBES {
            let seed_a = hash3(i as u64, cloud_id, time_slow);
            let seed_b = hash3(i as u64, cloud_id, time_slow.wrapping_add(1));
            let na = (seed_a & 0xFF) as f32 / 255.0;
            let nb = (seed_b & 0xFF) as f32 / 255.0;

            let t_frac = (global_frame % 180) as f32 / 180.0;
            let t_smooth = (1.0 - (t_frac * std::f32::consts::PI).cos()) * 0.5;
            let n = na * (1.0 - t_smooth) + nb * t_smooth;

            lobe_factor[i] = (0.55 + n * 0.4) * intensity_scale;
        }

        let scan_r = if globe.is_some() { r + r / 4 } else { r };

        // Clamp loop bounds to viewport — avoids iterating O(r²) pixels
        // when cloud extends far beyond screen (e.g. low-zoom cloud viewed at high zoom)
        let dy_min = (-scan_r).max(area.y as i16 - cy as i16);
        let dy_max = scan_r.min((area.y + area.height - 1) as i16 - cy as i16);
        let dx_min = (-scan_r).max(area.x as i16 - cx as i16);
        let dx_max = scan_r.min((area.x + area.width - 1) as i16 - cx as i16);

        // Texture scale: ~8 features across cloud diameter regardless of zoom.
        // Without this, pixel-level hash noise makes high-zoom edges look like static.
        let tex_scale = (r as f32).max(1.0) / 8.0;

        for dy in dy_min..=dy_max {
            let py = (cy as i16 + dy) as u16;

            for dx in dx_min..=dx_max {
                let px = (cx as i16 + dx) as u16;

                let angle_norm = fast_pseudo_angle(dx as f32, dy as f32) / 4.0;
                let lobe_pos = angle_norm * N_LOBES as f32;
                let lobe_idx = (lobe_pos as usize) % N_LOBES;
                let lobe_next = (lobe_idx + 1) % N_LOBES;
                let lobe_frac = lobe_pos - lobe_pos.floor();
                let t = lobe_frac * lobe_frac * (3.0 - 2.0 * lobe_frac);
                let lobe_mult = lobe_factor[lobe_idx] * (1.0 - t) + lobe_factor[lobe_next] * t;

                let dist_norm = if let Some(g) = globe {
                    let bx = (px as i32 - area.x as i32) * 2;
                    let by = (py as i32 - area.y as i32) * 4;
                    let point = match g.pixel_to_sphere_point(bx, by) {
                        Some(p) => p,
                        None => continue,
                    };
                    let cv = cloud_vec3.unwrap();
                    let dot = cv.dot(point).clamp(-1.0, 1.0);
                    let effective_r = radius_rad * lobe_mult as f64;
                    if effective_r < 0.0001 { continue; }
                    (fast_acos_approx(dot) / effective_r) as f32
                } else {
                    let dist = { let (dxf, dyf) = (dx as f32, dy as f32); (dxf * dxf + dyf * dyf).sqrt() };
                    let effective_r = r as f32 * lobe_mult;
                    if effective_r < 1.0 { continue; }
                    dist / effective_r
                };

                if dist_norm > 1.0 { continue; }

                // Texture scaled to cloud radius — consistent geographic-scale features
                // across zoom levels. Without this, edges dissolve into pixel noise at high zoom.
                let tex_x = (dx as f32 / tex_scale) as i64;
                let tex_y = (dy as f32 / tex_scale) as i64;
                let tex_key = hash3(
                    tex_x as u64 ^ cloud_id,
                    tex_y as u64,
                    time_glacial,
                );
                let texture = ((tex_key & 0xFF) as f32 / 255.0 - 0.5) * 0.15;

                let edge_factor = ((dist_norm - 0.6) / 0.4).max(0.0);
                let adjusted_dist = dist_norm + texture * edge_factor * 2.0;
                if adjusted_dist > 1.0 { continue; }

                let density = (1.0 - adjusted_dist.max(0.0)).powi(2) * intensity_norm;

                let idx = (py - area.y) as usize * w + (px - area.x) as usize;
                match cloud.weapon_type {
                    WeaponType::Bio => density_buf[idx].0 += density,
                    WeaponType::Chem => density_buf[idx].1 += density,
                    _ => {}
                }
            }
        }
    }

    // Render from accumulated density
    for row in 0..h {
        for col in 0..w {
            let idx = row * w + col;
            let (bio_d, chem_d) = density_buf[idx];
            if bio_d < 0.05 && chem_d < 0.05 { continue; }

            let px = area.x + col as u16;
            let py = area.y + row as u16;

            let shade_seed = hash2(px as u64 ^ 0xBEEF, py as u64 ^ 0xCAFE);
            let shade = ((shade_seed & 0x1F) as f32) / 31.0;

            // Dominant type determines color; combined density determines intensity
            let (r, g, b, ch) = if bio_d >= chem_d {
                bio_density_color(bio_d, shade)
            } else {
                chem_density_color(chem_d, shade)
            };

            buf[(px, py)].set_char(ch).set_fg(Color::Rgb(r, g, b));
        }
    }
}

/// Map accumulated bio density to color — overlap produces super-dense visuals
pub fn bio_density_color(d: f32, shade: f32) -> (u8, u8, u8, char) {
    if d > 1.0 {
        let extra = (d - 1.0).min(1.0);
        ((15.0 + extra * 25.0 + shade * 10.0) as u8,
         (220.0 + extra * 35.0).min(255.0) as u8,
         (40.0 + extra * 20.0 + shade * 10.0) as u8, '█')
    } else if d > 0.5 {
        ((10.0 + shade * 15.0) as u8, (180.0 + shade * 40.0) as u8, (30.0 + shade * 15.0) as u8, '▓')
    } else if d > 0.2 {
        (0, (100.0 + shade * 40.0) as u8, (15.0 + shade * 10.0) as u8, '▒')
    } else {
        (0, (45.0 + shade * 25.0) as u8, (5.0 + shade * 5.0) as u8, '░')
    }
}

/// Map accumulated chem density to color
pub fn chem_density_color(d: f32, shade: f32) -> (u8, u8, u8, char) {
    if d > 1.0 {
        let extra = (d - 1.0).min(1.0);
        ((160.0 + extra * 50.0).min(255.0) as u8,
         (10.0 + extra * 15.0) as u8,
         (200.0 + extra * 55.0).min(255.0) as u8, '█')
    } else if d > 0.5 {
        ((120.0 + shade * 40.0) as u8, (5.0 + shade * 10.0) as u8, (160.0 + shade * 40.0) as u8, '▓')
    } else if d > 0.2 {
        ((65.0 + shade * 30.0) as u8, 0, (100.0 + shade * 30.0) as u8, '▒')
    } else {
        ((25.0 + shade * 15.0) as u8, 0, (45.0 + shade * 20.0) as u8, '░')
    }
}
