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


/// A raised fountain: a narrow rising core separates into ballistic droplets.
/// Global time keeps the flow moving when the nozzle is refreshed by held input.
pub(super) fn render_spout(
    exp: &ExplosionRender, x: u16, y: u16, area: Rect, global_frame: u64, buf: &mut Buffer,
) {
    if exp.radius == 0 || exp.frame >= 60 { return; }
    let clip = area.intersection(buf.area);
    let height = (exp.radius as f32 * 0.9).clamp(4.0, 18.0);
    let width = height * 0.7;
    let age = exp.frame as f32;
    for strand in 0..17u64 {
        let direction = (strand as f32 / 16.0 - 0.5) * 2.0;
        let speed = 0.85 + rand_simple(exp.seed.wrapping_add(strand * 97)) as f32 * 0.15;
        for step in 0..100 {
            let t = step as f32 / 100.0;
            let travel = t * 36.0;
            if travel > age + 18.0 || travel < (age - 24.0).max(0.0) { continue; }
            // Keep the rising column connected; break the falling spray into beads.
            let pulse = (t * 38.0 - global_frame as f32 * 0.65 + strand as f32 * 1.7).sin();
            if t > 0.45 && pulse < 0.25 { continue; }
            let dx = direction * width * t.powi(2)
                + (t * 15.0 - global_frame as f32 * 0.17).sin() * t * 0.25;
            let dy = -height * speed * 4.0 * t * (1.0 - t);
            let dot_x = ((x as f32 + 0.5 + dx) * 2.0).floor() as i32;
            let dot_y = ((y as f32 + 0.5 + dy) * 4.0).floor() as i32;
            if dot_x < 0 || dot_y < 0 { continue; }
            let px = (dot_x / 2) as u16;
            let py = (dot_y / 4) as u16;
            if !clip.contains((px, py).into()) { continue; }
            let bit = [[1,2,4,64],[8,16,32,128]][(dot_x % 2) as usize][(dot_y % 4) as usize];
            let cell = &mut buf[(px, py)];
            let old = cell.symbol().chars().next().unwrap_or(' ') as u32;
            let bits = if (0x2800..=0x28ff).contains(&old) { old - 0x2800 } else { 0 };
            cell.set_char(char::from_u32(0x2800 + (bits | bit)).unwrap());
            let bright = direction.abs() < 0.3 || pulse > 0.75;
            cell.set_fg(if bright { Color::Rgb(165, 235, 255) } else { Color::Rgb(35, 155, 235) });
        }
    }
}


#[cfg(test)]
mod spout_tests {
    use super::*;
    use crate::app::WeaponType;

    #[test]
    fn jet_rises_above_its_anchor_animates_and_stops() {
        let area = Rect::new(3, 2, 50, 30);
        let mut exp = ExplosionRender { seed: 7, x: 25, y: 24, frame: 18,
            radius: 12, weapon_type: WeaponType::Water, lon: 0.0, lat: 0.0, radius_km: 500.0 };
        let mut a = Buffer::empty(Rect::new(0, 0, 60, 35));
        render_spout(&exp, 25, 24, area, 30, &mut a);
        assert!((12..18).any(|y| (20..30).any(|x| a[(x,y)].symbol() != " ")));
        let mut b = Buffer::empty(a.area);
        render_spout(&exp, 25, 24, area, 34, &mut b);
        assert_ne!(a, b);
        for y in 0..35 { for x in 0..60 {
            if !area.contains((x,y).into()) { assert_eq!(a[(x,y)].symbol(), " "); }
        }}
        exp.frame = 60;
        let mut ended = Buffer::empty(a.area);
        render_spout(&exp, 25, 24, area, 90, &mut ended);
        assert!(ended.content.iter().all(|c| c.symbol() == " "));
    }
}
