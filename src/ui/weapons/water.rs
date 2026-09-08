use super::ExplosionRender;
use crate::{hash::rand_simple, map::GlobeViewport};
use ratatui::{buffer::Buffer, layout::Rect, style::Color};

/// A quick splash settles into flowing pools, broken foam and travelling ripples.
/// Sample Braille dots so the shoreline stays soft without opaque block bands.
pub fn render(exp: &ExplosionRender, x: u16, y: u16, area: Rect, _global_frame: u64, buf: &mut Buffer, globe: Option<&GlobeViewport>) {
    if exp.radius == 0 || exp.frame >= exp.weapon_type.max_frames() { return; }
    let age = exp.frame as f32;
    let t = age / exp.weapon_type.max_frames() as f32;
    let fade = (1.0 - ((t - 0.6) / 0.4).clamp(0.0, 1.0)).powi(2);
    let spread = 0.12 + 1.5 * (1.0 - (-age / 16.0).exp());
    let phase = rand_simple(exp.lon.to_bits() ^ exp.lat.to_bits()) as f32 * 6.2831855;
    let r = exp.radius as f32;
    let clip = area.intersection(buf.area);
    let left = (x as i32 - (r * 1.9).ceil() as i32).max(clip.x as i32);
    let right = (x as i32 + (r * 1.9).ceil() as i32 + 1).min(clip.right() as i32);
    let top = (y as i32 - (r * 0.8).ceil() as i32).max(clip.y as i32);
    let bottom = (y as i32 + (r * 0.8).ceil() as i32 + 1).min(clip.bottom() as i32);
    for py in top..bottom {
        for px in left..right {
            let mut bits = 0u32;
            let mut light = 0.0f32;
            let mut foam = 0.0f32;
            for sy in 0..4 {
                for sx in 0..2 {
                    if globe.is_some_and(|g| g.pixel_to_sphere_point(px*2+sx-area.x as i32*2, py*4+sy-area.y as i32*4).is_none()) { continue; }
                    let dx = (px as f32 + (sx as f32+0.5)*0.5-x as f32-0.5)/r;
                    let dy = (py as f32 + (sy as f32+0.5)*0.25-y as f32-0.5)*2.0/r;
                    let angle = dy.atan2(dx);
                    let edge = spread * (1.0 + 0.07*(angle*3.0+phase).sin() + 0.045*(angle*7.0-phase+age*0.018).sin());
                    let d = (dx*dx + (dy/0.72).powi(2)).sqrt();
                    if d > edge { continue; }
                    let eddy = super::organic::noise(dx*3.0-age*0.018, dy*4.0+age*0.011, exp.lon.to_bits() ^ exp.lat.to_bits()) - 0.5;
                    let flow = (dx*5.0+dy*3.0-age*0.055+phase).sin()
                        + (dx*2.7-dy*6.0+age*0.035).sin()*0.5 + eddy*1.2;
                    let wave = (d*19.0-age*0.22+flow*0.9+eddy*2.0).sin();
                    let crest = ((wave-0.55)/0.45).max(0.0).powi(2);
                    let rim = (1.0-(edge-d)/0.08).clamp(0.0,1.0) * (angle*11.0+flow).sin().max(0.0);
                    let caustic = ((flow.abs()-0.5)*0.8).max(0.0);
                    let sparkle = crest.max(rim);
                    let value = (0.12 + caustic*0.24 + sparkle*0.7) * fade;
                    // Leave gaps in the water: geography remains readable underneath.
                    if value < 0.07 || (crest < 0.08 && rim < 0.15 && flow < 0.2) { continue; }
                    bits |= [[1,2,4,64],[8,16,32,128]][sx as usize][sy as usize];
                    light = light.max(value);
                    foam = foam.max(sparkle * fade);
                }
            }
            if bits != 0 {
                buf[(px as u16,py as u16)].set_char(char::from_u32(0x2800+bits).unwrap())
                    .set_fg(Color::Rgb((22.0*light+140.0*foam) as u8,(145.0*light+90.0*foam).min(255.0) as u8,(235.0*light+20.0*foam).min(255.0) as u8));
            }
        }
    }
}
