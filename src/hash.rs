/// Fast 2-value hash with xorshift
#[inline(always)]
pub fn hash2(a: u64, b: u64) -> u64 {
    let mut seed = a.wrapping_mul(2654435761).wrapping_add(b.wrapping_mul(2246822519));
    seed ^= seed << 13;
    seed ^= seed >> 7;
    seed ^= seed << 17;
    seed
}

/// Fast 3-value hash with xorshift
#[inline(always)]
pub fn hash3(a: u64, b: u64, c: u64) -> u64 {
    let mut seed = a
        .wrapping_mul(2654435761)
        .wrapping_add(b.wrapping_mul(2246822519))
        .wrapping_add(c);
    seed ^= seed << 13;
    seed ^= seed >> 7;
    seed ^= seed << 17;
    seed
}

/// Fast deterministic random using splitmix64 - handles small seeds properly
#[inline(always)]
pub fn rand_simple(seed: u64) -> f64 {
    let mut x = seed.wrapping_mul(0x9e3779b97f4a7c15);
    x ^= x >> 30;
    x = x.wrapping_mul(0xbf58476d1ce4e5b9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94d049bb133111eb);
    x ^= x >> 31;
    (x >> 11) as f64 / 9007199254740992.0
}
