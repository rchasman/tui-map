use crate::hash::hash3;
use crate::map::GlobeViewport;
use super::{ExplosionRender, fast_pseudo_angle};
use ratatui::{buffer::Buffer, layout::Rect, style::Color};

/// Smooth hermite interpolation — no sudden jumps between phases
#[inline(always)]
fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Life: a goddess blessing the earth — golden light descends from above,
/// where it touches land flowers bloom, vines crawl outward, leaves unfurl.
/// Warm gold core → emerald canopy → trailing tendrils of ivy and petals.
pub fn render(exp: &ExplosionRender, x: u16, y: u16, area: Rect, global_frame: u64, buf: &mut Buffer, globe: Option<&GlobeViewport>) {
    if exp.frame >= exp.weapon_type.max_frames() || exp.radius == 0 { return; }
    // Keep the arrival brisk, sustain the garden, then let it settle gradually.
    let frame = if exp.frame < 38 { exp.frame as f32 }
        else if exp.frame < 168 { 38.0 + (exp.frame as f32 - 38.0) * 34.0 / 130.0 }
        else { 72.0 + (exp.frame as f32 - 168.0) * 18.0 / 42.0 };
    // Slow, graceful expansion — smooth cubic ease-out
    let t = frame / 90.0; // normalized to full duration
    let progress = if t < 0.2 {
        smoothstep(t / 0.2) // Gentle arrival
    } else if t < 0.6 {
        1.0 + smoothstep((t - 0.2) / 0.4) * 0.35 // Slow continued growth
    } else {
        1.35
    };
    let max_r = exp.radius as f32 * progress;

    // Tall divine column of light, wider botanical spread at the base
    let pillar_height = (max_r * 2.5) as i16;
    let canopy_width = max_r * 1.4;

    // Phase boundaries — stretched for languid pacing
    let descend_phase = frame < 16.0;  // Golden light descends slowly
    let bloom_phase = frame < 38.0;    // Flowers + vines unfurl gradually
    let flourish_phase = frame < 72.0; // Long, peaceful full canopy
    // frame >= 72: very slow fade, leaves settle gently

    let radius_i16 = (exp.radius as f32 * 1.5) as i16;
    let pillar_h_f32 = pillar_height.max(1) as f32;

    let dy_min = (-pillar_height).max(area.y as i16 - y as i16);
    let dy_max = (pillar_height / 5).max(3).min((area.y + area.height - 1) as i16 - y as i16);
    let dx_lo = (-radius_i16).max(area.x as i16 - x as i16);
    let dx_hi = radius_i16.min((area.x + area.width - 1) as i16 - x as i16);

    // Very slow breathing — deep, meditative rhythm
    let breath = ((global_frame as f32 * 0.05).sin() * 0.1 + 0.9).max(0.0);
    // Gentle shimmer — slow sparkle drift, not frantic
    let shimmer = (global_frame as f32 * 0.15).sin() * 0.5 + 0.5;

    for dy in dy_min..=dy_max {
        let py = (y as i16 + dy) as u16;
        let dy_f32 = dy as f32;
        let height_ratio = -dy_f32 / pillar_h_f32; // 0 at center, 1 at top

        for dx in dx_lo..=dx_hi {
            let dx_f32 = dx as f32;
            let dist_sq = dx_f32 * dx_f32 + dy_f32 * dy_f32;

            // Slow organic turbulence — vines shift glacially
            let angle = fast_pseudo_angle(dx_f32, dy_f32);
            let vine_turb = (angle * 7.0 + global_frame as f32 * 0.018).sin() * 0.3
                + (angle * 13.0 - global_frame as f32 * 0.011).cos() * 0.15;
            let leaf_seed = hash3(dx as u64, dy as u64, 0);
            let leaf_turb = ((leaf_seed & 0xFF) as f32 / 255.0 - 0.5) * 0.6;

            // Shape: narrow divine pillar above, wide botanical canopy at ground level
            let is_above = dy < 0;
            let (in_effect, dist_norm) = if is_above && height_ratio > 0.15 {
                let pillar_w = canopy_width * (0.15 + vine_turb * 0.08 + (1.0 - height_ratio) * 0.25);
                let in_pillar = dx_f32.abs() <= pillar_w;
                let d = if pillar_w > 0.0 { dx_f32.abs() / pillar_w } else { 2.0 };
                (in_pillar, d.min(1.0))
            } else {
                let growth_factor = 1.0 + vine_turb * 0.5 + leaf_turb * 0.4;
                let eff_w = canopy_width * growth_factor;
                let eff_w_sq = eff_w * eff_w;
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

            let seed = hash3(px as u64, py as u64, 0);
            let flicker = ((seed & 0xFF) as f32) / 255.0;

            // Detail roll changes slowly — stable botanical pattern
            let detail_seed = hash3(px as u64 ^ 0xF10A, py as u64 ^ 0xBEAD, 0);
            let detail_roll = (detail_seed & 0xFF) as f32 / 255.0;

            let (r, g, b, ch) = if is_above && height_ratio > 0.15 {
                // === DIVINE PILLAR (above) ===
                if descend_phase {
                    // Light descends slowly from above — smoothstep arrival
                    let arrival = smoothstep(frame / 16.0);
                    let lit = height_ratio < arrival;
                    if !lit { continue; }
                    // Fade in gently based on how recently the light arrived here
                    let local_age = smoothstep((arrival - height_ratio).min(0.3) / 0.3);
                    ((255.0 * local_age) as u8, (250.0 * local_age) as u8,
                     (200.0 * local_age) as u8,
                     if dist_norm < 0.3 { '█' } else if dist_norm < 0.6 { '▓' } else { '░' })
                } else if bloom_phase {
                    let p = smoothstep((frame - 16.0) / 22.0);
                    let sparkle = if detail_roll > 0.88 && shimmer > 0.6 { 40.0 } else { 0.0 };
                    if dist_norm < 0.3 {
                        (255u8, (248.0f32 + sparkle * 0.1).min(255.0) as u8,
                         (175.0f32 + sparkle + p * 10.0).min(255.0) as u8,
                         if detail_roll > 0.94 { '✦' } else { '█' })
                    } else {
                        ((235.0 * breath) as u8, (205.0 * breath) as u8,
                         (95.0 + flicker * 30.0) as u8, '░')
                    }
                } else if flourish_phase {
                    // Pillar dims very gradually, sparkles drift
                    let p = smoothstep((frame - 38.0) / 34.0);
                    let fade = (1.0 - p * 0.5).max(0.0);
                    if detail_roll > 0.92 && shimmer > 0.5 {
                        ((250.0 * fade) as u8, (240.0 * fade) as u8, (195.0 * fade) as u8, '✦')
                    } else {
                        ((195.0 * fade * breath) as u8, (175.0 * fade * breath) as u8,
                         (75.0 * fade) as u8, '░')
                    }
                } else {
                    let p = smoothstep((frame - 72.0) / 18.0);
                    let fade = (1.0 - p).max(0.0);
                    ((115.0 * fade) as u8, (95.0 * fade) as u8, (38.0 * fade) as u8, '░')
                }
            } else {
                // === BOTANICAL CANOPY (ground level) ===
                if descend_phase {
                    // First touch — golden warmth seeds outward gently
                    let touch = smoothstep(frame / 16.0);
                    let visible = dist_norm < touch * 0.5;
                    if !visible { continue; }
                    let local_bright = smoothstep(1.0 - dist_norm / (touch * 0.5).max(0.01));
                    ((255.0 * local_bright) as u8, (245.0 * local_bright) as u8,
                     (180.0 * local_bright) as u8,
                     if dist_norm < 0.1 { '█' } else if dist_norm < 0.25 { '▓' } else { '░' })
                } else if bloom_phase {
                    // Growth unfurls slowly — smooth expanding front
                    let p = smoothstep((frame - 16.0) / 22.0);
                    let growth_front = p * 1.1;
                    let behind_front = dist_norm < growth_front;
                    if !behind_front && dist_norm > growth_front + 0.12 { continue; }
                    let at_front = (dist_norm - growth_front).abs() < 0.12;

                    // Smooth fade-in for newly grown areas
                    let local_age = if behind_front {
                        smoothstep(((growth_front - dist_norm) / 0.3).min(1.0))
                    } else {
                        smoothstep(1.0 - (dist_norm - growth_front) / 0.12)
                    };

                    if dist_norm < 0.12 && local_age > 0.8 {
                        // Sacred center — pulsing flower, slow pulse
                        let symbol_bright = (breath * 255.0) as u8;
                        (255, symbol_bright, 140, '❀')
                    } else if dist_norm < 0.25 && local_age > 0.5 {
                        // Dense inner garden
                        if detail_roll > 0.7 {
                            ((180.0 * local_age) as u8, (255.0 * local_age) as u8,
                             (100.0 * local_age) as u8,
                             if detail_roll > 0.88 { '❀' } else { '✿' })
                        } else {
                            ((20.0 * local_age) as u8, (200.0 * local_age * breath).min(255.0) as u8,
                             (40.0 * local_age) as u8, '█')
                        }
                    } else if at_front {
                        // Growth front — softly bright, not explosive
                        let front_bright = local_age * breath;
                        ((80.0 * front_bright) as u8, (200.0 * front_bright) as u8,
                         (30.0 * front_bright) as u8,
                         if detail_roll > 0.6 { '♣' } else { '▓' })
                    } else if dist_norm < 0.55 && local_age > 0.3 {
                        // Mid canopy — leaves and stems
                        let gv = (155.0 * local_age * breath + flicker * 25.0).min(255.0) as u8;
                        if detail_roll > 0.82 {
                            (25, gv, (45.0 * local_age) as u8, '♠')
                        } else {
                            ((12.0 * local_age) as u8, gv, (28.0 * local_age) as u8, '▓')
                        }
                    } else if local_age > 0.1 {
                        // Outer tendrils creeping out
                        let gv = (85.0 * local_age * breath + flicker * 15.0) as u8;
                        ((6.0 * local_age) as u8, gv, (12.0 * local_age) as u8, '░')
                    } else {
                        continue;
                    }
                } else if flourish_phase {
                    // Long, peaceful full garden — very slow sway
                    let p = smoothstep((frame - 38.0) / 34.0);
                    let sway = (global_frame as f32 * 0.04 + dx_f32 * 0.2).sin() * 0.06;

                    if dist_norm < 0.1 {
                        // Sacred heart — slow golden glow
                        let glow = (1.0 - p * 0.2).max(0.0);
                        ((215.0 * glow) as u8, (250.0 * glow * breath) as u8,
                         (115.0 * glow) as u8, '❀')
                    } else if dist_norm < 0.3 {
                        // Dense garden — occasional golden petals drift lazily
                        if detail_roll > 0.88 && shimmer > 0.5 {
                            let petal_bright = breath * (1.0 - p * 0.3);
                            ((195.0 * petal_bright) as u8, (175.0 * petal_bright) as u8,
                             (55.0 * petal_bright) as u8, '✿')
                        } else {
                            let gv = (175.0 * breath + sway * 120.0 + flicker * 20.0).clamp(0.0, 255.0) as u8;
                            ((18.0 + flicker * 12.0) as u8, gv,
                             (32.0 + flicker * 10.0) as u8, '█')
                        }
                    } else if dist_norm < 0.6 {
                        // Lush mid-zone — leafy canopy with scattered wildflowers
                        let gv = (135.0 * breath + sway * 100.0 + flicker * 20.0).clamp(0.0, 255.0) as u8;
                        if detail_roll > 0.92 {
                            ((130.0 + flicker * 30.0) as u8, (195.0 + flicker * 20.0).min(255.0) as u8,
                             (170.0 + flicker * 30.0) as u8, '❀')
                        } else if detail_roll > 0.77 {
                            ((8.0 + flicker * 10.0) as u8, gv, (18.0 + flicker * 8.0) as u8, '♣')
                        } else {
                            ((8.0 + flicker * 10.0) as u8, gv, (18.0 + flicker * 8.0) as u8, '▓')
                        }
                    } else if dist_norm < 0.85 {
                        // Outer vines — sparse trailing growth
                        let vine_fade = smoothstep(1.0 - (dist_norm - 0.6) / 0.25);
                        let gv = (75.0 * vine_fade * breath + flicker * 12.0) as u8;
                        ((4.0 * vine_fade) as u8, gv, (8.0 * vine_fade) as u8,
                         if detail_roll > 0.7 { '░' } else { '·' })
                    } else {
                        // Very edge — barely visible moss
                        let edge_fade = smoothstep(1.0 - (dist_norm - 0.85) / 0.15);
                        (0, (28.0 * edge_fade + flicker * 6.0) as u8, (4.0 * edge_fade) as u8, '·')
                    }
                } else {
                    // Very slow fade — leaves settle gently, golden afterglow lingers
                    let p = smoothstep((frame - 72.0) / 18.0);
                    let fade = (1.0 - p).max(0.0);
                    if dist_norm < 0.3 && detail_roll > 0.82 {
                        // Last petals drifting down slowly
                        let drift = (global_frame as f32 * 0.1 + dx_f32 * 0.5).sin() * 0.25;
                        let petal_fade = fade * (0.5 + drift.abs());
                        ((170.0 * petal_fade) as u8, (135.0 * petal_fade) as u8,
                         (38.0 * petal_fade) as u8,
                         if detail_roll > 0.93 { '✿' } else { '·' })
                    } else if dist_norm < 0.5 {
                        ((8.0 * fade) as u8, (55.0 * fade * breath) as u8, (12.0 * fade) as u8, '░')
                    } else {
                        (0, (22.0 * fade) as u8, (4.0 * fade) as u8, '·')
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
