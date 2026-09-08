//! Cell-based browser controls. Rendering and hit testing share one layout.
use crate::app::App;
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

const GROUPS: &[&[(&str, &str)]] = &[
    &[
        ("Escape", "Esc Crosshair"),
        ("1", "1 Water"),
        ("2", "2 Life"),
        ("3", "3 Nuke"),
        ("4", "4 Bio"),
        ("5", "5 EMP"),
        ("6", "6 Chem"),
        ("7", "7 Tornado"),
        ("8", "8 Frost"),
        ("9", "9 Meteor"),
    ],
    &[
        ("g", "g Globe/map"),
        ("-", "-"),
        ("+", "+"),
        ("r", "r Reset"),
        ("?", "? Help"),
    ],

];

pub struct Item {
    pub key: &'static str,
    label: &'static str,
    pub area: Rect,
}

pub fn layout(width: u16) -> (u16, Vec<Item>) {
    let mut items = Vec::new();
    let mut row = 1;
    for group in GROUPS {
        let mut col = 1;
        for &(key, label) in *group {
            let len = label.len() as u16 + 2;
            if col + len > width - 1 {
                col = 1;
                row += 1;
            }
            items.push(Item {
                key,
                label,
                area: Rect::new(col, row, len, 1),
            });
            col += len + 1;
        }
        row += 1;
    }
    (row + 1, items)
}

pub fn hit(width: u16, col: u16, row: u16) -> Option<&'static str> {
    layout(width)
        .1
        .into_iter()
        .find(|item| item.area.contains((col, row).into()))
        .map(|item| item.key)
}

pub fn render(frame: &mut Frame, app: &App, area: Rect, help: bool) {
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(" Controls · click or press key ")
            .border_style(Style::default().fg(Color::DarkGray)),
        area,
    );
    for item in layout(area.width).1 {
        let selected = match item.key {
            "1" => app.active_weapon.label() == "WATER",
            "2" => app.active_weapon.label() == "LIFE",
            "3" => app.active_weapon.label() == "NUKE",
            "4" => app.active_weapon.label() == "BIO",
            "5" => app.active_weapon.label() == "EMP",
            "6" => app.active_weapon.label() == "CHEM",
            "7" => app.active_weapon.label() == "TORNADO",
            "8" => app.active_weapon.label() == "FROST",
            "9" => app.active_weapon.label() == "METEOR",
            "Escape" => app.feeds.inspect,
            _ => false,
        };
        let selected =
            selected && !(app.feeds.inspect && item.key.chars().all(|c| c.is_ascii_digit()));
        let label = item.label;
        let style = if selected {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else {
            Style::default().fg(Color::Gray)
        };
        frame.render_widget(
            Paragraph::new(format!("[{label}]")).style(style),
            Rect::new(
                area.x + item.area.x,
                area.y + item.area.y,
                item.area.width,
                1,
            ),
        );
    }
    if help {
        let size = frame.area();
        let width = size.width.min(64);
        let height = size.height.min(22);
        let area = Rect::new(
            (size.width - width) / 2,
            (size.height - height) / 2,
            width,
            height,
        );
        frame.render_widget(Clear, area);
        frame.render_widget(Paragraph::new("Drag / swipe: rotate or pan\nClick / tap / Space: select, or use chosen effect\nScroll / +/-: zoom\n1-9: select effect\ne/d/a/t: quakes / hazards / aircraft / satellites\nEsc / i: return to crosshair selection\nArrows / hjkl: pan\ng: globe / flat map\nr: reset\n\nWater + fire = steam\nEMP + gas = electric filaments\nWater / frost + life = blooms\nTornado: swirls fire and gas\nFrost: quenches fire; Meteor: ignites land\n\nClick or press ? / Esc to close")
            .wrap(Wrap { trim: true }).block(Block::default().borders(Borders::ALL).title(" Help "))
            .style(Style::default().fg(Color::Cyan).bg(Color::Black)), area);
    }
}

