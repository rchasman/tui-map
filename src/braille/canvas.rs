/// Braille Unicode canvas for high-resolution terminal graphics.
/// Each character cell represents a 2x4 pixel grid (8 dots).
/// Unicode Braille patterns: U+2800 to U+28FF
///
/// Flat buffer layout: pixels[cy * width + cx] — single pointer chase,
/// single memcpy on clone, cache-line friendly sequential access.
#[derive(Clone)]
pub struct BrailleCanvas {
    width: usize,  // Characters
    height: usize, // Characters
    pixels: Vec<u8>, // Flat row-major bit patterns
}

/// Braille bit position lookup: BIT_TABLE[y & 3][x & 1]
/// Eliminates the branch in the tightest inner loop.
static BIT_TABLE: [[u8; 2]; 4] = [
    [0, 3], // y%4=0: bit 0 (left col) or 3 (right col)
    [1, 4], // y%4=1: bit 1 or 4
    [2, 5], // y%4=2: bit 2 or 5
    [6, 7], // y%4=3: bit 6 or 7
];

impl BrailleCanvas {
    /// Create a new canvas with the given character dimensions.
    /// Effective pixel resolution: width*2 x height*4
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixels: vec![0u8; width * height],
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
    #[inline(always)]
    pub fn set_pixel(&mut self, x: usize, y: usize) {
        let cx = x >> 1;  // x / 2
        let cy = y >> 2;  // y / 4

        if cx >= self.width || cy >= self.height {
            return;
        }

        let bit = 1u8 << BIT_TABLE[y & 3][x & 1];

        // Safety: bounds checked above
        unsafe {
            *self.pixels.get_unchecked_mut(cy * self.width + cx) |= bit;
        }
    }

    /// Set a pixel using signed coordinates (ignores negative values)
    #[inline(always)]
    pub fn set_pixel_signed(&mut self, x: i32, y: i32) {
        if x >= 0 && y >= 0 {
            self.set_pixel(x as usize, y as usize);
        }
    }

    /// Convert the canvas to a string of Braille characters
    #[cfg(test)]
    pub fn to_string(&self) -> String {
        (0..self.height)
            .map(|row| self.row_to_string(row))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Get a specific row as a string (for line-by-line rendering)
    pub fn row_to_string(&self, row: usize) -> String {
        if row >= self.height {
            return String::new();
        }
        let start = row * self.width;
        self.pixels[start..start + self.width]
            .iter()
            .map(|&b| char::from_u32(0x2800 + b as u32).unwrap_or(' '))
            .collect()
    }

    /// Get all rows as an iterator of strings
    pub fn rows(&self) -> impl Iterator<Item = String> + '_ {
        (0..self.height).map(|i| self.row_to_string(i))
    }

    /// Raw byte slice for a row — zero allocation, for direct buffer writes.
    #[inline(always)]
    pub fn row_raw(&self, row: usize) -> &[u8] {
        let start = row * self.width;
        &self.pixels[start..start + self.width]
    }

    /// Number of character rows.
    #[inline(always)]
    pub fn char_height(&self) -> usize {
        self.height
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
