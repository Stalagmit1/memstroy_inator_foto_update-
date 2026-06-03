use crate::photo_engine::types::{clamp01, clamp_u8, hsl_to_rgba, rgba_to_hsl, LayerKind, PhotoEngineDocument, PixelBuffer};

pub fn apply_to_selected_raster<F: FnMut([u8; 4], u32, u32) -> [u8; 4]>(doc: &mut PhotoEngineDocument, mut f: F) {
    let selection = doc.selection.mask.clone();
    let Some(layer) = doc.selected_layer_mut() else { return; };
    if layer.locked { return; }
    if !matches!(&layer.kind, LayerKind::Raster(_)) { return; }
    doc.push_history();
    let selection = selection;
    let layer = doc.selected_layer_mut().unwrap();
    if let LayerKind::Raster(img) = &mut layer.kind {
        for y in 0..img.height {
            for x in 0..img.width {
                let cov = selection.as_ref().map(|m| m.get(x.min(m.width - 1), y.min(m.height - 1))).unwrap_or(255);
                if cov == 0 { continue; }
                let old = img.get(x, y);
                let new = f(old, x, y);
                img.set(x, y, mix_rgba(old, new, cov as f32 / 255.0));
            }
        }
    }
}

pub fn invert(doc: &mut PhotoEngineDocument) {
    apply_to_selected_raster(doc, |c, _, _| [255 - c[0], 255 - c[1], 255 - c[2], c[3]]);
}

pub fn grayscale(doc: &mut PhotoEngineDocument) {
    apply_to_selected_raster(doc, |c, _, _| {
        let y = clamp_u8(c[0] as f32 * 0.2126 + c[1] as f32 * 0.7152 + c[2] as f32 * 0.0722);
        [y, y, y, c[3]]
    });
}

pub fn brightness_contrast(doc: &mut PhotoEngineDocument, brightness: f32, contrast: f32) {
    let b = brightness * 255.0;
    let c = (1.0 + contrast).max(0.0);
    apply_to_selected_raster(doc, move |px, _, _| {
        [
            clamp_u8((px[0] as f32 - 128.0) * c + 128.0 + b),
            clamp_u8((px[1] as f32 - 128.0) * c + 128.0 + b),
            clamp_u8((px[2] as f32 - 128.0) * c + 128.0 + b),
            px[3],
        ]
    });
}

pub fn levels(doc: &mut PhotoEngineDocument, black: u8, gamma: f32, white: u8) {
    let black = black as f32;
    let white = (white as f32).max(black + 1.0);
    let inv_gamma = 1.0 / gamma.max(0.01);
    apply_to_selected_raster(doc, move |px, _, _| {
        let map = |v: u8| -> u8 {
            let n = ((v as f32 - black) / (white - black)).max(0.0).min(1.0);
            clamp_u8(n.powf(inv_gamma) * 255.0)
        };
        [map(px[0]), map(px[1]), map(px[2]), px[3]]
    });
}

pub fn exposure(doc: &mut PhotoEngineDocument, exposure_stops: f32, gamma: f32) {
    let gain = 2.0_f32.powf(exposure_stops);
    let inv_gamma = 1.0 / gamma.max(0.01);
    apply_to_selected_raster(doc, move |px, _, _| {
        let map = |v: u8| clamp_u8(((v as f32 / 255.0 * gain).max(0.0).min(1.0)).powf(inv_gamma) * 255.0);
        [map(px[0]), map(px[1]), map(px[2]), px[3]]
    });
}

pub fn hue_saturation_lightness(doc: &mut PhotoEngineDocument, hue_degrees: f32, saturation_delta: f32, lightness_delta: f32) {
    let hd = hue_degrees / 360.0;
    apply_to_selected_raster(doc, move |px, _, _| {
        let (h, s, l) = rgba_to_hsl(px);
        hsl_to_rgba((h + hd).rem_euclid(1.0), clamp01(s + saturation_delta), clamp01(l + lightness_delta), px[3])
    });
}

pub fn vibrance(doc: &mut PhotoEngineDocument, amount: f32) {
    apply_to_selected_raster(doc, move |px, _, _| {
        let (h, s, l) = rgba_to_hsl(px);
        let boost = amount * (1.0 - s);
        hsl_to_rgba(h, clamp01(s + boost), l, px[3])
    });
}

