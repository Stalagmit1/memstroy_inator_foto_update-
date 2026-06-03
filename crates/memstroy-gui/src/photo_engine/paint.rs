use crate::photo_engine::blend::alpha_over;
use crate::photo_engine::types::{clamp01, clamp_u8, LayerKind, PhotoEngineDocument, PixelBuffer};

#[derive(Clone, Copy, Debug)]
pub struct Brush {
    pub radius: f32,
    pub hardness: f32,
    pub opacity: f32,
    pub flow: f32,
    pub color: [u8; 4],
}

impl Default for Brush {
    fn default() -> Self {
        Self { radius: 16.0, hardness: 0.8, opacity: 1.0, flow: 1.0, color: [255, 255, 255, 255] }
    }
}

pub fn brush_stroke(doc: &mut PhotoEngineDocument, points: &[(f32, f32)], brush: Brush) {
    edit_selected_raster(doc, |img, selection| {
        for pair in points.windows(2) {
            draw_line(img, selection, pair[0], pair[1], brush, PaintMode::Normal);
        }
        if points.len() == 1 {
            stamp(img, selection, points[0].0, points[0].1, brush, PaintMode::Normal);
        }
    });
}

pub fn eraser_stroke(doc: &mut PhotoEngineDocument, points: &[(f32, f32)], radius: f32, opacity: f32) {
    let brush = Brush { radius, hardness: 0.8, opacity, flow: 1.0, color: [0, 0, 0, 0] };
    edit_selected_raster(doc, |img, selection| {
        for pair in points.windows(2) {
            draw_line(img, selection, pair[0], pair[1], brush, PaintMode::Erase);
        }
    });
}

pub fn dodge_burn_stroke(doc: &mut PhotoEngineDocument, points: &[(f32, f32)], radius: f32, exposure: f32, dodge: bool) {
    let brush = Brush { radius, hardness: 0.7, opacity: exposure.abs(), flow: 1.0, color: [255, 255, 255, 255] };
    edit_selected_raster(doc, |img, selection| {
        for pair in points.windows(2) {
            draw_line(img, selection, pair[0], pair[1], brush, if dodge { PaintMode::Dodge } else { PaintMode::Burn });
        }
    });
}

pub fn sponge_stroke(doc: &mut PhotoEngineDocument, points: &[(f32, f32)], radius: f32, amount: f32) {
    let brush = Brush { radius, hardness: 0.7, opacity: amount.abs(), flow: 1.0, color: [255, 255, 255, 255] };
    edit_selected_raster(doc, |img, selection| {
        for pair in points.windows(2) {
            draw_line(img, selection, pair[0], pair[1], brush, if amount >= 0.0 { PaintMode::Saturate } else { PaintMode::Desaturate });
        }
    });
}

pub fn clone_stamp(doc: &mut PhotoEngineDocument, source: (f32, f32), target_points: &[(f32, f32)], radius: f32, opacity: f32) {
    let selection = doc.selection.mask.clone();
    let Some(layer) = doc.selected_layer_mut() else { return; };
    if layer.locked { return; }
    if !matches!(&layer.kind, LayerKind::Raster(_)) { return; }
    doc.push_history();
    let layer = doc.selected_layer_mut().unwrap();
    if let LayerKind::Raster(img) = &mut layer.kind {
        let snapshot = img.clone();
        if target_points.is_empty() { return; }
        let first = target_points[0];
        let offset = (source.0 - first.0, source.1 - first.1);
        let brush = Brush { radius, hardness: 0.8, opacity, flow: 1.0, color: [0, 0, 0, 0] };
        for &p in target_points {
            clone_stamp_at(img, selection.as_ref(), &snapshot, p.0, p.1, offset, brush);
        }
    }
}

pub fn healing_brush(doc: &mut PhotoEngineDocument, source: (f32, f32), target_points: &[(f32, f32)], radius: f32, opacity: f32) {
    clone_stamp(doc, source, target_points, radius, opacity * 0.55);
}

