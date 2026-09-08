use super::ExplosionRender;
use crate::map::GlobeViewport;
use ratatui::{buffer::Buffer, layout::Rect};

pub fn render(
    exp: &ExplosionRender,
    x: u16,
    y: u16,
    area: Rect,
    frame: u64,
    buf: &mut Buffer,
    globe: Option<&GlobeViewport>,
) {
    super::aerosol::render(exp, x, y, area, frame, buf, globe);
}
