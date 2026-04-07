use crate::hash::{hash2, hash3};
use crate::map::GlobeViewport;
use super::{ExplosionRender, fast_pseudo_angle};
use ratatui::{buffer::Buffer, layout::Rect, style::Color};

/// Bio: low creeping fog — wide but stays low, neon green palette, irregular tendrils
pub fn render(exp: &ExplosionRender, x: u16, y: u16, area: Rect, global_frame: u64, buf: &mut Buffer, globe: Option<&GlobeViewport>) {
    let progress = if exp.frame < 20 {
        (exp.frame as f32 / 20.0).powf(0.5) // Faster initial spread
    } else if exp.frame < 40 {
        1.0 + ((exp.frame - 20) as f32 / 20.0) * 0.4
    } else {
        1.4
    };
    let max_r = exp.radius as f32 * progress;

    // Low fog: 40% of nuke height, 1.8× width
    let cap_height = (max_r * 0.4 * (1.5 + (exp.frame as f32 / 60.0) * 0.5)) as i16;
    let cap_width = max_r * 1.8;

    let flash_phase = exp.frame < 5;
    let spread_phase = exp.frame < 20;
    let creep_phase = exp.frame < 45;

    let radius_i16 = (exp.radius as f32 * 1.8) as i16;
    let cap_height_f32 = cap_height.max(1) as f32;
    let frame_seed_component = global_frame + exp.frame as u64;

    // Fog extends both slightly above AND below cursor (hugs ground)
    let dy_min = -cap_height;
    let dy_max = (cap_height / 3).max(2); // Small drip below

    // Clamp loop bounds to viewport
    let dy_lo = dy_min.max(-(y as i16));
    let dy_hi = dy_max.min((area.y + area.height - 1) as i16 - y as i16);
    let dx_lo = (-radius_i16).max(-(x as i16));
    let dx_hi = radius_i16.min((area.x + area.width - 1) as i16 - x as i16);

    for dy in dy_lo..=dy_hi {
        let py = (y as i16 + dy) as u16;

        let dy_f32 = dy as f32;
        let height_ratio = dy_f32.abs() / cap_height_f32;

        for dx in dx_lo..=dx_hi {
            let dx_f32 = dx as f32;
            let dist_sq = dx_f32 * dx_f32 + dy_f32 * dy_f32;

            // Higher fine turbulence for irregular tendrils
            let large_turb_seed = hash2((fast_pseudo_angle(dx_f32, dy_f32) * 800.0) as u64, global_frame / 4);
            let large_turbulence = ((large_turb_seed & 0xFF) as f32 / 255.0 - 0.5) * 0.8;
            let fine_turb_seed = hash3(dx as u64, dy as u64, frame_seed_component);
            let fine_turbulence = ((fine_turb_seed & 0xFF) as f32 / 255.0 - 0.5) * 0.7; // High fine turbulence

            // Width-dominant shape (wide, low)
            let height_factor = 1.0 + large_turbulence * 0.6 + fine_turbulence * 0.5;
            let effective_width_sq = (cap_width * height_factor) * (cap_width * height_factor);

            // Vertical falloff: fog thins rapidly with height
            let vert_falloff = 1.0 - (height_ratio * height_ratio);
            let in_fog = dist_sq <= effective_width_sq * vert_falloff.max(0.0);

            if in_fog {
                let px = (x as i16 + dx) as u16;

                if let Some(g) = globe {
                    let bx = (px as i32 - area.x as i32) * 2;
                    let by = (py as i32 - area.y as i32) * 4;
                    if g.pixel_to_sphere_point(bx, by).is_none() { continue; }
                }

                let eff_w_sq = (cap_width * height_factor).max(1.0);
                let radial_dist = (dist_sq / (eff_w_sq * eff_w_sq)).sqrt();
                let dist_norm = (radial_dist * 0.6 + height_ratio * 0.4).min(1.0);

                let seed = hash3(px as u64, py as u64, global_frame + exp.frame as u64);
                let flicker = ((seed & 0xFF) as f32) / 255.0;

                let (r, g, b, ch) = if flash_phase {
                    if dist_norm < 0.4 { (200, 255, 200, '█') }
                    else if dist_norm < 0.7 { (100, 255, 80, '█') }
                    else { (50, 200, 40, '▓') }
                } else if spread_phase {
                    let p = (exp.frame - 5) as f32 / 15.0;
                    if dist_norm < 0.3 { (0, 255, 50, '█') }
                    else if dist_norm < 0.5 { ((40.0 * p) as u8, (255.0 - p * 55.0) as u8, (50.0 - p * 30.0) as u8, '▓') }
                    else if dist_norm < 0.7 { (80, (200.0 - p * 60.0) as u8, 0, '▒') }
                    else { (40, (120.0 - p * 40.0) as u8, 0, '░') }
                } else if creep_phase {
                    let p = (exp.frame - 20) as f32 / 25.0;
                    if dist_norm < 0.15 {
                        let pulse = if (exp.frame / 4) % 2 == 0 { 255 } else { 180 };
                        (0, pulse, 30, '☣')
                    } else if dist_norm < 0.4 {
                        ((40.0 + flicker * 20.0) as u8, (180.0 - p * 60.0) as u8, (20.0 - p * 10.0) as u8, '▓')
                    } else if dist_norm < 0.7 {
                        ((50.0 - p * 15.0) as u8, (100.0 - p * 30.0) as u8, (10.0 - p * 5.0) as u8, '▒')
                    } else {
                        ((40.0 - p * 10.0) as u8, (60.0 - p * 20.0) as u8, (10.0 - p * 5.0) as u8, '░')
                    }
                } else {
                    let p = (exp.frame - 45) as f32 / 15.0;
                    let ch = if dist_norm > 0.5 { '░' } else { '▒' };
                    ((30.0 - p * 15.0) as u8, (40.0 - p * 20.0) as u8, (20.0 - p * 10.0) as u8, ch)
                };

                // Merge with existing bio content: keep brighter of overlapping blasts/clouds
                {
                    let cell = &buf[(px, py)];
                    if matches!(cell.symbol(), "▓" | "▒" | "░" | "█" | "☣") {
                        if let Color::Rgb(_, eg, _) = cell.fg {
                            if eg >= g { continue; }
                        }
                    }
                }
                buf[(px, py)].set_char(ch).set_fg(Color::Rgb(r, g, b));
            }
        }
    }
}
