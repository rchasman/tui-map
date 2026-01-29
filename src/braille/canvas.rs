/// Braille Unicode canvas for high-resolution terminal graphics.
/// Each character cell represents a 2x4 pixel grid (8 dots).
/// Unicode Braille patterns: U+2800 to U+28FF
pub struct BrailleCanvas {
    width: usize,  // Characters
    height: usize, // Characters
    pixels: Vec<Vec<u8>>, // Bit patterns per char
}

impl BrailleCanvas {
    /// Create a new canvas with the given character dimensions.
    /// Effective pixel resolution: width*2 x height*4
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixels: vec![vec![0u8; width]; height],
        }
    }

    /// Set a pixel at the given coordinates.
    /// Braille dot layout per character:
    /// ```
    /// (0,0) (1,0)   bits: 0x01 0x08
    /// (0,1) (1,1)   bits: 0x02 0x10
    /// (0,2) (1,2)   bits: 0x04 0x20
    /// (0,3) (1,3)   bits: 0x40 0x80
    /// ```
    pub fn set_pixel(&mut self, x: usize, y: usize) {
        let cx = x / 2;
        let cy = y / 4;

        if cx >= self.width || cy >= self.height {
            return;
        }

        let bit = match (x % 2, y % 4) {
            (0, 0) => 0x01,
            (1, 0) => 0x08,
            (0, 1) => 0x02,
            (1, 1) => 0x10,
            (0, 2) => 0x04,
            (1, 2) => 0x20,
            (0, 3) => 0x40,
            (1, 3) => 0x80,
            _ => 0,
        };

        self.pixels[cy][cx] |= bit;
    }

    /// Set a pixel using signed coordinates (ignores negative values)
    pub fn set_pixel_signed(&mut self, x: i32, y: i32) {
        if x >= 0 && y >= 0 {
            self.set_pixel(x as usize, y as usize);
        }
    }

    /// Convert the canvas to a string of Braille characters
    #[cfg(test)]
    pub fn to_string(&self) -> String {
        self.pixels
            .iter()
            .map(|row| {
                row.iter()
                    .map(|&b| char::from_u32(0x2800 + b as u32).unwrap_or(' '))
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Get a specific row as a string (for line-by-line rendering)
    pub fn row_to_string(&self, row: usize) -> String {
        if row >= self.height {
            return String::new();
        }
        self.pixels[row]
            .iter()
            .map(|&b| char::from_u32(0x2800 + b as u32).unwrap_or(' '))
            .collect()
    }

    /// Get all rows as an iterator of strings
    pub fn rows(&self) -> impl Iterator<Item = String> + '_ {
        (0..self.height).map(|i| self.row_to_string(i))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_pixel() {
        let mut canvas = BrailleCanvas::new(1, 1);
        canvas.set_pixel(0, 0);
        assert_eq!(canvas.to_string(), "⠁"); // U+2801
    }

    #[test]
    fn test_all_dots() {
        let mut canvas = BrailleCanvas::new(1, 1);
        // Set all 8 dots
        for x in 0..2 {
            for y in 0..4 {
                canvas.set_pixel(x, y);
            }
        }
        assert_eq!(canvas.to_string(), "⣿"); // U+28FF (all dots)
    }

    #[test]
    fn test_diagonal() {
        let mut canvas = BrailleCanvas::new(2, 1);
        canvas.set_pixel(0, 0);
        canvas.set_pixel(1, 1);
        canvas.set_pixel(2, 2);
        canvas.set_pixel(3, 3);
        // First char: (0,0) and (1,1) = 0x01 | 0x10 = 0x11
        // Second char: (0,2) and (1,3) = 0x04 | 0x80 = 0x84
        assert_eq!(canvas.to_string(), "⠑⢄");
    }
}
