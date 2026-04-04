use crate::hash::{hash2, hash3};
use crate::map::GlobeViewport;
use super::{ExplosionRender, fast_pseudo_angle};
use ratatui::{buffer::Buffer, layout::Rect, style::Color};

/// Life: a goddess blessing the earth — golden light descends from above,
/// where it touches land flowers bloom, vines crawl outward, leaves unfurl.
/// Warm gold core → emerald canopy → trailing tendrils of ivy and petals.
pub fn render(exp: &ExplosionRender, x: u16, y: u16, area: Rect, global_frame: u64, buf: &mut Buffer, globe: Option<&GlobeViewport>) {
    let progress = if exp.frame < 12 {
        (exp.frame as f32 / 12.0).powf(0.4) // Sudden divine light
    } else if exp.frame < 30 {
        1.0 + ((exp.frame - 12) as f32 / 18.0) * 0.4
    } else {
        1.4
    };
    let max_r = exp.radius as f32 * progress;

    // Tall divine column of light, wider botanical spread at the base
    let pillar_height = (max_r * 2.5) as i16; // Tall beam from above
    let canopy_width = max_r * 1.4; // Wide botanical spread

    let descend_phase = exp.frame < 8;   // Golden light descends
    let bloom_phase = exp.frame < 20;    // Flowers + vines erupt
    let flourish_phase = exp.frame < 38; // Full botanical canopy, petals drift
    // frame >= 38: gentle fade, leaves settle

    let radius_i16 = (exp.radius as f32 * 1.5) as i16;
    let pillar_h_f32 = pillar_height.max(1) as f32;
    let frame_seed = global_frame + exp.frame as u64;

    // Extends far above (divine light) and slightly below (roots)
    let dy_min = (-pillar_height).max(-(y as i16));
    let dy_max = (pillar_height / 5).max(3).min((area.y + area.height - 1) as i16 - y as i16);
    let dx_lo = (-radius_i16).max(-(x as i16));
    let dx_hi = radius_i16.min((area.x + area.width - 1) as i16 - x as i16);

    // Slow breathing pulse — the earth inhaling life
    let breath = ((global_frame as f32 * 0.1).sin() * 0.12 + 0.88).max(0.0);
    // Faster shimmer for divine sparkle
    let shimmer = (global_frame as f32 * 0.4).sin() * 0.5 + 0.5;

    for dy in dy_min..=dy_max {
        let py = (y as i16 + dy) as u16;
        let dy_sq = dy * dy;
        let dy_f32 = dy as f32;
        let height_ratio = -dy_f32 / pillar_h_f32; // 0 at center, 1 at top

        for dx in dx_lo..=dx_hi {
            let dist_sq = (dx * dx + dy_sq) as f32;
            let dx_f32 = dx as f32;

            // Organic turbulence — vines and tendrils, not smooth circles
            let angle = fast_pseudo_angle(dx_f32, dy_f32);
            let vine_seed = hash2((angle * 500.0) as u64, global_frame / 8);
            let vine_turb = ((vine_seed & 0xFF) as f32 / 255.0 - 0.5) * 0.9;
            let leaf_seed = hash3(dx as u64, dy as u64, frame_seed);
            let leaf_turb = ((leaf_seed & 0xFF) as f32 / 255.0 - 0.5) * 0.6;

            // Shape: narrow divine pillar above, wide botanical canopy at ground level
            let is_above = dy < 0;
            let (in_effect, dist_norm) = if is_above && height_ratio > 0.15 {
                // Divine light column — narrow beam widening slightly downward
                let pillar_w = canopy_width * (0.15 + vine_turb * 0.08 + (1.0 - height_ratio) * 0.25);
                let in_pillar = (dx_f32.abs()) <= pillar_w;
                let d = if pillar_w > 0.0 { dx_f32.abs() / pillar_w } else { 2.0 };
                (in_pillar, d.min(1.0))
            } else {
                // Ground-level botanical explosion — irregular canopy
                let growth_factor = 1.0 + vine_turb * 0.5 + leaf_turb * 0.4;
                let eff_w = canopy_width * growth_factor;
                let eff_w_sq = eff_w * eff_w;
                // Below center: slight root spread
                let below_falloff = if dy > 0 {
                    let below_ratio = dy_f32 / (pillar_height as f32 / 5.0).max(1.0);
                    (1.0 - below_ratio * below_ratio).max(0.0)
                } else {
                    1.0
                };
                let in_canopy = dist_sq <= eff_w_sq * below_falloff;
                let d = if eff_w > 0.0 { (dist_sq / eff_w_sq).sqrt() } else { 2.0 };
                (in_canopy, d.min(1.0))
            };

            if !in_effect { continue; }

            let px = (x as i16 + dx) as u16;

            if let Some(g) = globe {
                let bx = (px as i32 - area.x as i32) * 2;
                let by = (py as i32 - area.y as i32) * 4;
                if g.pixel_to_sphere_point(bx, by).is_none() { continue; }
            }

            let seed = hash3(px as u64, py as u64, frame_seed);
            let flicker = ((seed & 0xFF) as f32) / 255.0;

            // Petal / vine detail — scattered botanical symbols
            let detail_seed = hash3(px as u64 ^ 0xF10A, py as u64 ^ 0xBEAD, global_frame / 4);
            let detail_roll = (detail_seed & 0xFF) as f32 / 255.0;

            let (r, g, b, ch) = if is_above && height_ratio > 0.15 {
                // === DIVINE PILLAR (above) ===
                // Golden-white light streaming down
                if descend_phase {
                    let arrival = (exp.frame as f32 / 8.0).min(1.0);
                    let lit = height_ratio < arrival; // Light descends from top
                    if !lit { continue; }
                    if dist_norm < 0.3 { (255, 255, 220, '█') }
                    else if dist_norm < 0.6 { (255, 240, 160, '▓') }
                    else { (220, 200, 100, '░') }
                } else if bloom_phase {
                    // Pillar brightens, golden sparkles
                    let sparkle = if detail_roll > 0.85 && shimmer > 0.6 { 40.0 } else { 0.0 };
                    if dist_norm < 0.3 {
                        (255u8, (250.0f32 + sparkle * 0.1).min(255.0) as u8,
                         (180.0f32 + sparkle).min(255.0) as u8,
                         if detail_roll > 0.92 { '✦' } else { '█' })
                    } else {
                        ((240.0 * breath) as u8, (210.0 * breath) as u8,
                         (100.0 + flicker * 40.0) as u8, '░')
                    }
                } else if flourish_phase {
                    // Pillar dims slowly, sparkles linger
                    let p = (exp.frame - 20) as f32 / 18.0;
                    let fade = (1.0 - p * 0.6).max(0.0);
                    if detail_roll > 0.9 && shimmer > 0.5 {
                        ((255.0 * fade) as u8, (245.0 * fade) as u8, (200.0 * fade) as u8, '✦')
                    } else {
                        ((200.0 * fade * breath) as u8, (180.0 * fade * breath) as u8,
                         (80.0 * fade) as u8, '░')
                    }
                } else {
                    let p = (exp.frame - 38) as f32 / 12.0;
                    let fade = (1.0 - p).max(0.0);
                    ((120.0 * fade) as u8, (100.0 * fade) as u8, (40.0 * fade) as u8, '░')
                }
            } else {
                // === BOTANICAL CANOPY (ground level) ===
                if descend_phase {
                    // First touch: golden impact ring, seeds of green
                    if dist_norm < 0.2 { (255, 255, 200, '█') }
                    else if dist_norm < 0.4 { (200, 240, 100, '▓') }
                    else if dist_norm < 0.7 { (80, 180, 40, '▒') }
                    else { (30, 100, 20, '░') }
                } else if bloom_phase {
                    // Flowers erupt, vines snake outward
                    let p = (exp.frame - 8) as f32 / 12.0;
                    let growth_front = p * 1.2; // Expanding growth edge
                    let at_front = (dist_norm - growth_front).abs() < 0.15;

                    if dist_norm < 0.12 {
                        // Sacred center — pulsing flower
                        let symbol_pulse = if (exp.frame / 4) % 3 == 0 { 255 } else { 210 };
                        (255, symbol_pulse, 140, '❀')
                    } else if dist_norm < 0.25 {
                        // Dense inner garden — flowers and rich green
                        if detail_roll > 0.7 {
                            ((180.0 + flicker * 40.0) as u8, 255, (100.0 + flicker * 40.0) as u8,
                             if detail_roll > 0.88 { '❀' } else { '✿' })
                        } else {
                            ((20.0 + flicker * 30.0) as u8, (200.0 * breath + flicker * 40.0).min(255.0) as u8,
                             (40.0 + flicker * 20.0) as u8, '█')
                        }
                    } else if at_front {
                        // Growth front — bright bursting tendrils
                        ((100.0 + flicker * 60.0) as u8, (220.0 + flicker * 35.0).min(255.0) as u8,
                         (30.0 + flicker * 30.0) as u8,
                         if detail_roll > 0.6 { '♣' } else { '▓' })
                    } else if dist_norm < 0.55 {
                        // Mid canopy — leaves and stems
                        let gv = (160.0 * breath + flicker * 40.0).min(255.0) as u8;
                        if detail_roll > 0.8 {
                            (30, gv, (50.0 + flicker * 20.0) as u8, '♠')
                        } else {
                            ((15.0 + flicker * 15.0) as u8, gv, (30.0 + flicker * 15.0) as u8, '▓')
                        }
                    } else {
                        // Outer tendrils — sparse vines reaching out
                        let gv = (90.0 * breath + flicker * 30.0) as u8;
                        ((8.0 + flicker * 10.0) as u8, gv, (15.0 + flicker * 10.0) as u8, '░')
                    }
                } else if flourish_phase {
                    // Full garden — drifting petals, swaying canopy
                    let p = (exp.frame - 20) as f32 / 18.0;
                    let sway = (global_frame as f32 * 0.08 + dx_f32 * 0.3).sin() * 0.08;

                    if dist_norm < 0.1 {
                        // Sacred heart still glowing
                        let glow = (1.0 - p * 0.3).max(0.0);
                        ((220.0 * glow) as u8, (255.0 * glow * breath) as u8,
                         (120.0 * glow) as u8, '❀')
                    } else if dist_norm < 0.3 {
                        // Dense garden with occasional golden petals drifting
                        if detail_roll > 0.85 && shimmer > 0.4 {
                            // Drifting petal — warm gold
                            ((200.0 + flicker * 40.0).min(255.0) as u8, (180.0 + flicker * 30.0) as u8,
                             (60.0 + flicker * 40.0) as u8, '✿')
                        } else {
                            let gv = (180.0 * breath + sway * 200.0 + flicker * 30.0).clamp(0.0, 255.0) as u8;
                            ((20.0 + flicker * 20.0) as u8, gv,
                             (35.0 + flicker * 15.0) as u8, '█')
                        }
                    } else if dist_norm < 0.6 {
                        // Lush mid-zone — leafy with scattered wildflowers
                        let gv = (140.0 * breath + sway * 150.0 + flicker * 30.0).clamp(0.0, 255.0) as u8;
                        if detail_roll > 0.9 {
                            ((140.0 + flicker * 40.0) as u8, (200.0 + flicker * 30.0).min(255.0) as u8,
                             (180.0 + flicker * 40.0) as u8, '❀')
                        } else if detail_roll > 0.75 {
                            ((10.0 + flicker * 15.0) as u8, gv, (20.0 + flicker * 10.0) as u8, '♣')
                        } else {
                            ((10.0 + flicker * 15.0) as u8, gv, (20.0 + flicker * 10.0) as u8, '▓')
                        }
                    } else if dist_norm < 0.85 {
                        // Outer vines — sparse trailing growth
                        let vine_fade = (1.0 - (dist_norm - 0.6) / 0.25).max(0.0);
                        let gv = (80.0 * vine_fade * breath + flicker * 20.0) as u8;
                        ((5.0 * vine_fade + flicker * 8.0) as u8, gv,
                         (10.0 * vine_fade + flicker * 5.0) as u8,
                         if detail_roll > 0.7 { '░' } else { '·' })
                    } else {
                        // Very edge — barely visible moss/lichen
                        let edge_fade = (1.0 - (dist_norm - 0.85) / 0.15).max(0.0);
                        (0, (30.0 * edge_fade + flicker * 10.0) as u8, (5.0 * edge_fade) as u8, '·')
                    }
                } else {
                    // Gentle fade — leaves settle, golden afterglow
                    let p = (exp.frame - 38) as f32 / 12.0;
                    let fade = (1.0 - p).max(0.0);
                    if dist_norm < 0.3 && detail_roll > 0.8 {
                        // Last few petals drifting down
                        let drift_y = (global_frame as f32 * 0.2 + dx_f32).sin() * 0.3;
                        let petal_fade = fade * (0.5 + drift_y.abs());
                        ((180.0 * petal_fade) as u8, (140.0 * petal_fade) as u8,
                         (40.0 * petal_fade) as u8,
                         if detail_roll > 0.92 { '✿' } else { '·' })
                    } else if dist_norm < 0.5 {
                        ((10.0 * fade) as u8, (60.0 * fade * breath) as u8, (15.0 * fade) as u8, '░')
                    } else {
                        (0, (25.0 * fade) as u8, (5.0 * fade) as u8, '·')
                    }
                }
            };

            // Merge: keep brighter of overlapping
            {
                let cell = &buf[(px, py)];
                if matches!(cell.symbol(), "▓" | "▒" | "░" | "█" | "❀" | "✿" | "♣" | "♠" | "✦" | "·") {
                    if let Color::Rgb(_, eg, _) = cell.fg {
                        if eg >= g { continue; }
                    }
                }
            }
            buf[(px, py)].set_char(ch).set_fg(Color::Rgb(r, g, b));
        }
    }
}
