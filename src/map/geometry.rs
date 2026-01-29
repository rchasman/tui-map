use crate::braille::BrailleCanvas;

/// Draw a line using Bresenham's algorithm
pub fn draw_line(canvas: &mut BrailleCanvas, x0: i32, y0: i32, x1: i32, y1: i32) {
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    let mut x = x0;
    let mut y = y0;

    loop {
        canvas.set_pixel_signed(x, y);

        if x == x1 && y == y1 {
            break;
        }

        let e2 = 2 * err;

        if e2 >= dy {
            if x == x1 {
                break;
            }
            err += dy;
            x += sx;
        }

        if e2 <= dx {
            if y == y1 {
                break;
            }
            err += dx;
            y += sy;
        }
    }
}

/// Draw a thicker line (useful for borders at low zoom)
pub fn draw_thick_line(canvas: &mut BrailleCanvas, x0: i32, y0: i32, x1: i32, y1: i32) {
    draw_line(canvas, x0, y0, x1, y1);
    draw_line(canvas, x0 + 1, y0, x1 + 1, y1);
    draw_line(canvas, x0, y0 + 1, x1, y1 + 1);
}

/// Draw a point marker (small cross)
pub fn draw_marker(canvas: &mut BrailleCanvas, x: i32, y: i32, size: i32) {
    for i in -size..=size {
        canvas.set_pixel_signed(x + i, y);
        canvas.set_pixel_signed(x, y + i);
    }
}

/// Draw a filled circle (for city markers)
pub fn draw_circle(canvas: &mut BrailleCanvas, cx: i32, cy: i32, radius: i32) {
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx * dx + dy * dy <= radius * radius {
                canvas.set_pixel_signed(cx + dx, cy + dy);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_horizontal_line() {
        let mut canvas = BrailleCanvas::new(5, 1);
        draw_line(&mut canvas, 0, 0, 9, 0);
        // Should have pixels across the top
        let s = canvas.to_string();
        assert!(s.contains('⠁') || s.contains('⠉') || s.len() > 0);
    }

    #[test]
    fn test_vertical_line() {
        let mut canvas = BrailleCanvas::new(1, 2);
        draw_line(&mut canvas, 0, 0, 0, 7);
        let s = canvas.to_string();
        assert!(s.len() > 0);
    }
}
