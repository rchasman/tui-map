//! Browser host for the same App and Ratatui renderer used by the terminal.
use crate::{
    app::{App, WeaponType},
    data,
    map::{Lod, MapRenderer, Projection},
    ui,
};
use ratatui::{
    backend::TestBackend,
    style::{Color, Modifier},
    Terminal,
};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct BrowserApp {
    app: App,
    terminal: Terminal<TestBackend>,
    width: u16,
    height: u16,
    paused: bool,
    help: bool,
    map_drag: bool,
}

#[wasm_bindgen]
impl BrowserApp {
    #[wasm_bindgen(constructor)]
    pub fn new(width: u16, height: u16) -> Result<BrowserApp, JsValue> {
        console_error_panic_hook::set_once();
        let (width, height) = (width.clamp(40, 240), height.clamp(16, 100));
        let mut app = App::new(
            width as usize,
            (height - ui::menu::layout(width).0) as usize,
        );
        app.frame = 30;
        app.map_renderer.settings.show_labels = false;
        let terminal = Terminal::new(TestBackend::new(width, height)).map_err(js_error)?;
        Ok(Self {
            app,
            terminal,
            width,
            height,
            paused: false,
            help: false,
            map_drag: false,
        })
    }

    pub fn load_layer(&mut self, kind: &str, lod: u8, bytes: Vec<u8>) -> Result<(), JsValue> {
        let lod = match lod {
            0 => Lod::Low,
            1 => Lod::Medium,
            _ => Lod::High,
        };
        data::load_geojson_bytes(&mut self.app.map_renderer, kind, lod, bytes).map_err(js_error)
    }

    pub fn finish_loading(&mut self) {
        if !self.app.map_renderer.has_data() {
            data::generate_simple_world(&mut self.app.map_renderer);
        }
        self.app.map_renderer.build_land_grid();
        self.app.map_renderer.build_spatial_indexes();
        self.app.map_renderer.invalidate_cache();
    }

    pub fn resize(&mut self, width: u16, height: u16) -> Result<(), JsValue> {
        let (width, height) = (width.clamp(40, 240), height.clamp(16, 100));
        self.width = width;
        self.height = height;
        self.app.resize(
            width as usize,
            (height - ui::menu::layout(width).0) as usize,
        );
        self.terminal.backend_mut().resize(width, height);
        self.terminal
            .resize(ratatui::layout::Rect::new(0, 0, width, height))
            .map_err(js_error)
    }

    pub fn tick(&mut self) {
        if !self.paused {
            self.app.update_explosions();
        }
    }

    pub fn pointer(&mut self, kind: &str, col: u16, row: u16) {
        let col = col.min(self.width.saturating_sub(1));
        let row = row.min(self.height.saturating_sub(1));
        let map_height = self.height - ui::menu::layout(self.width).0;
        if kind == "end" {
            self.map_drag = false;
            self.app.end_drag();
            return;
        }
        if self.help {
            if kind == "fire" {
                self.help = false;
            }
            return;
        }
        if row >= map_height {
            self.app.mouse_pos = None;
            if kind == "start" {
                self.map_drag = false;
                self.app.end_drag();
            }
            if kind == "fire" {
                if let Some(key) = ui::menu::hit(self.width, col, row - map_height) {
                    self.command(key);
                }
            }
            return;
        }
        if kind == "start" {
            self.map_drag = true;
        }
        if kind == "drag" && !self.map_drag {
            return;
        }
        self.app.set_mouse_pos(col, row);
        match kind {
            "start" => self.app.start_drag(col, row),
            "drag" => self.app.handle_drag(col, row),
            "end" => self.app.end_drag(),
            "fire" => self.app.launch_nuke(col, row),
            "in" => self.app.zoom_in_at(col, row),
            "out" => self.app.zoom_out_at(col, row),
            "leave" => self.app.mouse_pos = None,
            _ => {}
        }
    }

    pub fn command(&mut self, key: &str) {
        if self.help {
            if matches!(key, "?" | "Escape") {
                self.help = false;
            }
            return;
        }
        match key {
            "?" => self.help = true,
            "Escape" => self.paused = !self.paused,
            "1" => self.app.select_weapon(WeaponType::Nuke),
            "2" => self.app.select_weapon(WeaponType::Bio),
            "3" => self.app.select_weapon(WeaponType::Emp),
            "4" => self.app.select_weapon(WeaponType::Chem),
            "5" => self.app.select_weapon(WeaponType::Water),
            "6" => self.app.select_weapon(WeaponType::Life),
            "g" => self.app.toggle_projection(),
            "b" => self.app.map_renderer.toggle_borders(),
            "s" => self.app.map_renderer.toggle_states(),
            "c" => self.app.map_renderer.toggle_cities(),
            "y" => self.app.map_renderer.toggle_counties(),
            "L" => self.app.map_renderer.toggle_labels(),
            "p" => self.app.map_renderer.toggle_population(),
            "h" | "ArrowLeft" => self.app.pan(-4, 0),
            "l" | "ArrowRight" => self.app.pan(4, 0),
            "k" | "ArrowUp" => self.app.pan(0, -4),
            "j" | "ArrowDown" => self.app.pan(0, 4),
            "+" | "=" => self.app.zoom_in(),
            "-" => self.app.zoom_out(),
            " " => {
                if let Some((col, row)) = self.app.mouse_pos {
                    self.app.launch_nuke(col, row);
                }
            }
            "r" | "0" => self.reset(),
            _ => {}
        }
    }

