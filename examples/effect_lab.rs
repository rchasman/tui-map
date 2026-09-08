//! Run `cargo run --release --example effect_lab` for a live interaction gallery.
//! `--export /tmp/effect-lab.html` writes a self-contained animated preview.
use crossterm::event::{self, Event, KeyCode};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Color,
    widgets::{Block, Borders, Widget},
};
use std::{io::Write, time::Duration};
use tui_map::{
    app::{Explosion, Fire, WeaponType},
    interactions::Interactions,
    map::{Projection, Viewport},
    ui::weapons::{composite::Compositor, gas_clouds, ExplosionRender, GasCloudRender},
};

const LABELS: [&str; 9] = [
    "Water + fire / steam",
    "EMP + gas / ionization",
    "Water + life / bloom",
    "Fire + life / char",
    "Bio + chem / mixed wisps",
    "Shockwave + cloud / displacement",
    "Nuke / fireball to mushroom cloud",
    "Water + water / crossing swells",
    "Water cascade / overlapping ripples",
];

struct Scene {
    kind: usize,
    frame: u64,
    world: Interactions,
    fires: Vec<Fire>,
    explosions: Vec<Explosion>,
    clouds: Vec<GasCloudRender>,
    compositor: Compositor,
    density: Vec<(f32, f32)>,
}

impl Scene {
    fn new(kind: usize) -> Self {
        let mut scene = Self {
            kind,
            frame: 0,
            world: Interactions::default(),
            fires: Vec::new(),
            explosions: Vec::new(),
            clouds: Vec::new(),
            compositor: Compositor::default(),
            density: Vec::new(),
        };
        if kind == 0 || kind == 3 {
            for row in -8..=8 {
                for col in -12..=12 {
                    if col * col + row * row > 144 {
                        continue;
                    }
                    scene.fires.push(Fire {
                        lon: col as f64 * 0.45,
                        lat: row as f64 * 0.45,
                        intensity: if col < 0 { 230 } else { 75 },
                        weapon_type: WeaponType::Nuke,
                    });
                }
            }
        }
        if matches!(kind, 1 | 4 | 5) {
            scene.clouds.push(GasCloudRender {
                lon: 0.0,
                lat: 0.0,
                radius_km: 1300.0,
                intensity: 2000,
                weapon_type: WeaponType::Bio,
            });
            if kind == 4 {
                scene.clouds.push(GasCloudRender {
                    lon: 3.0,
                    lat: 0.0,
                    radius_km: 1300.0,
                    intensity: 2000,
                    weapon_type: WeaponType::Chem,
                });
            }
        }
        scene
    }

    fn tick(&mut self) {
        let launch = match (self.kind, self.frame) {
            (0, 10) => Some((WeaponType::Water, -3.0)),
            (1, 10) => Some((WeaponType::Emp, -3.0)),
            (2, 0) => Some((WeaponType::Water, -2.0)),
            (2, 20) => Some((WeaponType::Life, 2.0)),
            (3, 10) => Some((WeaponType::Life, 0.0)),
            (5, 10) => Some((WeaponType::Nuke, -4.0)),
            (6, 10) => Some((WeaponType::Nuke, 0.0)),
            (7, 0) => Some((WeaponType::Water, -4.0)),
            (7, 8) => Some((WeaponType::Water, 4.0)),
            (8, 0) => Some((WeaponType::Water, -5.0)),
            (8, 22) => Some((WeaponType::Water, 5.0)),
            (8, 44) => Some((WeaponType::Water, 0.0)),
            _ => None,
        };
        if let Some((weapon_type, lon)) = launch {
            let explosion = Explosion {
                lon,
                lat: 0.0,
                frame: 0,
                radius_km: 550.0,
                weapon_type,
            };
            self.world.launch(&explosion);
            self.explosions.push(explosion);
        }
        self.world.update(&mut self.fires);
        self.explosions.retain_mut(|e| {
            e.frame += 1;
            e.frame < e.weapon_type.max_frames()
        });
        self.frame += 1;
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer) {
        let projection = Projection::Mercator(Viewport::new(
            0.0,
            if matches!(self.kind, 5 | 6) { 5.0 } else { 0.0 },
            10.0,
            area.width as usize * 2,
            area.height as usize * 4,
        ));
        // A quiet coordinate grid makes cleared space and moving fronts readable.
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                if (x - area.x) % 10 == 0 || (y - area.y) % 5 == 0 {
                    buf[(x, y)].set_char('·').set_fg(Color::Rgb(22, 37, 51));
                }
            }
        }
        for fire in &self.fires {
            if let Some((x, y)) = projection.project_point(fire.lon, fire.lat) {
                let (x, y) = (x / 2, y / 4);
                if x >= 0 && y >= 0 && x < area.width as i32 && y < area.height as i32 {
                    buf[(area.x + x as u16, area.y + y as u16)]
                        .set_char(if fire.intensity > 100 { '▓' } else { '░' })
                        .set_fg(Color::Rgb(fire.intensity, fire.intensity / 3, 12));
                }
            }
        }
        self.density
            .resize(area.width as usize * area.height as usize, (0.0, 0.0));
        gas_clouds::render_interacting(
            &self.clouds,
            &mut self.density,
            area,
            self.frame,
            buf,
            &projection,
            &self.world,
        );
        let explosions: Vec<_> = self
            .explosions
            .iter()
            .filter_map(|e| {
                let (x, y) = projection.project_point(e.lon, e.lat)?;
                if x < 0 || y < 0 {
                    return None;
                }
                Some(ExplosionRender {
                    x: (x / 2) as u16,
                    y: (y / 4) as u16,
                    frame: e.frame,
                    radius: (projection.deg_to_pixels(e.radius_km / 111.0) / 2.0).max(3.0) as u16,
                    lon: e.lon,
                    lat: e.lat,
                    radius_km: e.radius_km,
                    weapon_type: e.weapon_type,
                })
            })
            .collect();
        self.compositor
            .render(&explosions, &self.world, &projection, area, self.frame, buf);
    }
}