pub fn threshold(doc: &mut PhotoEngineDocument, level: u8) {
    apply_to_selected_raster(doc, move |px, _, _| {
        let y = (px[0] as u16 * 54 + px[1] as u16 * 183 + px[2] as u16 * 19) / 256;
        let v = if y as u8 >= level { 255 } else { 0 };
        [v, v, v, px[3]]
    });
}

pub fn posterize(doc: &mut PhotoEngineDocument, levels: u8) {
    let levels = levels.max(2) as f32;
    apply_to_selected_raster(doc, move |px, _, _| {
        let map = |v: u8| clamp_u8(((v as f32 / 255.0 * (levels - 1.0)).round() / (levels - 1.0)) * 255.0);
        [map(px[0]), map(px[1]), map(px[2]), px[3]]
    });
}

pub fn color_balance(doc: &mut PhotoEngineDocument, cyan_red: f32, magenta_green: f32, yellow_blue: f32) {
    apply_to_selected_raster(doc, move |px, _, _| {
        [
            clamp_u8(px[0] as f32 + cyan_red * 255.0),
            clamp_u8(px[1] as f32 + magenta_green * 255.0),
            clamp_u8(px[2] as f32 + yellow_blue * 255.0),
            px[3],
        ]
    });
}

pub fn gradient_map(doc: &mut PhotoEngineDocument, dark: [u8; 4], light: [u8; 4]) {
    apply_to_selected_raster(doc, move |px, _, _| {
        let t = (px[0] as f32 * 0.2126 + px[1] as f32 * 0.7152 + px[2] as f32 * 0.0722) / 255.0;
        [
            clamp_u8(dark[0] as f32 * (1.0 - t) + light[0] as f32 * t),
            clamp_u8(dark[1] as f32 * (1.0 - t) + light[1] as f32 * t),
            clamp_u8(dark[2] as f32 * (1.0 - t) + light[2] as f32 * t),
            px[3],
        ]
    });
}

pub fn replace_color(doc: &mut PhotoEngineDocument, from: [u8; 4], to: [u8; 4], tolerance: u8) {
    apply_to_selected_raster(doc, move |px, _, _| {
        let dr = px[0] as i32 - from[0] as i32;
        let dg = px[1] as i32 - from[1] as i32;
        let db = px[2] as i32 - from[2] as i32;
        let d = ((dr * dr + dg * dg + db * db) as f32).sqrt();
        if d <= tolerance as f32 { [to[0], to[1], to[2], px[3]] } else { px }
    });
}

pub fn match_average_color(source: &PixelBuffer, target: &mut PixelBuffer) {
    let a = average_rgb(source);
    let b = average_rgb(target);
    for y in 0..target.height {
        for x in 0..target.width {
            let p = target.get(x, y);
            target.set(x, y, [
                clamp_u8(p[0] as f32 + a[0] - b[0]),
                clamp_u8(p[1] as f32 + a[1] - b[1]),
                clamp_u8(p[2] as f32 + a[2] - b[2]),
                p[3],
            ]);
        }
    }
}

fn average_rgb(img: &PixelBuffer) -> [f32; 3] {
    let mut sum = [0f64; 3];
    let mut count = 0f64;
    for px in img.data.chunks_exact(4) {
        let a = px[3] as f64 / 255.0;
        sum[0] += px[0] as f64 * a;
        sum[1] += px[1] as f64 * a;
        sum[2] += px[2] as f64 * a;
        count += a;
    }
    let d = count.max(1.0);
    [(sum[0] / d) as f32, (sum[1] / d) as f32, (sum[2] / d) as f32]
}

fn mix_rgba(a: [u8; 4], b: [u8; 4], t: f32) -> [u8; 4] {
    let t = clamp01(t);
    [
        clamp_u8(a[0] as f32 * (1.0 - t) + b[0] as f32 * t),
        clamp_u8(a[1] as f32 * (1.0 - t) + b[1] as f32 * t),
        clamp_u8(a[2] as f32 * (1.0 - t) + b[2] as f32 * t),
        clamp_u8(a[3] as f32 * (1.0 - t) + b[3] as f32 * t),
    ]
}
