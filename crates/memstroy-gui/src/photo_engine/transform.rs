use crate::photo_engine::document::resize_mask;
use crate::photo_engine::types::{bilinear_sample, deg_to_rad, LayerKind, PhotoEngineDocument, PixelBuffer, RectI, ResampleMethod};

pub fn move_selected(doc: &mut PhotoEngineDocument, dx: f32, dy: f32) {
    let Some(layer) = doc.selected_layer_mut() else { return; };
    if layer.locked { return; }
    doc.push_history();
    let layer = doc.selected_layer_mut().unwrap();
    layer.x += dx;
    layer.y += dy;
}

pub fn scale_selected(doc: &mut PhotoEngineDocument, sx: f32, sy: f32) {
    let Some(layer) = doc.selected_layer_mut() else { return; };
    if layer.locked { return; }
    doc.push_history();
    let layer = doc.selected_layer_mut().unwrap();
    layer.scale_x *= sx;
    layer.scale_y *= sy;
}

pub fn rotate_selected(doc: &mut PhotoEngineDocument, degrees: f32) {
    let Some(layer) = doc.selected_layer_mut() else { return; };
    if layer.locked { return; }
    doc.push_history();
    doc.selected_layer_mut().unwrap().rotation_degrees += degrees;
}

pub fn flip_selected_horizontal(doc: &mut PhotoEngineDocument) {
    let Some(layer) = doc.selected_layer_mut() else { return; };
    if layer.locked { return; }
    doc.push_history();
    doc.selected_layer_mut().unwrap().scale_x *= -1.0;
}

pub fn flip_selected_vertical(doc: &mut PhotoEngineDocument) {
    let Some(layer) = doc.selected_layer_mut() else { return; };
    if layer.locked { return; }
    doc.push_history();
    doc.selected_layer_mut().unwrap().scale_y *= -1.0;
}

pub fn reset_selected_transform(doc: &mut PhotoEngineDocument) {
    let Some(layer) = doc.selected_layer_mut() else { return; };
    if layer.locked { return; }
    doc.push_history();
    let layer = doc.selected_layer_mut().unwrap();
    layer.x = 0.0;
    layer.y = 0.0;
    layer.scale_x = 1.0;
    layer.scale_y = 1.0;
    layer.rotation_degrees = 0.0;
    layer.skew_x = 0.0;
    layer.skew_y = 0.0;
}

pub fn rasterize_selected_transform(doc: &mut PhotoEngineDocument, method: ResampleMethod) {
    let canvas_w = doc.width;
    let canvas_h = doc.height;
    let Some(layer) = doc.selected_layer_mut() else { return; };
    if layer.locked { return; }
    doc.push_history();
    let layer = doc.selected_layer_mut().unwrap();
    let source = layer.rasterize_placeholder();
    let mut out = PixelBuffer::new(canvas_w, canvas_h);
    crate::photo_engine::document::blit_transformed(&mut out, &source, layer);
    layer.x = 0.0;
    layer.y = 0.0;
    layer.scale_x = 1.0;
    layer.scale_y = 1.0;
    layer.rotation_degrees = 0.0;
    layer.skew_x = 0.0;
    layer.skew_y = 0.0;
    layer.kind = LayerKind::Raster(match method { ResampleMethod::Nearest => out, ResampleMethod::Bilinear => out });
    layer.mask = None;
}

pub fn rotate_image_destructive(img: &PixelBuffer, degrees: f32) -> PixelBuffer {
    let angle = deg_to_rad(degrees);
    let cos_a = angle.cos();
    let sin_a = angle.sin();
    let corners = [
        (0.0, 0.0),
        (img.width as f32, 0.0),
        (0.0, img.height as f32),
        (img.width as f32, img.height as f32),
    ];
    let cx = img.width as f32 / 2.0;
    let cy = img.height as f32 / 2.0;
    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;
    for (x, y) in corners {
        let dx = x - cx;
        let dy = y - cy;
        let rx = cos_a * dx - sin_a * dy;
        let ry = sin_a * dx + cos_a * dy;
        min_x = min_x.min(rx);
        max_x = max_x.max(rx);
        min_y = min_y.min(ry);
        max_y = max_y.max(ry);
    }
    let w = (max_x - min_x).ceil().max(1.0) as u32;
    let h = (max_y - min_y).ceil().max(1.0) as u32;
    let mut out = PixelBuffer::new(w, h);
    let ocx = w as f32 / 2.0;
    let ocy = h as f32 / 2.0;
    for y in 0..h {
        for x in 0..w {
            let dx = x as f32 - ocx;
            let dy = y as f32 - ocy;
            let sx = cos_a * dx + sin_a * dy + cx;
            let sy = -sin_a * dx + cos_a * dy + cy;
            out.set(x, y, bilinear_sample(img, sx, sy));
        }
    }
    out
}

pub fn skew_image(img: &PixelBuffer, skew_x: f32, skew_y: f32) -> PixelBuffer {
    let add_w = (img.height as f32 * skew_x.abs()).ceil() as u32;
    let add_h = (img.width as f32 * skew_y.abs()).ceil() as u32;
    let mut out = PixelBuffer::new(img.width + add_w, img.height + add_h);
    for y in 0..out.height {
        for x in 0..out.width {
            let sx = x as f32 - y as f32 * skew_x - if skew_x < 0.0 { add_w as f32 } else { 0.0 };
            let sy = y as f32 - x as f32 * skew_y - if skew_y < 0.0 { add_h as f32 } else { 0.0 };
            out.set(x, y, bilinear_sample(img, sx, sy));
        }
    }
    out
}

pub fn warp_bulge(img: &PixelBuffer, center: (f32, f32), radius: f32, strength: f32) -> PixelBuffer {
    let mut out = PixelBuffer::new(img.width, img.height);
    let r = radius.max(1.0);
    for y in 0..img.height {
        for x in 0..img.width {
            let dx = x as f32 - center.0;
            let dy = y as f32 - center.1;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist < r {
                let t = dist / r;
                let factor = 1.0 + strength * (1.0 - t).powi(2);
                out.set(x, y, bilinear_sample(img, center.0 + dx / factor, center.1 + dy / factor));
            } else {
                out.set(x, y, img.get(x, y));
            }
        }
    }
    out
}

pub fn free_crop_layer_pixels(doc: &mut PhotoEngineDocument, rect: RectI) {
    let Some(layer) = doc.selected_layer_mut() else { return; };
    if layer.locked { return; }
    if !matches!(&layer.kind, LayerKind::Raster(_)) { return; }
    doc.push_history();
    let layer = doc.selected_layer_mut().unwrap();
    if let LayerKind::Raster(img) = &mut layer.kind {
        *img = img.sub_image(rect);
    }
    if let Some(mask) = &mut layer.mask {
        let buf = PixelBuffer { width: mask.width, height: mask.height, data: mask.data.iter().flat_map(|v| [*v, *v, *v, 255]).collect() };
        let sub = buf.sub_image(rect);
        *mask = crate::photo_engine::types::Mask { width: sub.width, height: sub.height, data: sub.data.chunks_exact(4).map(|p| p[0]).collect() };
    }
}
