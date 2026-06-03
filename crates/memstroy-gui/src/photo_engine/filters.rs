use crate::photo_engine::adjustments::apply_to_selected_raster;
use crate::photo_engine::types::{clamp_u8, LayerKind, PhotoEngineDocument, PixelBuffer};

pub fn gaussian_blur(doc: &mut PhotoEngineDocument, radius: u32) {
    let selection = doc.selection.mask.clone();
    let Some(layer) = doc.selected_layer_mut() else { return; };
    if layer.locked { return; }
    if !matches!(&layer.kind, LayerKind::Raster(_)) { return; }
    doc.push_history();
    let layer = doc.selected_layer_mut().unwrap();
    if let LayerKind::Raster(img) = &mut layer.kind {
        let blurred = box_blur(img, radius);
        blend_selected(img, &blurred, selection.as_ref());
    }
}

pub fn surface_blur(doc: &mut PhotoEngineDocument, radius: u32, threshold: u8) {
    let threshold = threshold as i32;
    apply_raster_filter(doc, |src| {
        let mut out = src.clone();
        let r = radius as i32;
        for y in 0..src.height as i32 {
            for x in 0..src.width as i32 {
                let center = src.get(x as u32, y as u32);
                let mut sum = [0u32; 4];
                let mut count = 0u32;
                for yy in y - r..=y + r {
                    for xx in x - r..=x + r {
                        if !src.in_bounds(xx, yy) { continue; }
                        let c = src.get(xx as u32, yy as u32);
                        if color_dist(center, c) <= threshold {
                            for i in 0..4 { sum[i] += c[i] as u32; }
                            count += 1;
                        }
                    }
                }
                let d = count.max(1);
                out.set(x as u32, y as u32, [(sum[0] / d) as u8, (sum[1] / d) as u8, (sum[2] / d) as u8, (sum[3] / d) as u8]);
            }
        }
        out
    });
}

pub fn motion_blur(doc: &mut PhotoEngineDocument, radius: u32, angle_degrees: f32) {
    let angle = angle_degrees.to_radians();
    let dx = angle.cos();
    let dy = angle.sin();
    apply_raster_filter(doc, |src| {
        let mut out = src.clone();
        let r = radius as i32;
        for y in 0..src.height as i32 {
            for x in 0..src.width as i32 {
                let mut sum = [0u32; 4];
                let mut count = 0u32;
                for i in -r..=r {
                    let sx = x + (i as f32 * dx).round() as i32;
                    let sy = y + (i as f32 * dy).round() as i32;
                    if src.in_bounds(sx, sy) {
                        let c = src.get(sx as u32, sy as u32);
                        for k in 0..4 { sum[k] += c[k] as u32; }
                        count += 1;
                    }
                }
                let d = count.max(1);
                out.set(x as u32, y as u32, [(sum[0] / d) as u8, (sum[1] / d) as u8, (sum[2] / d) as u8, (sum[3] / d) as u8]);
            }
        }
        out
    });
}

pub fn sharpen(doc: &mut PhotoEngineDocument, amount: f32) {
    unsharp_mask(doc, 1, amount);
}

pub fn unsharp_mask(doc: &mut PhotoEngineDocument, radius: u32, amount: f32) {
    apply_raster_filter(doc, |src| {
        let blur = box_blur(src, radius);
        let mut out = src.clone();
        for y in 0..src.height {
            for x in 0..src.width {
                let a = src.get(x, y);
                let b = blur.get(x, y);
                out.set(x, y, [
                    clamp_u8(a[0] as f32 + (a[0] as f32 - b[0] as f32) * amount),
                    clamp_u8(a[1] as f32 + (a[1] as f32 - b[1] as f32) * amount),
                    clamp_u8(a[2] as f32 + (a[2] as f32 - b[2] as f32) * amount),
                    a[3],
                ]);
            }
        }
        out
    });
}