/// The picker is drawn and hit-tested in terminal cells inside the map border.
const LAYERS: [(&str, &str); 10] = [
    ("b", "Borders"), ("s", "States"), ("y", "Counties"), ("c", "Cities"),
    ("e", "Quakes"), ("d", "Hazards"), ("a", "Aircraft"), ("t", "Satellites"),
    ("L", "Labels"), ("p", "Population"),
];

fn enabled(app: &App) -> [bool; 10] {
    let s = &app.map_renderer.settings;
    [s.show_borders, s.show_states, s.show_counties, s.show_cities,
     app.feeds.layers[0].enabled, app.feeds.layers[1].enabled,
     app.feeds.layers[2].enabled, app.feeds.layers[3].enabled,
     s.show_labels, s.show_population]
}

fn layer_area(width: u16, height: u16, open: bool) -> Rect {
    Rect::new(1, 1, if open { width.saturating_sub(2).min(40) } else { width.saturating_sub(2) },
        if open { height.saturating_sub(2).min(12) } else { 1 })
}

pub fn layer_visible_rows(height: u16) -> u16 { height.saturating_sub(4).min(10) }

pub fn layer_hit(width: u16, height: u16, open: bool, offset: u16, col: u16, row: u16) -> Option<&'static str> {
    let area = layer_area(width, height, open);
    if !area.contains((col, row).into()) { return None; }
    if row == area.y { return Some("v"); }
    if open && row < area.bottom() - 1 {
        return LAYERS.get((offset + row - area.y - 1) as usize).map(|item| item.0);
    }
    Some("")
}

pub fn render_layers(frame: &mut Frame, app: &App, width: u16, height: u16, open: bool, offset: u16) {
    let area = layer_area(width, height, open);
    let states = enabled(app);
    let count = states[..8].iter().filter(|&&on| on).count();
    let names = LAYERS[..8].iter().zip(states).filter(|(_, on)| *on)
        .map(|((_, name), _)| *name).collect::<Vec<_>>().join(" · ");
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(format!("[v Layers: {count} {}]{}", if open { "−" } else { "+" }, if open { String::new() } else { format!(" {names}") }))
        .style(Style::default().fg(Color::Cyan).bg(Color::Reset)),
        Rect::new(area.x, area.y, area.width, 1));
    if !open { return; }
    let rows = layer_visible_rows(height);
    for (i, ((key, name), on)) in LAYERS.iter().zip(states).enumerate().skip(offset as usize).take(rows as usize) {
        let suffix = if (4..8).contains(&i) && on {
            let layer = &app.feeds.layers[i - 4];
            format!(" · {} {}", if layer.visible(app.feeds.now) { layer.markers.len() } else { 0 }, layer.state(app.feeds.now))
        } else { String::new() };
        let row = Rect::new(area.x, area.y + 1 + i as u16 - offset, area.width, 1);
        frame.render_widget(Paragraph::new(format!(" [{mark}] {key} {name}{suffix}", mark=if on { "x" } else { " " }))
            .style(Style::default().fg(if on { Color::White } else { Color::DarkGray })), row);
    }
    frame.render_widget(Paragraph::new(if rows < 10 { format!(" ↑↓  {}–{} / 10", offset + 1, offset + rows) } else { "─".repeat(area.width as usize) }).style(Style::default().fg(Color::DarkGray)),
        Rect::new(area.x, area.bottom() - 1, area.width, 1));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_controls_fit_and_hit_their_own_cells() {
        for width in [40, 55, 120, 240] {
            let (height, items) = layout(width);
            assert_eq!(items.len(), 15);
            for item in items {
                assert!(item.area.right() < width);
                assert!(item.area.bottom() < height);
                assert_eq!(hit(width, item.area.x, item.area.y), Some(item.key));
            }
            assert_eq!(hit(width, 0, 0), None);
        }
    }
}
