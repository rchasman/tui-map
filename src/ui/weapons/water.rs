use crate::hash::{hash2, hash3};
use crate::map::GlobeViewport;
use super::{ExplosionRender, fast_pseudo_angle};
use ratatui::{buffer::Buffer, layout::Rect, style::Color};

/// Water: concentric ripple rings expanding outward — deep blue center fading to cyan mist
pub fn render(exp: &ExplosionRender, x: u16, y: u16, area: Rect, global_frame: u64, buf: &mut Buffer, globe: Option<&GlobeViewport>) {
    let progress = if exp.frame < 15 {
        (exp.frame as f32 / 15.0).powf(0.6)
    } else if exp.frame < 30 {
        1.0 + ((exp.frame - 15) as f32 / 15.0) * 0.3
    } else {
        1.3
    };
    let max_r = exp.radius as f32 * progress;

    // Water is wide and low — spreading outward like a shockwave
    let cap_height = (max_r * 0.5) as i16;
    let cap_width = max_r * 1.6;

    let splash_phase = exp.frame < 8;
    let ripple_phase = exp.frame < 25;
    let settling_phase = exp.frame < 38;

    let radius_i16 = (exp.radius as f32 * 1.6) as i16;
    let cap_height_f32 = cap_height.max(1) as f32;
    let frame_seed_component = global_frame + exp.frame as u64;

    let dy_min = (-cap_height).max(-(y as i16));
    let dy_max = (cap_height / 2).min((area.y + area.height - 1) as i16 - y as i16);
    let dx_lo = (-radius_i16).max(-(x as i16));
    let dx_hi = radius_i16.min((area.x + area.width - 1) as i16 - x as i16);

    for dy in dy_min..=dy_max {
        let py = (y as i16 + dy) as u16;
        let dy_sq = dy * dy;
        let dy_f32 = dy as f32;
        let height_ratio = dy_f32.abs() / cap_height_f32;

        for dx in dx_lo..=dx_hi {
            let dist_sq = (dx * dx + dy_sq) as f32;
            let dx_f32 = dx as f32;

            let large_turb_seed = hash2((fast_pseudo_angle(dx_f32, dy_f32) * 600.0) as u64, global_frame / 3);
            let large_turbulence = ((large_turb_seed & 0xFF) as f32 / 255.0 - 0.5) * 0.5;
            let fine_turb_seed = hash3(dx as u64, dy as u64, frame_seed_component);
            let fine_turbulence = ((fine_turb_seed & 0xFF) as f32 / 255.0 - 0.5) * 0.4;

            let height_factor = 1.0 + large_turbulence * 0.4 + fine_turbulence * 0.3;
            let effective_width_sq = (cap_width * height_factor) * (cap_width * height_factor);
            let vert_falloff = 1.0 - (height_ratio * height_ratio);
            let in_splash = dist_sq <= effective_width_sq * vert_falloff.max(0.0);

            if in_splash {
                let px = (x as i16 + dx) as u16;

                if let Some(g) = globe {
                    let bx = (px as i32 - area.x as i32) * 2;
                    let by = (py as i32 - area.y as i32) * 4;
                    if g.pixel_to_sphere_point(bx, by).is_none() { continue; }
                }

                let eff_w_sq = (cap_width * height_factor).max(1.0);
                let radial_dist = (dist_sq / (eff_w_sq * eff_w_sq)).sqrt();
                let dist_norm = (radial_dist * 0.7 + height_ratio * 0.3).min(1.0);

                let seed = hash3(px as u64, py as u64, global_frame + exp.frame as u64);
                let flicker = ((seed & 0xFF) as f32) / 255.0;

                // Concentric ripple bands
                let ripple_freq = 4.0 + exp.frame as f32 * 0.15;
                let ripple = ((dist_norm * ripple_freq * std::f32::consts::PI).sin() * 0.5 + 0.5)
                    * (1.0 - dist_norm); // Fade ripples at edges

                let (r, g, b, ch) = if splash_phase {
                    if dist_norm < 0.3 { (200, 230, 255, '█') }
                    else if dist_norm < 0.6 { (100, 180, 255, '█') }
                    else { (50, 140, 220, '▓') }
                } else if ripple_phase {
                    let p = (exp.frame - 8) as f32 / 17.0;
                    if dist_norm < 0.2 {
                        let pulse = if (exp.frame / 3) % 2 == 0 { 255 } else { 200 };
                        (30, 100, pulse, '≋')
                    } else if ripple > 0.6 {
                        // Bright ripple crests
                        ((80.0 + ripple * 60.0) as u8, (160.0 + ripple * 60.0) as u8, 255, '▓')
                    } else if dist_norm < 0.6 {
                        ((20.0 + p * 10.0) as u8, (100.0 + flicker * 30.0) as u8, (220.0 - p * 30.0) as u8, '▒')
                    } else {
                        ((10.0 + flicker * 15.0) as u8, (60.0 + flicker * 20.0) as u8, (160.0 - p * 40.0) as u8, '░')
                    }
                } else if settling_phase {
                    let p = (exp.frame - 25) as f32 / 13.0;
                    if ripple > 0.5 && dist_norm > 0.3 {
                        let fade = 1.0 - p;
                        ((40.0 * fade) as u8, (120.0 * fade + flicker * 20.0) as u8, (200.0 * fade) as u8, '▒')
                    } else {
                        let fade = 1.0 - p;
                        ((15.0 * fade) as u8, (60.0 * fade) as u8, (140.0 * fade) as u8, '░')
                    }
                } else {
                    let p = (exp.frame - 38) as f32 / 7.0;
                    let fade = (1.0 - p).max(0.0);
                    ((10.0 * fade) as u8, (40.0 * fade) as u8, (100.0 * fade) as u8, '░')
                };

                {
                    let cell = &buf[(px, py)];
                    if matches!(cell.symbol(), "▓" | "▒" | "░" | "█" | "≋") {
                        if let Color::Rgb(_, _, eb) = cell.fg {
                            if eb >= b { continue; }
                        }
                    }
                }
                buf[(px, py)].set_char(ch).set_fg(Color::Rgb(r, g, b));
            }
        }
    }
}