pub fn high_pass(doc: &mut PhotoEngineDocument, radius: u32) {
    apply_raster_filter(doc, |src| {
        let blur = box_blur(src, radius);
        let mut out = src.clone();
        for y in 0..src.height {
            for x in 0..src.width {
                let a = src.get(x, y);
                let b = blur.get(x, y);
                out.set(x, y, [
                    clamp_u8(a[0] as f32 - b[0] as f32 + 128.0),
                    clamp_u8(a[1] as f32 - b[1] as f32 + 128.0),
                    clamp_u8(a[2] as f32 - b[2] as f32 + 128.0),
                    a[3],
                ]);
            }
        }
        out
    });
}

pub fn add_noise(doc: &mut PhotoEngineDocument, amount: f32, monochrome: bool) {
    let mut seed = 0x12345678u32;
    apply_to_selected_raster(doc, move |px, x, y| {
        seed ^= x.wrapping_mul(747796405).wrapping_add(y.wrapping_mul(2891336453));
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed as f32 / u32::MAX as f32 - 0.5) * amount * 255.0
        };
        if monochrome {
            let n = rnd();
            [clamp_u8(px[0] as f32 + n), clamp_u8(px[1] as f32 + n), clamp_u8(px[2] as f32 + n), px[3]]
        } else {
            [clamp_u8(px[0] as f32 + rnd()), clamp_u8(px[1] as f32 + rnd()), clamp_u8(px[2] as f32 + rnd()), px[3]]
        }
    });
}

pub fn median(doc: &mut PhotoEngineDocument, radius: u32) {
    apply_raster_filter(doc, |src| {
        let mut out = src.clone();
        let r = radius as i32;
        let mut vals = Vec::<u8>::new();
        for y in 0..src.height as i32 {
            for x in 0..src.width as i32 {
                let mut c = [0u8; 4];
                for ch in 0..4 {
                    vals.clear();
                    for yy in y - r..=y + r {
                        for xx in x - r..=x + r {
                            if src.in_bounds(xx, yy) { vals.push(src.get(xx as u32, yy as u32)[ch]); }
                        }
                    }
                    vals.sort_unstable();
                    c[ch] = vals[vals.len() / 2];
                }
                out.set(x as u32, y as u32, c);
            }
        }
        out
    });
}

pub fn mosaic(doc: &mut PhotoEngineDocument, cell: u32) {
    let cell = cell.max(1);
    apply_raster_filter(doc, |src| {
        let mut out = src.clone();
        let mut y = 0;
        while y < src.height {
            let mut x = 0;
            while x < src.width {
                let c = average_rect(src, x, y, cell, cell);
                for yy in y..(y + cell).min(src.height) {
                    for xx in x..(x + cell).min(src.width) {
                        out.set(xx, yy, c);
                    }
                }
                x += cell;
            }
            y += cell;
        }
        out
    });
}

pub fn oil_paint_like(doc: &mut PhotoEngineDocument, radius: u32) {
    median(doc, radius);
    sharpen(doc, 0.6);
}

pub fn twirl(doc: &mut PhotoEngineDocument, strength: f32) {
    apply_raster_filter(doc, |src| {
        let mut out = PixelBuffer::new(src.width, src.height);
        let cx = src.width as f32 / 2.0;
        let cy = src.height as f32 / 2.0;
        let max_r = cx.min(cy).max(1.0);
        for y in 0..src.height {
            for x in 0..src.width {
                let dx = x as f32 - cx;
                let dy = y as f32 - cy;
                let r = (dx * dx + dy * dy).sqrt();
                let a = dy.atan2(dx) + strength * (1.0 - (r / max_r).min(1.0));
                let sx = cx + r * a.cos();
                let sy = cy + r * a.sin();
                out.set(x, y, crate::photo_engine::types::bilinear_sample(src, sx, sy));
            }
        }
        out
    });
}