    pub fn reset(&mut self) {
        let mut renderer = std::mem::replace(&mut self.app.map_renderer, MapRenderer::new());
        for idx in 0..renderer.city_grid.len() {
            if let Some(city) = renderer.city_grid.get_mut(idx) {
                city.set_population(city.original_population);
            }
        }
        renderer.invalidate_cache();
        self.app = App::new(
            self.width as usize,
            (self.height - ui::menu::layout(self.width).0) as usize,
        );
        self.app.frame = 30;
        self.paused = false;
        self.map_drag = false;
        self.app.map_renderer = renderer;
    }

    /// Three u32s per cell: Unicode scalar, foreground RGB, background RGB.
    pub fn render(&mut self) -> Result<Vec<u32>, JsValue> {
        self.terminal
            .draw(|frame| {
                let menu_height = ui::menu::layout(self.width).0;
                let map_height = self.height - menu_height;
                ui::render_in(
                    frame,
                    &mut self.app,
                    ratatui::layout::Rect::new(0, 0, self.width, map_height),
                );
                ui::menu::render(
                    frame,
                    &self.app,
                    ratatui::layout::Rect::new(0, map_height, self.width, menu_height),
                    self.paused,
                    self.help,
                );
            })
            .map_err(js_error)?;
        let cells = &self.terminal.backend().buffer().content;
        let mut output = Vec::with_capacity(cells.len() * 3);
        for cell in cells {
            let mut fg = rgb(cell.fg, 0xb8cbd5);
            let mut bg = rgb(cell.bg, 0x080e16);
            if cell.modifier.contains(Modifier::REVERSED) {
                std::mem::swap(&mut fg, &mut bg);
            }
            if cell.modifier.contains(Modifier::DIM) {
                fg = ((fg & 0xfefefe) >> 1) & 0x7f7f7f;
            }
            output.extend_from_slice(&[cell.symbol().chars().next().unwrap_or(' ') as u32, fg, bg]);
        }
        Ok(output)
    }

    pub fn status(&self) -> String {
        serde_json::json!({"weapon":self.app.active_weapon.label(),
            "projection":if matches!(self.app.projection,Projection::Globe(_)){"Globe"}else{"Mercator"},
            "zoom":self.app.zoom_level(),"center":self.app.center_coords(),
            "fires":self.app.fires.len(),"effects":self.app.explosions.len(),
            "casualties":self.app.casualties,"frame":self.app.frame, "paused":self.paused, "help":self.help, "mapRows":self.height - ui::menu::layout(self.width).0}).to_string()
    }
}

fn js_error(error: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&error.to_string())
}

fn rgb(color: Color, default: u32) -> u32 {
    const ANSI: [u32; 16] = [
        0x080e16, 0xe45b60, 0x81c784, 0xe3c674, 0x609cde, 0xc886da, 0x72d6d4, 0xc8d5df, 0x536474,
        0xff7e82, 0xa4ee9a, 0xffdf90, 0x8fc1ff, 0xe8adff, 0xa5f4ed, 0xf3f7fa,
    ];
    match color {
        Color::Reset => default,
        Color::Rgb(r, g, b) => (r as u32) << 16 | (g as u32) << 8 | b as u32,
        Color::Black => ANSI[0],
        Color::Red => ANSI[1],
        Color::Green => ANSI[2],
        Color::Yellow => ANSI[3],
        Color::Blue => ANSI[4],
        Color::Magenta => ANSI[5],
        Color::Cyan => ANSI[6],
        Color::Gray => ANSI[7],
        Color::DarkGray => ANSI[8],
        Color::LightRed => ANSI[9],
        Color::LightGreen => ANSI[10],
        Color::LightYellow => ANSI[11],
        Color::LightBlue => ANSI[12],
        Color::LightMagenta => ANSI[13],
        Color::LightCyan => ANSI[14],
        Color::White => ANSI[15],
        Color::Indexed(i) if i < 16 => ANSI[i as usize],
        Color::Indexed(i) if i >= 232 => {
            let v = 8 + (i as u32 - 232) * 10;
            v << 16 | v << 8 | v
        }
        Color::Indexed(i) => {
            let i = i as u32 - 16;
            let c = |n| if n == 0 { 0 } else { 55 + n * 40 };
            c(i / 36) << 16 | c(i / 6 % 6) << 8 | c(i % 6)
        }
    }
}
