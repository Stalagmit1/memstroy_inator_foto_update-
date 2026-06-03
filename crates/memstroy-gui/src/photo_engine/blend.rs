use crate::photo_engine::types::{clamp01, clamp_u8, hsl_to_rgba, rgba_to_hsl, BlendMode, PixelBuffer};

pub fn alpha_over(dst: [u8; 4], src: [u8; 4], opacity: f32, mode: BlendMode) -> [u8; 4] {
    let src_a = (src[3] as f32 / 255.0) * clamp01(opacity);
    let dst_a = dst[3] as f32 / 255.0;
    if src_a <= 0.0 { return dst; }
    let out_a = src_a + dst_a * (1.0 - src_a);
    if out_a <= 0.0 { return [0, 0, 0, 0]; }

    let blended = blend_rgb(dst, src, mode);
    let mut out = [0u8; 4];
    for i in 0..3 {
        let s = blended[i] as f32 / 255.0;
        let d = dst[i] as f32 / 255.0;
        let c = (s * src_a + d * dst_a * (1.0 - src_a)) / out_a;
        out[i] = clamp_u8(c * 255.0);
    }
    out[3] = clamp_u8(out_a * 255.0);
    out
}

pub fn composite_in_place(dst: &mut PixelBuffer, src: &PixelBuffer, x: i32, y: i32, opacity: f32, mode: BlendMode) {
    for sy in 0..src.height as i32 {
        for sx in 0..src.width as i32 {
            let dx = x + sx;
            let dy = y + sy;
            if !dst.in_bounds(dx, dy) { continue; }
            let d = dst.get(dx as u32, dy as u32);
            let s = src.get(sx as u32, sy as u32);
            dst.set(dx as u32, dy as u32, alpha_over(d, s, opacity, mode));
        }
    }
}

pub fn blend_rgb(dst: [u8; 4], src: [u8; 4], mode: BlendMode) -> [u8; 4] {
    use BlendMode::*;
    match mode {
        Normal | Dissolve => [src[0], src[1], src[2], src[3]],
        Hue | Saturation | Color | Luminosity => blend_hsl(dst, src, mode),
        DarkerColor => if luma(src) < luma(dst) { src } else { dst },
        LighterColor => if luma(src) > luma(dst) { src } else { dst },
        _ => {
            let mut out = src;
            for i in 0..3 {
                let d = dst[i] as f32 / 255.0;
                let s = src[i] as f32 / 255.0;
                let c = match mode {
                    Darken => d.min(s),
                    Multiply => d * s,
                    ColorBurn => if s <= 0.0 { 0.0 } else { 1.0 - ((1.0 - d) / s).min(1.0) },
                    LinearBurn => (d + s - 1.0).max(0.0),
                    Lighten => d.max(s),
                    Screen => 1.0 - (1.0 - d) * (1.0 - s),
                    ColorDodge => if s >= 1.0 { 1.0 } else { (d / (1.0 - s)).min(1.0) },
                    LinearDodge => (d + s).min(1.0),
                    Overlay => if d < 0.5 { 2.0 * d * s } else { 1.0 - 2.0 * (1.0 - d) * (1.0 - s) },
                    SoftLight => soft_light(d, s),
                    HardLight => if s < 0.5 { 2.0 * d * s } else { 1.0 - 2.0 * (1.0 - d) * (1.0 - s) },
                    VividLight => if s < 0.5 { color_burn(d, 2.0 * s) } else { color_dodge(d, 2.0 * (s - 0.5)) },
                    LinearLight => (d + 2.0 * s - 1.0).max(0.0).min(1.0),
                    PinLight => if s < 0.5 { d.min(2.0 * s) } else { d.max(2.0 * (s - 0.5)) },
                    HardMix => if (if s < 0.5 { 2.0 * d * s } else { 1.0 - 2.0 * (1.0 - d) * (1.0 - s) }) < 0.5 { 0.0 } else { 1.0 },
                    Difference => (d - s).abs(),
                    Exclusion => d + s - 2.0 * d * s,
                    Subtract => (d - s).max(0.0),
                    Divide => if s <= 0.0 { 1.0 } else { (d / s).min(1.0) },
                    _ => s,
                };
                out[i] = clamp_u8(c * 255.0);
            }
            out
        }
    }
}

fn blend_hsl(dst: [u8; 4], src: [u8; 4], mode: BlendMode) -> [u8; 4] {
    let (dh, ds, dl) = rgba_to_hsl(dst);
    let (sh, ss, sl) = rgba_to_hsl(src);
    match mode {
        BlendMode::Hue => hsl_to_rgba(sh, ds, dl, src[3]),
        BlendMode::Saturation => hsl_to_rgba(dh, ss, dl, src[3]),
        BlendMode::Color => hsl_to_rgba(sh, ss, dl, src[3]),
        BlendMode::Luminosity => hsl_to_rgba(dh, ds, sl, src[3]),
        _ => src,
    }
}

fn luma(c: [u8; 4]) -> f32 {
    c[0] as f32 * 0.2126 + c[1] as f32 * 0.7152 + c[2] as f32 * 0.0722
}

fn color_dodge(d: f32, s: f32) -> f32 {
    if s >= 1.0 { 1.0 } else { (d / (1.0 - s)).min(1.0) }
}

fn color_burn(d: f32, s: f32) -> f32 {
    if s <= 0.0 { 0.0 } else { 1.0 - ((1.0 - d) / s).min(1.0) }
}

fn soft_light(d: f32, s: f32) -> f32 {
    if s < 0.5 {
        d - (1.0 - 2.0 * s) * d * (1.0 - d)
    } else {
        let g = if d <= 0.25 { ((16.0 * d - 12.0) * d + 4.0) * d } else { d.sqrt() };
        d + (2.0 * s - 1.0) * (g - d)
    }
}
