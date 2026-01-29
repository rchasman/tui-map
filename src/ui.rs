use crate::app::App;
use crate::braille::BrailleCanvas;
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

    // Create braille canvas sized to inner area
    let mut canvas = BrailleCanvas::new(inner.width as usize, inner.height as usize);

    // Update viewport size and render
    let mut viewport = app.viewport.clone();
    viewport.width = canvas.pixel_width();
    viewport.height = canvas.pixel_height();

    let labels = app.map_renderer.render(&mut canvas, &viewport);

    // Render braille map
    let map_widget = MapWidget {
        canvas,
        labels,
        inner_width: inner.width,
        inner_height: inner.height,
    };
    frame.render_widget(map_widget, inner);
}

/// Custom widget that renders braille map with text labels overlaid
struct MapWidget {
    canvas: BrailleCanvas,
    labels: Vec<(u16, u16, String)>,
    inner_width: u16,
    inner_height: u16,
}

impl Widget for MapWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // First render the braille characters
        for (row_idx, row_str) in self.canvas.rows().enumerate() {
            if row_idx >= area.height as usize {
                break;
            }
            let y = area.y + row_idx as u16;

            for (col_idx, ch) in row_str.chars().enumerate() {
                if col_idx >= area.width as usize {
                    break;
                }
                let x = area.x + col_idx as u16;
                buf[(x, y)].set_char(ch).set_fg(Color::Cyan);
            }
        }

        // Then overlay city markers and labels
        let marker_style = Style::default().fg(Color::White);
        let label_style = Style::default().fg(Color::Yellow);

        for (lx, ly, text) in &self.labels {
            // Check bounds
            if *ly >= self.inner_height || *lx >= self.inner_width {
                continue;
            }

            let x = area.x + *lx;
            let y = area.y + *ly;

            // Check if this is a marker glyph (single char) or a label
            let is_marker = text.len() == 1 && matches!(text.chars().next(), Some('◆' | '●' | '○' | '·'));
            let style = if is_marker { marker_style } else { label_style };

            // Truncate label to fit
            let max_len = (self.inner_width.saturating_sub(*lx)) as usize;
            let max_display = if is_marker { 1 } else { 12 };
            let display_text: String = text.chars().take(max_len.min(max_display)).collect();

            for (i, ch) in display_text.chars().enumerate() {
                let px = x + i as u16;
                if px < area.x + area.width {
                    buf[(px, y)].set_char(ch).set_style(style);
                }
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
            if settings.show_borders { "[B]orders " } else { "[b]orders " },
            Style::default().fg(if settings.show_borders { Color::Green } else { Color::DarkGray }),
        ),
        Span::styled(
            if settings.show_states { "[S]tates " } else { "[s]tates " },
            Style::default().fg(if settings.show_states { Color::Green } else { Color::DarkGray }),
        ),
        Span::styled(
            if settings.show_cities { "[C]ities " } else { "[c]ities " },
            Style::default().fg(if settings.show_cities { Color::Green } else { Color::DarkGray }),
        ),
        Span::styled(
            if settings.show_labels { "[L]abels " } else { "[l]abels " },
            Style::default().fg(if settings.show_labels { Color::Green } else { Color::DarkGray }),
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
