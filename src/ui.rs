use crate::app::App;
use crate::braille::BrailleCanvas;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
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

    app.map_renderer.render(&mut canvas, &viewport);

    // Convert canvas to paragraph lines
    let lines: Vec<Line> = canvas
        .rows()
        .map(|row| Line::from(Span::styled(row, Style::default().fg(Color::Cyan))))
        .collect();

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

fn render_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let status = Line::from(vec![
        Span::styled(" Zoom: ", Style::default().fg(Color::DarkGray)),
        Span::styled(app.zoom_level(), Style::default().fg(Color::Yellow)),
        Span::styled(" (", Style::default().fg(Color::DarkGray)),
        Span::styled(app.lod_level(), Style::default().fg(Color::Magenta)),
        Span::styled(") | ", Style::default().fg(Color::DarkGray)),
        Span::styled(app.center_coords(), Style::default().fg(Color::Green)),
        Span::styled(
            " | hjkl: pan, +/-: zoom, r: reset, q: quit",
            Style::default().fg(Color::DarkGray),
        ),
    ]);

    let paragraph = Paragraph::new(status);
    frame.render_widget(paragraph, area);
}
