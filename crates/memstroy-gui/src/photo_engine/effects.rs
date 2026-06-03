use crate::photo_engine::blend::alpha_over;
use crate::photo_engine::filters::box_blur;
use crate::photo_engine::types::{clamp_u8, LayerEffect, LayerEffectKind, PixelBuffer, RectI};

pub fn apply_layer_effects(img: &mut PixelBuffer, effects: &[LayerEffect]) {
    for effect in effects.iter().filter(|e| e.enabled) {
        match effect.kind {
            LayerEffectKind::DropShadow => drop_shadow(img, effect),
            LayerEffectKind::OuterGlow => outer_glow(img, effect),
            LayerEffectKind::Stroke => stroke(img, effect),
            LayerEffectKind::ColorOverlay => color_overlay(img, effect),
            LayerEffectKind::GradientOverlay => gradient_overlay(img, effect),
            LayerEffectKind::InnerShadow => inner_shadow(img, effect),
            LayerEffectKind::InnerGlow => inner_glow(img, effect),
        }
    }
}

pub fn drop_shadow(img: &mut PixelBuffer, effect: &LayerEffect) {
    let pad = (effect.size + effect.distance.abs() + 4.0).ceil() as u32;
    let mut out = PixelBuffer::new(img.width + pad * 2, img.height + pad * 2);
    let angle = effect.angle_degrees.to_radians();
    let dx = effect.distance * angle.cos();
    let dy = effect.distance * angle.sin();
    let mut shadow = PixelBuffer::new(out.width, out.height);
    for y in 0..img.height {
        for x in 0..img.width {
            let a = img.get(x, y)[3];
            if a == 0 { continue; }
            let mut c = effect.color;
            c[3] = clamp_u8(a as f32 * effect.opacity * (effect.color[3] as f32 / 255.0));
            shadow.set_checked((x + pad) as i32 + dx.round() as i32, (y + pad) as i32 + dy.round() as i32, c);
        }
    }
    shadow = box_blur(&shadow, effect.size.max(0.0) as u32);
    for y in 0..out.height {
        for x in 0..out.width {
            out.set(x, y, shadow.get(x, y));
        }
    }
    for y in 0..img.height {
        for x in 0..img.width {
            let d = out.get(x + pad, y + pad);
            let s = img.get(x, y);
            out.set(x + pad, y + pad, alpha_over(d, s, 1.0, crate::photo_engine::types::BlendMode::Normal));
        }
    }
    *img = out;
}

pub fn outer_glow(img: &mut PixelBuffer, effect: &LayerEffect) {
    let mut glow = PixelBuffer::new(img.width, img.height);
    for y in 0..img.height {
        for x in 0..img.width {
            let a = img.get(x, y)[3];
            if a > 0 {
                let mut c = effect.color;
                c[3] = clamp_u8(a as f32 * effect.opacity);
                glow.set(x, y, c);
            }
        }
    }
    glow = box_blur(&glow, effect.size.max(0.0) as u32);
    for y in 0..img.height {
        for x in 0..img.width {
            img.set(x, y, alpha_over(glow.get(x, y), img.get(x, y), 1.0, crate::photo_engine::types::BlendMode::Normal));
        }
    }
}

pub fn inner_glow(img: &mut PixelBuffer, effect: &LayerEffect) {
    let mut edge = PixelBuffer::new(img.width, img.height);
    for y in 0..img.height {
        for x in 0..img.width {
            let a = img.get(x, y)[3];
            if a == 0 { continue; }
            let mut is_edge = false;
            for yy in y.saturating_sub(1)..=(y + 1).min(img.height - 1) {
                for xx in x.saturating_sub(1)..=(x + 1).min(img.width - 1) {
                    if img.get(xx, yy)[3] == 0 { is_edge = true; }
                }
            }
            if is_edge {
                let mut c = effect.color;
                c[3] = clamp_u8(a as f32 * effect.opacity);
                edge.set(x, y, c);
            }
        }
    }
    edge = box_blur(&edge, effect.size.max(0.0) as u32);
    for y in 0..img.height {
        for x in 0..img.width {
            let base = img.get(x, y);
            if base[3] > 0 {
                img.set(x, y, alpha_over(base, edge.get(x, y), 1.0, crate::photo_engine::types::BlendMode::Normal));
            }
        }
    }
}

