//! Continuous material variation: nearby samples and frames stay related.
use crate::hash::hash3;

pub(super) fn noise(x: f32, y: f32, seed: u64) -> f32 {
    let ix = x.floor() as i64;
    let iy = y.floor() as i64;
    let smooth = |t: f32| t * t * (3.0 - 2.0 * t);
    let tx = smooth(x - x.floor());
    let ty = smooth(y - y.floor());
    let value = |dx: i64, dy: i64| {
        (hash3((ix + dx) as u64, (iy + dy) as u64, seed) & 65535) as f32 / 65535.0
    };
    let a = value(0, 0) * (1.0 - tx) + value(1, 0) * tx;
    let b = value(0, 1) * (1.0 - tx) + value(1, 1) * tx;
    a * (1.0 - ty) + b * ty
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn material_is_continuous_at_cell_boundaries() {
        for i in -10..10 {
            let left = noise(i as f32 - 0.0001, 0.75, 123);
            let right = noise(i as f32 + 0.0001, 0.75, 123);
            assert!((left - right).abs() < 0.001);
            assert!((0.0..=1.0).contains(&left));
        }
    }
}
