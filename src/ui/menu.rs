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
        ("1", "1 Nuke"),
        ("2", "2 Bio"),
        ("3", "3 EMP"),
        ("4", "4 Chem"),
        ("5", "5 Water"),
        ("6", "6 Life"),
    ],
    &[
        ("g", "g Globe/map"),
        ("-", "-"),
        ("+", "+"),
        ("Escape", "Esc Pause"),
        ("r", "r Reset"),
        ("?", "? Help"),
    ],
    &[
        ("b", "b Borders"),
        ("s", "s States"),
        ("y", "y Counties"),
        ("c", "c Cities"),
        ("L", "L Labels"),
        ("p", "p Population"),
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

pub fn render(frame: &mut Frame, app: &App, area: Rect, paused: bool, help: bool) {
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(" Controls · click or press key ")
            .border_style(Style::default().fg(Color::DarkGray)),
        area,
    );
    let settings = &app.map_renderer.settings;
    for item in layout(area.width).1 {
        let selected = match item.key {
            "1" => app.active_weapon.label() == "NUKE",
            "2" => app.active_weapon.label() == "BIO",
            "3" => app.active_weapon.label() == "EMP",
            "4" => app.active_weapon.label() == "CHEM",
            "5" => app.active_weapon.label() == "WATER",
            "6" => app.active_weapon.label() == "LIFE",
            "b" => settings.show_borders,
            "s" => settings.show_states,
            "y" => settings.show_counties,
            "c" => settings.show_cities,
            "L" => settings.show_labels,
            "p" => settings.show_population,
            "Escape" => paused,
            _ => false,
        };
        let label = if item.key == "Escape" && paused {
            "Esc Play "
        } else {
            item.label
        };
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
        let height = size.height.min(17);
        let area = Rect::new(
            (size.width - width) / 2,
            (size.height - height) / 2,
            width,
            height,
        );
        frame.render_widget(Clear, area);
        frame.render_widget(Paragraph::new("Drag / swipe: rotate or pan\nClick / tap / Space: release effect\nScroll / +/-: zoom\n1-6: select effect\nArrows / hjkl: pan\ng: globe / flat map\nEsc: pause / resume   r: reset\n\nWater + fire = steam\nEMP + gas = electric filaments\nWater + life = blooms\n\nClick or press ? / Esc to close")
            .wrap(Wrap { trim: true }).block(Block::default().borders(Borders::ALL).title(" Help "))
            .style(Style::default().fg(Color::Cyan).bg(Color::Black)), area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_controls_fit_and_hit_their_own_cells() {
        for width in [40, 55, 120, 240] {
            let (height, items) = layout(width);
            assert_eq!(items.len(), 18);
            for item in items {
                assert!(item.area.right() < width);
                assert!(item.area.bottom() < height);
                assert_eq!(hit(width, item.area.x, item.area.y), Some(item.key));
            }
            assert_eq!(hit(width, 0, 0), None);
        }
    }
}