pub fn inner_shadow(img: &mut PixelBuffer, effect: &LayerEffect) {
    let angle = effect.angle_degrees.to_radians();
    let dx = effect.distance * angle.cos();
    let dy = effect.distance * angle.sin();
    let original = img.clone();
    for y in 0..img.height {
        for x in 0..img.width {
            let base = original.get(x, y);
            if base[3] == 0 { continue; }
            let sx = x as i32 - dx.round() as i32;
            let sy = y as i32 - dy.round() as i32;
            let source_alpha = original.get_checked(sx, sy)[3];
            if source_alpha == 0 {
                let mut c = effect.color;
                c[3] = clamp_u8(base[3] as f32 * effect.opacity);
                img.set(x, y, alpha_over(base, c, 1.0, crate::photo_engine::types::BlendMode::Normal));
            }
        }
    }
}

pub fn stroke(img: &mut PixelBuffer, effect: &LayerEffect) {
    let size = effect.size.max(1.0) as i32;
    let original = img.clone();
    let mut out = img.clone();
    for y in 0..img.height as i32 {
        for x in 0..img.width as i32 {
            if original.get_checked(x, y)[3] > 0 { continue; }
            let mut near = false;
            'outer: for yy in y - size..=y + size {
                for xx in x - size..=x + size {
                    if original.in_bounds(xx, yy) && original.get(xx as u32, yy as u32)[3] > 0 {
                        near = true;
                        break 'outer;
                    }
                }
            }
            if near {
                let mut c = effect.color;
                c[3] = clamp_u8(c[3] as f32 * effect.opacity);
                out.set(x as u32, y as u32, alpha_over(out.get(x as u32, y as u32), c, 1.0, crate::photo_engine::types::BlendMode::Normal));
            }
        }
    }
    *img = out;
}

pub fn color_overlay(img: &mut PixelBuffer, effect: &LayerEffect) {
    for y in 0..img.height {
        for x in 0..img.width {
            let base = img.get(x, y);
            if base[3] == 0 { continue; }
            let mut c = effect.color;
            c[3] = clamp_u8(base[3] as f32 * effect.opacity);
            img.set(x, y, alpha_over(base, c, 1.0, crate::photo_engine::types::BlendMode::Normal));
        }
    }
}

pub fn gradient_overlay(img: &mut PixelBuffer, effect: &LayerEffect) {
    let top = effect.color;
    let bottom = [255 - top[0], 255 - top[1], 255 - top[2], top[3]];
    for y in 0..img.height {
        let t = y as f32 / (img.height - 1).max(1) as f32;
        for x in 0..img.width {
            let base = img.get(x, y);
            if base[3] == 0 { continue; }
            let c = [
                clamp_u8(top[0] as f32 * (1.0 - t) + bottom[0] as f32 * t),
                clamp_u8(top[1] as f32 * (1.0 - t) + bottom[1] as f32 * t),
                clamp_u8(top[2] as f32 * (1.0 - t) + bottom[2] as f32 * t),
                clamp_u8(base[3] as f32 * effect.opacity),
            ];
            img.set(x, y, alpha_over(base, c, 1.0, crate::photo_engine::types::BlendMode::Normal));
        }
    }
}

pub fn content_aware_fill_simple(img: &mut PixelBuffer, area: RectI, passes: u32) {
    for _ in 0..passes.max(1) {
        let before = img.clone();
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                if !img.in_bounds(x, y) { continue; }
                let mut sum = [0u32; 4];
                let mut count = 0u32;
                for yy in y - 2..=y + 2 {
                    for xx in x - 2..=x + 2 {
                        if !before.in_bounds(xx, yy) || area.contains(xx, yy) { continue; }
                        let c = before.get(xx as u32, yy as u32);
                        if c[3] == 0 { continue; }
                        for i in 0..4 { sum[i] += c[i] as u32; }
                        count += 1;
                    }
                }
                if count > 0 {
                    let d = count.max(1);
                    img.set(x as u32, y as u32, [(sum[0] / d) as u8, (sum[1] / d) as u8, (sum[2] / d) as u8, (sum[3] / d) as u8]);
                }
            }
        }
    }
}
