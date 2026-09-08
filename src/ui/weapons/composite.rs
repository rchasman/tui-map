//! Resolve effect contributions once per pass, independent of source order.
use super::{accents, reactions, render_body, ExplosionRender};
use crate::{
    app::WeaponType,
    interactions::Interactions,
    map::{globe::lonlat_to_vec3, Projection},
};
use ratatui::{buffer::Buffer, layout::Rect, style::Color};

#[derive(Clone, Copy, Default)]
struct Contribution {
    rgb: [u32; 3],
    bits: u32,
    glyph: Option<(u32, char)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::Explosion, map::Viewport};

    #[test]
    fn reversing_explosions_preserves_blend_and_viewport_border() {
        let area = Rect::new(5, 3, 50, 24);
        let projection = Projection::Mercator(Viewport::new(0.0, 0.0, 6.0, 100, 96));
        let mut explosions: Vec<_> = [WeaponType::Nuke, WeaponType::Emp, WeaponType::Life]
            .into_iter()
            .map(|weapon_type| ExplosionRender {
                x: 25,
                y: 14,
                frame: 15,
                radius: 8,
                weapon_type,
                lon: 0.0,
                lat: 0.0,
                radius_km: 400.0,
            })
            .collect();
        let mut world = Interactions::default();
        for e in &explosions {
            world.launch(&Explosion {
                lon: e.lon,
                lat: e.lat,
                frame: e.frame,
                radius_km: e.radius_km,
                weapon_type: e.weapon_type,
            });
        }
        let mut a = Buffer::empty(Rect::new(0, 0, 65, 32));
        let mut b = a.clone();
        let mut compositor = Compositor::default();
        compositor.render(&explosions, &world, &projection, area, 15, &mut a);
        explosions.reverse();
        world.fields.reverse();
        compositor.render(&explosions, &world, &projection, area, 15, &mut b);
        assert_eq!(a, b);
        assert!(a.content.iter().any(|c| c.symbol() != " "));
        for y in 0..32 {
            for x in 0..65 {
                if !area.contains((x, y).into()) {
                    assert_eq!(a[(x, y)].symbol(), " ");
                }
            }
        }
    }

    #[test]
    fn crossed_light_unions_dots_and_caps_brightness() {
        let area = Rect::new(0, 0, 1, 1);
        let mut compositor = Compositor::default();
        compositor.begin(area);
        compositor.scratch[(0, 0)]
            .set_char('⠁')
            .set_fg(Color::Rgb(200, 100, 0));
        compositor.collect(area);
        compositor.scratch[(0, 0)]
            .set_char('⠈')
            .set_fg(Color::Rgb(0, 100, 200));
        compositor.collect(area);
        let mut buf = Buffer::empty(area);
        compositor.resolve(area, &mut buf, true);
        assert_eq!(buf[(0, 0)].symbol(), "⠉");
        assert_eq!(buf[(0, 0)].fg, Color::Rgb(200, 200, 200));
        compositor.scratch[(0, 0)]
            .set_char('⠁')
            .set_fg(Color::Rgb(200, 100, 0));
        compositor.collect(area);
        buf.reset();
        compositor.resolve(area, &mut buf, true);
        assert_eq!(buf[(0, 0)].fg, Color::Rgb(255, 191, 127));
    }
}

pub struct Compositor {
    scratch: Buffer,
    cells: Vec<Contribution>,
}

impl Default for Compositor {
    fn default() -> Self {
        Self {
            scratch: Buffer::empty(Rect::default()),
            cells: Vec::new(),
        }
    }
}

impl Compositor {
    fn begin(&mut self, area: Rect) {
        self.scratch.resize(area);
        self.scratch.reset();
        self.cells.resize(
            area.width as usize * area.height as usize,
            Contribution::default(),
        );
        self.cells.fill(Contribution::default());
    }

