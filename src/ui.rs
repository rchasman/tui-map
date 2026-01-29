use crate::app::App;
use crate::map::MapLayers;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
    Frame,
};

/// Render the UI
pub fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // Split into map area and status bar
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),    // Map
            Constraint::Length(1), // Status bar
        ])
        .split(area);

    render_map(frame, app, chunks[0]);
    render_status_bar(frame, app, chunks[1]);
}

fn render_map(frame: &mut Frame, app: &App, area: Rect) {
    // Create a block with border
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(Span::styled(
            " World Map ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Update viewport size for rendering
    let mut viewport = app.viewport.clone();
    // Braille gives 2x4 resolution per character
    viewport.width = inner.width as usize * 2;
    viewport.height = inner.height as usize * 4;

    // Render map layers
    let layers = app.map_renderer.render(inner.width as usize, inner.height as usize, &viewport);

    // Get mouse cursor position for marker
    let cursor_pos = app.mouse_pixel_pos().and_then(|(px, py)| {
        // Convert braille pixels to character position
        let cx = (px / 2) as u16;
        let cy = (py / 4) as u16;
        if cx < inner.width && cy < inner.height {
            Some((cx, cy))
        } else {
            None
        }
    });

    // Render braille map
    let map_widget = MapWidget {
        layers,
        cursor_pos,
        has_states: app.map_renderer.settings.show_states,
        zoom: viewport.zoom,
        inner_width: inner.width,
        inner_height: inner.height,
    };
    frame.render_widget(map_widget, inner);
}

/// Custom widget that renders braille map with text labels overlaid
struct MapWidget {
    layers: MapLayers,
    cursor_pos: Option<(u16, u16)>,
    has_states: bool,
    zoom: f64,
    inner_width: u16,
    inner_height: u16,
}

impl MapWidget {
    /// Render a braille canvas layer with a specific color
    fn render_layer(&self, canvas: &crate::braille::BrailleCanvas, color: Color, area: Rect, buf: &mut Buffer) {
        for (row_idx, row_str) in canvas.rows().enumerate() {
            if row_idx >= area.height as usize {
                break;
            }
            let y = area.y + row_idx as u16;

            for (col_idx, ch) in row_str.chars().enumerate() {
                if col_idx >= area.width as usize {
                    break;
                }
                // Skip empty braille characters (U+2800)
                if ch == '\u{2800}' {
                    continue;
                }
                let x = area.x + col_idx as u16;
                buf[(x, y)].set_char(ch).set_fg(color);
            }
        }
    }
}

impl Widget for MapWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Render layers from back to front:
        // 1. Coastlines (Cyan - at back)
        self.render_layer(&self.layers.coastlines, Color::Cyan, area, buf);

        // 2. County borders (DarkGray)
        self.render_layer(&self.layers.counties, Color::DarkGray, area, buf);

        // 3. Country borders (Yellow if states visible at this zoom, else Cyan)
        let states_visible = self.has_states && self.zoom >= 4.0;
        let border_color = if states_visible { Color::Yellow } else { Color::Cyan };
        self.render_layer(&self.layers.borders, border_color, area, buf);

        // 4. State borders (Yellow - on top)
        self.render_layer(&self.layers.states, Color::Yellow, area, buf);

        // Then overlay city markers and labels
        let marker_style = Style::default().fg(Color::White);
        let label_style = Style::default().fg(Color::White);

        for (lx, ly, text) in &self.layers.labels {
            // Check bounds
            if *ly >= self.inner_height || *lx >= self.inner_width {
                continue;
            }

            let x = area.x + *lx;
            let y = area.y + *ly;

            // Check if this is a marker glyph (single char) or a label
            let is_marker = text.len() <= 3 && matches!(text.chars().next(), Some('⚜' | '★' | '◆' | '■' | '●' | '○' | '◦' | '·'));
            let style = if is_marker { marker_style } else { label_style };

            // Truncate label to fit screen, allow longer labels for population
            let max_len = (self.inner_width.saturating_sub(*lx)) as usize;
            let max_display = if is_marker { 1 } else { 24 };
            let display_text: String = text.chars().take(max_len.min(max_display)).collect();

            for (i, ch) in display_text.chars().enumerate() {
                let px = x + i as u16;
                if px < area.x + area.width {
                    buf[(px, y)].set_char(ch).set_style(style);
                }
            }
        }

        // Render cursor marker
        if let Some((cx, cy)) = self.cursor_pos {
            let x = area.x + cx;
            let y = area.y + cy;
            if x < area.x + area.width && y < area.y + area.height {
                buf[(x, y)].set_char('╋').set_fg(Color::Red);
            }
        }
    }
}

fn render_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let settings = &app.map_renderer.settings;

    let status = Line::from(vec![
        Span::styled(" Zoom: ", Style::default().fg(Color::DarkGray)),
        Span::styled(app.zoom_level(), Style::default().fg(Color::Yellow)),
        Span::styled(" (", Style::default().fg(Color::DarkGray)),
        Span::styled(app.lod_level(), Style::default().fg(Color::Magenta)),
        Span::styled(") ", Style::default().fg(Color::DarkGray)),
        // Toggle indicators
        Span::styled(
            if settings.show_borders { "[B]order " } else { "[b]order " },
            Style::default().fg(if settings.show_borders { Color::Green } else { Color::DarkGray }),
        ),
        Span::styled(
            if settings.show_states { "[S]tate " } else { "[s]tate " },
            Style::default().fg(if settings.show_states { Color::Green } else { Color::DarkGray }),
        ),
        Span::styled(
            if settings.show_counties { "[Y]county " } else { "[y]county " },
            Style::default().fg(if settings.show_counties { Color::Green } else { Color::DarkGray }),
        ),
        Span::styled(
            if settings.show_cities { "[C]ities " } else { "[c]ities " },
            Style::default().fg(if settings.show_cities { Color::Green } else { Color::DarkGray }),
        ),
        Span::styled(
            if settings.show_labels { "[L]abels " } else { "[l]abels " },
            Style::default().fg(if settings.show_labels { Color::Green } else { Color::DarkGray }),
        ),
        Span::styled(
            if settings.show_population { "[P]op " } else { "[p]op " },
            Style::default().fg(if settings.show_population { Color::Green } else { Color::DarkGray }),
        ),
        Span::styled("| ", Style::default().fg(Color::DarkGray)),
        Span::styled(app.center_coords(), Style::default().fg(Color::Cyan)),
        Span::styled(
            " | hjkl:pan +/-:zoom r:reset q:quit",
            Style::default().fg(Color::DarkGray),
        ),
    ]);

    let paragraph = Paragraph::new(status);
    frame.render_widget(paragraph, area);
}