pub fn ripple(doc: &mut PhotoEngineDocument, amplitude: f32, wavelength: f32) {
    let wavelength = wavelength.max(1.0);
    apply_raster_filter(doc, |src| {
        let mut out = PixelBuffer::new(src.width, src.height);
        for y in 0..src.height {
            for x in 0..src.width {
                let sx = x as f32 + (y as f32 / wavelength).sin() * amplitude;
                let sy = y as f32 + (x as f32 / wavelength).sin() * amplitude;
                out.set(x, y, crate::photo_engine::types::bilinear_sample(src, sx, sy));
            }
        }
        out
    });
}

pub fn box_blur(src: &PixelBuffer, radius: u32) -> PixelBuffer {
    if radius == 0 { return src.clone(); }
    let mut out = src.clone();
    let r = radius as i32;
    for y in 0..src.height as i32 {
        for x in 0..src.width as i32 {
            let mut sum = [0u32; 4];
            let mut count = 0u32;
            for yy in y - r..=y + r {
                for xx in x - r..=x + r {
                    if src.in_bounds(xx, yy) {
                        let c = src.get(xx as u32, yy as u32);
                        for i in 0..4 { sum[i] += c[i] as u32; }
                        count += 1;
                    }
                }
            }
            let d = count.max(1);
            out.set(x as u32, y as u32, [(sum[0] / d) as u8, (sum[1] / d) as u8, (sum[2] / d) as u8, (sum[3] / d) as u8]);
        }
    }
    out
}

fn apply_raster_filter<F: FnOnce(&PixelBuffer) -> PixelBuffer>(doc: &mut PhotoEngineDocument, f: F) {
    let selection = doc.selection.mask.clone();
    let Some(layer) = doc.selected_layer_mut() else { return; };
    if layer.locked { return; }
    if !matches!(&layer.kind, LayerKind::Raster(_)) { return; }
    doc.push_history();
    let layer = doc.selected_layer_mut().unwrap();
    if let LayerKind::Raster(img) = &mut layer.kind {
        let filtered = f(img);
        blend_selected(img, &filtered, selection.as_ref());
    }
}

fn blend_selected(dst: &mut PixelBuffer, src: &PixelBuffer, selection: Option<&crate::photo_engine::types::Mask>) {
    for y in 0..dst.height.min(src.height) {
        for x in 0..dst.width.min(src.width) {
            let cov = selection.map(|m| m.get(x.min(m.width - 1), y.min(m.height - 1))).unwrap_or(255);
            if cov == 255 {
                dst.set(x, y, src.get(x, y));
            } else if cov > 0 {
                let a = dst.get(x, y);
                let b = src.get(x, y);
                let t = cov as f32 / 255.0;
                dst.set(x, y, [
                    clamp_u8(a[0] as f32 * (1.0 - t) + b[0] as f32 * t),
                    clamp_u8(a[1] as f32 * (1.0 - t) + b[1] as f32 * t),
                    clamp_u8(a[2] as f32 * (1.0 - t) + b[2] as f32 * t),
                    clamp_u8(a[3] as f32 * (1.0 - t) + b[3] as f32 * t),
                ]);
            }
        }
    }
}

fn average_rect(src: &PixelBuffer, x: u32, y: u32, w: u32, h: u32) -> [u8; 4] {
    let mut sum = [0u32; 4];
    let mut count = 0u32;
    for yy in y..(y + h).min(src.height) {
        for xx in x..(x + w).min(src.width) {
            let c = src.get(xx, yy);
            for i in 0..4 { sum[i] += c[i] as u32; }
            count += 1;
        }
    }
    let d = count.max(1);
    [(sum[0] / d) as u8, (sum[1] / d) as u8, (sum[2] / d) as u8, (sum[3] / d) as u8]
}

fn color_dist(a: [u8; 4], b: [u8; 4]) -> i32 {
    let dr = a[0] as i32 - b[0] as i32;
    let dg = a[1] as i32 - b[1] as i32;
    let db = a[2] as i32 - b[2] as i32;
    ((dr * dr + dg * dg + db * db) as f32).sqrt() as i32
}