    fn collect(&mut self, area: Rect) {
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                let idx = self.scratch.index_of(x, y);
                let cell = &self.scratch.content[idx];
                let ch = cell.symbol().chars().next().unwrap_or(' ');
                if ch == ' ' {
                    continue;
                }
                let Color::Rgb(r, g, b) = cell.fg else {
                    continue;
                };
                let entry = &mut self.cells[idx];
                entry.rgb[0] += r as u32;
                entry.rgb[1] += g as u32;
                entry.rgb[2] += b as u32;
                if ('\u{2800}'..='\u{28ff}').contains(&ch) {
                    entry.bits |= ch as u32 - 0x2800;
                } else {
                    // Brightest body wins shape; ties use a stable character key.
                    let candidate = (r as u32 + g as u32 + b as u32, ch);
                    if entry.glyph.is_none_or(|old| candidate > old) {
                        entry.glyph = Some(candidate);
                    }
                }
            }
        }
        self.scratch.reset();
    }

    fn resolve(&self, area: Rect, buf: &mut Buffer, light: bool) {
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                let entry = self.cells[self.scratch.index_of(x, y)];
                if entry.rgb == [0; 3] {
                    continue;
                }
                let cell = &mut buf[(x, y)];
                let previous = cell.symbol().chars().next().unwrap_or(' ');
                let mut rgb = entry.rgb;
                if light {
                    if let Color::Rgb(r, g, b) = cell.fg {
                        for (channel, old) in rgb.iter_mut().zip([r, g, b]) {
                            *channel += old as u32;
                        }
                    }
                }
                let ch = if let Some((_, glyph)) = entry.glyph {
                    glyph
                } else if light && previous != ' ' && !('\u{2800}'..='\u{28ff}').contains(&previous)
                {
                    previous
                } else {
                    let old_bits = if light && ('\u{2800}'..='\u{28ff}').contains(&previous) {
                        previous as u32 - 0x2800
                    } else {
                        0
                    };
                    char::from_u32(0x2800 + (old_bits | entry.bits)).unwrap()
                };
                // Hue-preserving exposure cap keeps many overlaps from washing white.
                let peak = *rgb.iter().max().unwrap();
                let scale = 255.0 / peak.max(255) as f32;
                cell.set_char(ch).set_fg(Color::Rgb(
                    (rgb[0] as f32 * scale) as u8,
                    (rgb[1] as f32 * scale) as u8,
                    (rgb[2] as f32 * scale) as u8,
                ));
            }
        }
    }

    pub fn render(
        &mut self,
        explosions: &[ExplosionRender],
        world: &Interactions,
        projection: &Projection,
        area: Rect,
        frame: u64,
        buf: &mut Buffer,
    ) {
        if explosions.is_empty() && !world.active() {
            return;
        }
        let area = area.intersection(buf.area);
        if area.is_empty() {
            return;
        }
        let globe = match projection {
            Projection::Globe(g) => Some(g),
            _ => None,
        };
        self.begin(buf.area);
        for exp in explosions {
            render_body(
                exp,
                area.x + exp.x,
                area.y + exp.y,
                area,
                frame,
                &mut self.scratch,
                globe,
            );
            if matches!(exp.weapon_type, WeaponType::Nuke | WeaponType::Chem)
                && world.fields.iter().any(|f| f.weapon == WeaponType::Water)
            {
                // The thermal body also yields where water has already arrived.
                for y in area.y..area.bottom() {
                    for x in area.x..area.right() {
                        if self.scratch[(x, y)].symbol() == " " {
                            continue;
                        }
                        if let Some((lon, lat)) =
                            projection.unproject((x - area.x) as i32 * 2, (y - area.y) as i32 * 4)
                        {
                            let point = lonlat_to_vec3(lon, lat);
                            if world
                                .fields
                                .iter()
                                .any(|f| f.weapon == WeaponType::Water && f.contains(point))
                            {
                                self.scratch[(x, y)].reset();
                            }
                        }
                    }
                }
            }
            self.collect(area);
        }
        self.resolve(area, buf, false);

        self.cells.fill(Contribution::default());
        for field in &world.fields {
            if field.progress() >= 1.0 {
                continue;
            }
            reactions::wave(field, projection, area, &mut self.scratch);
            self.collect(area);
        }
        for exp in explosions {
            accents::render(
                exp,
                area.x + exp.x,
                area.y + exp.y,
                area,
                &mut self.scratch,
                globe,
                true,
            );
            self.collect(area);
        }
        // Group residue into one source: thousands of neighboring steam particles
        // should form a soft veil, not accumulate thousands of exposures.
        for reaction in world.reactions.values() {
            reactions::residue(reaction, projection, area, &mut self.scratch);
        }
        self.collect(area);
        self.resolve(area, buf, true);
    }
}