pub fn paint_bucket(doc: &mut PhotoEngineDocument, x: u32, y: u32, color: [u8; 4], tolerance: u8, contiguous: bool) {
    edit_selected_raster(doc, |img, selection| {
        if x >= img.width || y >= img.height { return; }
        let target = img.get(x, y);
        if contiguous {
            let mut seen = vec![false; (img.width * img.height) as usize];
            let mut stack = vec![(x, y)];
            while let Some((cx, cy)) = stack.pop() {
                if cx >= img.width || cy >= img.height { continue; }
                let idx = (cy * img.width + cx) as usize;
                if seen[idx] { continue; }
                seen[idx] = true;
                if dist(img.get(cx, cy), target) <= tolerance as i32 && selected(selection, cx, cy) > 0.0 {
                    img.set(cx, cy, color);
                    if cx > 0 { stack.push((cx - 1, cy)); }
                    if cy > 0 { stack.push((cx, cy - 1)); }
                    if cx + 1 < img.width { stack.push((cx + 1, cy)); }
                    if cy + 1 < img.height { stack.push((cx, cy + 1)); }
                }
            }
        } else {
            for yy in 0..img.height {
                for xx in 0..img.width {
                    if dist(img.get(xx, yy), target) <= tolerance as i32 && selected(selection, xx, yy) > 0.0 {
                        img.set(xx, yy, color);
                    }
                }
            }
        }
    });
}

pub fn gradient_fill(doc: &mut PhotoEngineDocument, start: (f32, f32), end: (f32, f32), a: [u8; 4], b: [u8; 4]) {
    edit_selected_raster(doc, |img, selection| {
        let vx = end.0 - start.0;
        let vy = end.1 - start.1;
        let denom = (vx * vx + vy * vy).max(0.0001);
        for y in 0..img.height {
            for x in 0..img.width {
                let t = (((x as f32 - start.0) * vx + (y as f32 - start.1) * vy) / denom).max(0.0).min(1.0);
                let cov = selected(selection, x, y);
                if cov <= 0.0 { continue; }
                let c = [
                    clamp_u8(a[0] as f32 * (1.0 - t) + b[0] as f32 * t),
                    clamp_u8(a[1] as f32 * (1.0 - t) + b[1] as f32 * t),
                    clamp_u8(a[2] as f32 * (1.0 - t) + b[2] as f32 * t),
                    clamp_u8(a[3] as f32 * (1.0 - t) + b[3] as f32 * t),
                ];
                let old = img.get(x, y);
                img.set(x, y, alpha_over(old, c, cov, crate::photo_engine::types::BlendMode::Normal));
            }
        }
    });
}

#[derive(Clone, Copy)]
enum PaintMode { Normal, Erase, Dodge, Burn, Saturate, Desaturate }

fn edit_selected_raster<F: FnOnce(&mut PixelBuffer, Option<&crate::photo_engine::types::Mask>)>(doc: &mut PhotoEngineDocument, f: F) {
    let selection = doc.selection.mask.clone();
    let Some(layer) = doc.selected_layer_mut() else { return; };
    if layer.locked { return; }
    if !matches!(&layer.kind, LayerKind::Raster(_)) { return; }
    doc.push_history();
    let layer = doc.selected_layer_mut().unwrap();
    if let LayerKind::Raster(img) = &mut layer.kind {
        f(img, selection.as_ref());
    }
}

fn draw_line(img: &mut PixelBuffer, selection: Option<&crate::photo_engine::types::Mask>, a: (f32, f32), b: (f32, f32), brush: Brush, mode: PaintMode) {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let steps = dx.abs().max(dy.abs()).ceil().max(1.0) as i32;
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        stamp(img, selection, a.0 + dx * t, a.1 + dy * t, brush, mode);
    }
}