fn html_frame(buf: &Buffer) -> String {
    let mut output = String::new();
    for y in buf.area.y..buf.area.bottom() {
        let mut last = Color::Reset;
        for x in buf.area.x..buf.area.right() {
            let cell = &buf[(x, y)];
            let fg = match cell.fg {
                Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
                _ => Color::Rgb(30, 40, 50),
            };
            if fg != last {
                if last != Color::Reset {
                    output.push_str("</span>");
                }
                let (r, g, b) = match cell.fg {
                    Color::Rgb(r, g, b) => (r, g, b),
                    _ => (30, 40, 50),
                };
                output.push_str(&format!("<span style='color:#{r:02x}{g:02x}{b:02x}'>"));
                last = fg;
            }
            output.push_str(&cell.symbol().replace('&', "&amp;").replace('<', "&lt;"));
        }
        if last != Color::Reset {
            output.push_str("</span>");
        }
        output.push('\n');
    }
    output
}

fn export(path: &str) -> anyhow::Result<()> {
    let mut file = std::fs::File::create(path)?;
    file.write_all(br#"<!doctype html><meta charset="utf-8"><title>Effect interaction lab</title>
<style>body{background:#080e19;color:#c8d7e8;font:15px system-ui;margin:32px}h1{font-size:24px}button{background:#172438;color:#c8d7e8;border:1px solid #30425c;border-radius:6px;padding:10px;margin:0 8px 8px 0;cursor:pointer}button[aria-pressed=true]{border-color:#80d8ff;color:#80d8ff}pre{font:14px/1.1 'Menlo','DejaVu Sans Mono',monospace;white-space:pre;margin:20px 0;overflow:auto}input{width:480px;max-width:70vw}small{color:#8b9cb3}</style>
<h1>Effect interaction lab</h1><p>Nine scenes, including intersecting water waves, rendered by the terminal effect engine.</p><nav id="tabs"></nav><pre id="screen"></pre>
<button id="play">Pause</button><input id="time" type="range" min="0" max="59" value="0" aria-label="Animation frame"><small id="stamp"></small>
<p><small>Water consumes fire at contact; steam and pollen linger. Shockwaves displace fog temporarily. Water waves reinforce or cancel at crossings; opposing crests break into foam.</small></p><script>const labels="#)?;
    write!(file, "{:?};const scenes=[", LABELS)?;
    for kind in 0..LABELS.len() {
        if kind > 0 {
            write!(file, ",")?;
        }
        write!(file, "[")?;
        let mut scene = Scene::new(kind);
        for sample in 0..60 {
            for _ in 0..3 {
                scene.tick();
            }
            let mut buf = Buffer::empty(Rect::new(0, 0, 100, 36));
            scene.draw(buf.area, &mut buf);
            if sample > 0 {
                write!(file, ",")?;
            }
            write!(file, "{:?}", html_frame(&buf))?;
        }
        write!(file, "]")?;
    }
    file.write_all(br#"];let scene=0,frame=0,playing=!matchMedia('(prefers-reduced-motion: reduce)').matches;
const screen=document.getElementById('screen'),time=document.getElementById('time'),play=document.getElementById('play');
labels.forEach((name,i)=>{let b=document.createElement('button');b.textContent=name;b.onclick=()=>{scene=i;frame=0;draw()};document.getElementById('tabs').append(b)});
function draw(){screen.innerHTML=scenes[scene][frame];time.value=frame;play.textContent=playing?'Pause':'Play';document.getElementById('stamp').textContent=' Frame '+(frame*3+3);Array.from(document.querySelectorAll('nav button')).forEach((b,i)=>b.setAttribute('aria-pressed',String(i===scene)))}
play.onclick=()=>{playing=!playing;draw()};time.oninput=()=>{playing=false;frame=+time.value;draw()};setInterval(()=>{if(playing){frame=(frame+1)%60;draw()}},100);draw();</script>"#)?;
    eprintln!("Wrote {path}");
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).is_some_and(|a| a == "--export") {
        return export(
            args.get(2)
                .map(String::as_str)
                .unwrap_or("/tmp/effect-lab.html"),
        );
    }
    let mut terminal = ratatui::init();
    let result = (|| -> anyhow::Result<()> {
        let mut scene = Scene::new(0);
        let mut paused = false;
        loop {
            terminal.draw(|frame| {
                let area = frame.area();
                let block = Block::default().borders(Borders::ALL).title(format!(
                    " {} | 1–9: scene · Space: pause · R: restart · Q: quit ",
                    LABELS[scene.kind]
                ));
                let inner = block.inner(area);
                block.render(area, frame.buffer_mut());
                scene.draw(inner, frame.buffer_mut());
            })?;
            if event::poll(Duration::from_millis(33))? {
                if let Event::Key(key) = event::read()? {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        KeyCode::Char(' ') => paused = !paused,
                        KeyCode::Char('r') => scene = Scene::new(scene.kind),
                        KeyCode::Char(c @ '1'..='9') => {
                            scene = Scene::new(c as usize - '1' as usize)
                        }
                        _ => {}
                    }
                }
            }
            if !paused {
                scene.tick();
                if scene.frame >= 240 {
                    scene = Scene::new(scene.kind);
                }
            }
        }
        Ok(())
    })();
    ratatui::restore();
    result
}
