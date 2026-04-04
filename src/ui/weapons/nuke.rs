use crate::hash::{hash2, hash3};
use crate::map::GlobeViewport;
use super::{ExplosionRender, fast_pseudo_angle};
use ratatui::{buffer::Buffer, layout::Rect, style::Color};

/// Nuke: mushroom cloud rising UPWARD — white → yellow → orange → red → smoke
pub fn render(exp: &ExplosionRender, x: u16, y: u16, area: Rect, global_frame: u64, buf: &mut Buffer, globe: Option<&GlobeViewport>) {
    let progress = if exp.frame < 20 {
        (exp.frame as f32 / 20.0).powf(0.7)
    } else if exp.frame < 40 {
        1.0 + ((exp.frame - 20) as f32 / 20.0) * 0.3
    } else {
        1.3
    };
    let max_r = exp.radius as f32 * progress;
    let cap_height = (max_r * (2.0 + (exp.frame as f32 / 60.0) * 1.2)) as i16;
    let cap_width = max_r;

    let flash_phase = exp.frame < 8;
    let fireball_phase = exp.frame < 25;
    let cooling_phase = exp.frame < 45;

    let radius_i16 = exp.radius as i16;
    let cap_height_f32 = cap_height as f32;
    let frame_seed_component = global_frame + exp.frame as u64;

    // Clamp vertical loop to viewport
    let dy_min = (-cap_height).max(-(y as i16));
    let dy_max = (-1i16).min((area.y + area.height - 1) as i16 - y as i16);

    for dy in dy_min..=dy_max {
        let py = (y as i16 + dy) as u16;

        let dy_sq = dy * dy;
        let dy_f32 = dy as f32;
        let height_ratio = -dy_f32 / cap_height_f32;

        let (base_width, height_mult, large_mult, fine_mult) = if height_ratio < 0.2 {
            (0.5, 0.4, 0.0, 0.5)
        } else if height_ratio < 0.5 {
            (0.9, 1.5, 0.7, 0.3)
        } else if height_ratio < 0.75 {
            (1.4, 2.0, 1.2, 0.4)
        } else {
            (1.9, 2.5, 2.0, 0.8)
        };

        let height_component = if height_ratio < 0.2 {
            height_ratio * height_mult
        } else if height_ratio < 0.5 {
            (height_ratio - 0.2) * height_mult
        } else if height_ratio < 0.75 {
            (height_ratio - 0.5) * height_mult
        } else {
            (height_ratio - 0.75) * height_mult
        };

        let dx_lo = (-radius_i16).max(-(x as i16));
        let dx_hi = radius_i16.min((area.x + area.width - 1) as i16 - x as i16);

        for dx in dx_lo..=dx_hi {
            let dist_sq = (dx * dx + dy_sq) as f32;
            let dx_f32 = dx as f32;
            let large_turb_seed = hash2((fast_pseudo_angle(dx_f32, dy_f32) * 1000.0) as u64, global_frame / 5);
            let large_turbulence = ((large_turb_seed & 0xFF) as f32 / 255.0 - 0.5) * 0.6;
            let fine_turb_seed = hash3(dx as u64, dy as u64, frame_seed_component);
            let fine_turbulence = ((fine_turb_seed & 0xFF) as f32 / 255.0 - 0.5) * 0.4;

            let height_factor = base_width + height_component +
                               large_turbulence * large_mult +
                               fine_turbulence * fine_mult;
            let effective_width_sq = (cap_width * height_factor) * (cap_width * height_factor);

            if dist_sq <= effective_width_sq {
                let px = (x as i16 + dx) as u16;

                if let Some(g) = globe {
                    let bx = (px as i32 - area.x as i32) * 2;
                    let by = (py as i32 - area.y as i32) * 4;
                    if g.pixel_to_sphere_point(bx, by).is_none() { continue; }
                }

                let radial_dist = (dist_sq / effective_width_sq).sqrt();
                let vertical_factor = (-dy as f32) / cap_height as f32;
                let dist_norm = (radial_dist * 0.5 + vertical_factor * 0.5).min(1.0);

                let seed = hash3(px as u64, py as u64, global_frame + exp.frame as u64);
                let flicker = ((seed & 0xFF) as f32) / 255.0;

                let (r, g, b, ch) = if flash_phase {
                    if dist_norm < 0.4 { (255, 255, 255, '█') }
                    else if dist_norm < 0.7 { (255, 250, 220, '█') }
                    else { (255, 240, 150, '▓') }
                } else if fireball_phase {
                    let phase_progress = (exp.frame - 8) as f32 / 17.0;
                    let core_threshold = 0.3 - (phase_progress * 0.15);
                    if dist_norm < core_threshold { (255, 255, 250, '█') }
                    else if dist_norm < 0.4 {
                        (255, (250.0 - phase_progress * 70.0) as u8, (120.0 - phase_progress * 100.0) as u8, '▓')
                    } else if dist_norm < 0.6 {
                        (255, (180.0 - phase_progress * 100.0) as u8, (20.0 * (1.0 - phase_progress)) as u8, '▓')
                    } else if dist_norm < 0.8 { (255, 80, 0, '▒') }
                    else { (200, 40, 0, '░') }
                } else if cooling_phase {
                    let cooling_progress = (exp.frame - 25) as f32 / 20.0;
                    if dist_norm < 0.15 {
                        let pulse = if (exp.frame / 3) % 2 == 0 { 60 } else { 20 };
                        (255, pulse, 30, '☢')
                    } else if dist_norm < 0.4 {
                        ((220.0 - cooling_progress * 80.0 - flicker * 40.0) as u8, (60.0 - cooling_progress * 20.0) as u8, 0, '▓')
                    } else if dist_norm < 0.7 {
                        ((160.0 - cooling_progress * 50.0) as u8, (40.0 - cooling_progress * 20.0) as u8, 0, '▒')
                    } else {
                        ((100.0 - cooling_progress * 20.0) as u8, (20.0 - cooling_progress * 10.0) as u8, 0, '░')
                    }
                } else {
                    let final_progress = (exp.frame - 45) as f32 / 15.0;
                    let ch = if dist_norm > 0.5 { '░' } else { '▒' };
                    ((80.0 - final_progress * 30.0) as u8, (15.0 - final_progress * 10.0) as u8, 0, ch)
                };

                buf[(px, py)].set_char(ch).set_fg(Color::Rgb(r, g, b));
            }
        }
    }
}