fn stamp(img: &mut PixelBuffer, selection: Option<&crate::photo_engine::types::Mask>, cx: f32, cy: f32, brush: Brush, mode: PaintMode) {
    let r = brush.radius.max(0.5);
    let min_x = (cx - r).floor() as i32;
    let max_x = (cx + r).ceil() as i32;
    let min_y = (cy - r).floor() as i32;
    let max_y = (cy + r).ceil() as i32;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            if !img.in_bounds(x, y) { continue; }
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            let d = (dx * dx + dy * dy).sqrt() / r;
            if d > 1.0 { continue; }
            let soft = if d <= brush.hardness { 1.0 } else { 1.0 - (d - brush.hardness) / (1.0 - brush.hardness).max(0.0001) };
            let cov = selected(selection, x as u32, y as u32) * soft * brush.opacity * brush.flow;
            if cov <= 0.0 { continue; }
            let old = img.get(x as u32, y as u32);
            let new = match mode {
                PaintMode::Normal => alpha_over(old, brush.color, cov, crate::photo_engine::types::BlendMode::Normal),
                PaintMode::Erase => { let mut c = old; c[3] = clamp_u8(c[3] as f32 * (1.0 - cov)); c },
                PaintMode::Dodge => [clamp_u8(old[0] as f32 + 255.0 * cov), clamp_u8(old[1] as f32 + 255.0 * cov), clamp_u8(old[2] as f32 + 255.0 * cov), old[3]],
                PaintMode::Burn => [clamp_u8(old[0] as f32 * (1.0 - cov)), clamp_u8(old[1] as f32 * (1.0 - cov)), clamp_u8(old[2] as f32 * (1.0 - cov)), old[3]],
                PaintMode::Saturate => adjust_sat(old, cov),
                PaintMode::Desaturate => adjust_sat(old, -cov),
            };
            img.set(x as u32, y as u32, new);
        }
    }
}

fn clone_stamp_at(img: &mut PixelBuffer, selection: Option<&crate::photo_engine::types::Mask>, src: &PixelBuffer, tx: f32, ty: f32, offset: (f32, f32), brush: Brush) {
    let r = brush.radius.max(0.5);
    for y in (ty - r).floor() as i32..=(ty + r).ceil() as i32 {
        for x in (tx - r).floor() as i32..=(tx + r).ceil() as i32 {
            if !img.in_bounds(x, y) { continue; }
            let dx = x as f32 + 0.5 - tx;
            let dy = y as f32 + 0.5 - ty;
            if (dx * dx + dy * dy).sqrt() > r { continue; }
            let sx = (x as f32 + offset.0).round() as i32;
            let sy = (y as f32 + offset.1).round() as i32;
            if !src.in_bounds(sx, sy) { continue; }
            let cov = selected(selection, x as u32, y as u32) * brush.opacity;
            let old = img.get(x as u32, y as u32);
            let sample = src.get(sx as u32, sy as u32);
            img.set(x as u32, y as u32, alpha_over(old, sample, cov, crate::photo_engine::types::BlendMode::Normal));
        }
    }
}

fn selected(selection: Option<&crate::photo_engine::types::Mask>, x: u32, y: u32) -> f32 {
    selection.map(|m| m.get(x.min(m.width - 1), y.min(m.height - 1)) as f32 / 255.0).unwrap_or(1.0)
}

fn adjust_sat(px: [u8; 4], amount: f32) -> [u8; 4] {
    let (h, s, l) = crate::photo_engine::types::rgba_to_hsl(px);
    crate::photo_engine::types::hsl_to_rgba(h, clamp01(s + amount), l, px[3])
}

fn dist(a: [u8; 4], b: [u8; 4]) -> i32 {
    let dr = a[0] as i32 - b[0] as i32;
    let dg = a[1] as i32 - b[1] as i32;
    let db = a[2] as i32 - b[2] as i32;
    ((dr * dr + dg * dg + db * db) as f32).sqrt() as i32
}
