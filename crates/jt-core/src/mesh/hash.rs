//! JT 9.5 Appendix D. Bob Jenkins (1996), freely usable as stated there.
fn mix(mut a: u32, mut b: u32, mut c: u32) -> (u32, u32, u32) {
    a = a.wrapping_sub(b).wrapping_sub(c) ^ (c >> 13);
    b = b.wrapping_sub(c).wrapping_sub(a) ^ (a << 8);
    c = c.wrapping_sub(a).wrapping_sub(b) ^ (b >> 13);
    a = a.wrapping_sub(b).wrapping_sub(c) ^ (c >> 12);
    b = b.wrapping_sub(c).wrapping_sub(a) ^ (a << 16);
    c = c.wrapping_sub(a).wrapping_sub(b) ^ (b >> 5);
    a = a.wrapping_sub(b).wrapping_sub(c) ^ (c >> 3);
    b = b.wrapping_sub(c).wrapping_sub(a) ^ (a << 10);
    c = c.wrapping_sub(a).wrapping_sub(b) ^ (b >> 15);
    (a, b, c)
}
pub(super) fn words(data: &[i32], seed: u32) -> u32 {
    let (mut a, mut b, mut c) = (0x9e3779b9u32, 0x9e3779b9u32, seed);
    let mut chunks = data.chunks_exact(3);
    for w in &mut chunks {
        a = a.wrapping_add(w[0] as u32);
        b = b.wrapping_add(w[1] as u32);
        c = c.wrapping_add(w[2] as u32);
        (a, b, c) = mix(a, b, c);
    }
    c = c.wrapping_add(data.len() as u32);
    let rem = chunks.remainder();
    if rem.len() > 1 {
        b = b.wrapping_add(rem[1] as u32);
    }
    if !rem.is_empty() {
        a = a.wrapping_add(rem[0] as u32);
    }
    mix(a, b, c).2
}
pub(super) fn shorts(data: &[i32], seed: u32) -> u32 {
    let (mut a, mut b, mut c) = (0x9e3779b9u32, 0x9e3779b9u32, seed);
    let mut chunks = data.chunks_exact(6);
    for w in &mut chunks {
        a = a.wrapping_add((w[0] as u16 as u32) | ((w[1] as u16 as u32) << 16));
        b = b.wrapping_add((w[2] as u16 as u32) | ((w[3] as u16 as u32) << 16));
        c = c.wrapping_add((w[4] as u16 as u32) | ((w[5] as u16 as u32) << 16));
        (a, b, c) = mix(a, b, c);
    }
    c = c.wrapping_add(data.len() as u32);
    for (i, &w) in chunks.remainder().iter().enumerate() {
        let w = w as u16 as u32;
        match i {
            0 => a = a.wrapping_add(w),
            1 => a = a.wrapping_add(w << 16),
            2 => b = b.wrapping_add(w),
            3 => b = b.wrapping_add(w << 16),
            4 => c = c.wrapping_add(w << 16),
            _ => unreachable!(),
        }
    }
    mix(a, b, c).2
}
