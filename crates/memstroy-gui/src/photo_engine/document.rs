use crate::photo_engine::blend::alpha_over;
use crate::photo_engine::effects::apply_layer_effects;
use crate::photo_engine::types::*;

impl PhotoEngineDocument {
    pub fn add_raster_layer(&mut self, name: impl Into<String>, pixels: PixelBuffer) -> String {
        self.push_history();
        let id = self.next_id("raster");
        let layer = Layer::raster(id.clone(), name, pixels);
        self.layers.push(layer);
        self.selected_layer = Some(id.clone());
        id
    }

    pub fn add_empty_raster_layer(&mut self, name: impl Into<String>) -> String {
        self.add_raster_layer(name, PixelBuffer::new(self.width, self.height))
    }

    pub fn add_text_layer(&mut self, text: impl Into<String>, x: f32, y: f32) -> String {
        self.push_history();
        let id = self.next_id("text");
        let layer = Layer {
            id: id.clone(),
            name: "Text Layer".to_string(),
            visible: true,
            locked: false,
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            x,
            y,
            scale_x: 1.0,
            scale_y: 1.0,
            rotation_degrees: 0.0,
            skew_x: 0.0,
            skew_y: 0.0,
            mask: None,
            clip_to_below: false,
            effects: Vec::new(),
            kind: LayerKind::TextPlaceholder { text: text.into(), size: 64.0, color: [255, 255, 255, 255] },
        };
        self.layers.push(layer);
        self.selected_layer = Some(id.clone());
        id
    }

    pub fn add_shape_rect(&mut self, width: u32, height: u32, fill: [u8; 4], x: f32, y: f32) -> String {
        self.push_history();
        let id = self.next_id("shape");
        let layer = Layer {
            id: id.clone(),
            name: "Rectangle".to_string(),
            visible: true,
            locked: false,
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            x,
            y,
            scale_x: 1.0,
            scale_y: 1.0,
            rotation_degrees: 0.0,
            skew_x: 0.0,
            skew_y: 0.0,
            mask: None,
            clip_to_below: false,
            effects: Vec::new(),
            kind: LayerKind::ShapeRect { width, height, fill, stroke: [0, 0, 0, 0], stroke_width: 0 },
        };
        self.layers.push(layer);
        self.selected_layer = Some(id.clone());
        id
    }

    pub fn add_shape_ellipse(&mut self, width: u32, height: u32, fill: [u8; 4], x: f32, y: f32) -> String {
        self.push_history();
        let id = self.next_id("shape");
        let layer = Layer {
            id: id.clone(),
            name: "Ellipse".to_string(),
            visible: true,
            locked: false,
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            x,
            y,
            scale_x: 1.0,
            scale_y: 1.0,
            rotation_degrees: 0.0,
            skew_x: 0.0,
            skew_y: 0.0,
            mask: None,
            clip_to_below: false,
            effects: Vec::new(),
            kind: LayerKind::ShapeEllipse { width, height, fill, stroke: [0, 0, 0, 0], stroke_width: 0 },
        };
        self.layers.push(layer);
        self.selected_layer = Some(id.clone());
        id
    }

    pub fn delete_selected_layer(&mut self) {
        let Some(id) = self.selected_layer.clone() else { return; };
        self.push_history();
        self.layers.retain(|l| l.id != id);
        self.selected_layer = self.layers.last().map(|l| l.id.clone());
    }

    pub fn duplicate_selected_layer(&mut self) -> Option<String> {
        let id = self.selected_layer.clone()?;
        let source = self.layers.iter().find(|l| l.id == id)?.clone();
        self.push_history();
        let mut copy = source;
        copy.id = self.next_id("copy");
        copy.name = format!("{} Copy", copy.name);
        copy.x += 24.0;
        copy.y += 24.0;
        let new_id = copy.id.clone();
        self.layers.push(copy);
        self.selected_layer = Some(new_id.clone());
        Some(new_id)
    }

    pub fn move_selected_layer(&mut self, delta: i32) {
        let Some(id) = self.selected_layer.clone() else { return; };
        let Some(idx) = self.layers.iter().position(|l| l.id == id) else { return; };
        let new_idx = if delta > 0 { (idx + delta as usize).min(self.layers.len().saturating_sub(1)) } else { idx.saturating_sub((-delta) as usize) };
        if idx != new_idx {
            self.push_history();
            self.layers.swap(idx, new_idx);
        }
    }

    pub fn flatten(&mut self) {
        self.push_history();
        let rendered = self.render();
        self.layers.clear();
        let id = self.next_id("flattened");
        self.layers.push(Layer::raster(id.clone(), "Flattened", rendered));
        self.selected_layer = Some(id);
    }

    pub fn merge_selected_down(&mut self) {
        let Some(id) = self.selected_layer.clone() else { return; };
        let Some(idx) = self.layers.iter().position(|l| l.id == id) else { return; };
        if idx == 0 { return; }
        self.push_history();
        let upper = self.render_single_layer(idx);
        let lower = self.render_single_layer(idx - 1);
        let mut merged = lower.clone();
        for y in 0..merged.height {
            for x in 0..merged.width {
                let d = merged.get(x, y);
                let s = upper.get(x, y);
                merged.set(x, y, alpha_over(d, s, 1.0, BlendMode::Normal));
            }
        }
        let lower_name = self.layers[idx - 1].name.clone();
        let lower_id = self.layers[idx - 1].id.clone();
        self.layers.remove(idx);
        self.layers[idx - 1] = Layer::raster(lower_id.clone(), lower_name, merged);
        self.selected_layer = Some(lower_id);
    }

    pub fn resize_image(&mut self, width: u32, height: u32, method: ResampleMethod) {
        self.push_history();
        let sx = width as f32 / self.width.max(1) as f32;
        let sy = height as f32 / self.height.max(1) as f32;
        self.width = width.max(1);
        self.height = height.max(1);
        for layer in &mut self.layers {
            layer.x *= sx;
            layer.y *= sy;
            layer.scale_x *= sx;
            layer.scale_y *= sy;
            if let Some(mask) = &mut layer.mask {
                *mask = resize_mask(mask, self.width, self.height);
            }
            if let LayerKind::Raster(p) = &mut layer.kind {
                *p = match method {
                    ResampleMethod::Nearest => p.resize_nearest((p.width as f32 * sx).max(1.0) as u32, (p.height as f32 * sy).max(1.0) as u32),
                    ResampleMethod::Bilinear => p.resize_bilinear((p.width as f32 * sx).max(1.0) as u32, (p.height as f32 * sy).max(1.0) as u32),
                };
            }
        }
        if let Some(mask) = &mut self.selection.mask {
            *mask = resize_mask(mask, self.width, self.height);
        }
    }

    pub fn resize_canvas(&mut self, width: u32, height: u32, anchor_x: f32, anchor_y: f32) {
        self.push_history();
        let dx = (width as f32 - self.width as f32) * anchor_x;
        let dy = (height as f32 - self.height as f32) * anchor_y;
        self.width = width.max(1);
        self.height = height.max(1);
        for layer in &mut self.layers {
            layer.x += dx;
            layer.y += dy;
        }
        self.selection.clear();
    }

    pub fn crop(&mut self, rect: RectI) {
        self.push_history();
        let full = RectI::new(0, 0, self.width as i32, self.height as i32);
        let r = rect.intersect(full).unwrap_or(full);
        self.width = r.w.max(1) as u32;
        self.height = r.h.max(1) as u32;
        for layer in &mut self.layers {
            layer.x -= r.x as f32;
            layer.y -= r.y as f32;
        }
        self.selection.clear();
    }

    pub fn align_selected(&mut self, align: Alignment) {
        let Some(layer) = self.selected_layer_mut() else { return; };
        if layer.locked { return; }
        let Some((w, h)) = layer.raster_size() else { return; };
        self.push_history();
        let doc_w = self.width as f32;
        let doc_h = self.height as f32;
        let layer = self.selected_layer_mut().unwrap();
        let sw = w as f32 * layer.scale_x.abs();
        let sh = h as f32 * layer.scale_y.abs();
        match align {
            Alignment::Left => layer.x = 0.0,
            Alignment::CenterX => layer.x = doc_w * 0.5 - sw * 0.5,
            Alignment::Right => layer.x = doc_w - sw,
            Alignment::Top => layer.y = 0.0,
            Alignment::CenterY => layer.y = doc_h * 0.5 - sh * 0.5,
            Alignment::Bottom => layer.y = doc_h - sh,
        }
    }

    pub fn render(&self) -> PixelBuffer {
        let mut canvas = PixelBuffer::solid(self.width, self.height, self.background);
        for i in 0..self.layers.len() {
            let layer_img = self.render_single_layer(i);
            let layer = &self.layers[i];
            for y in 0..self.height {
                for x in 0..self.width {
                    let dst = canvas.get(x, y);
                    let src = layer_img.get(x, y);
                    canvas.set(x, y, alpha_over(dst, src, layer.opacity, layer.blend_mode));
                }
            }
        }
        canvas
    }

    pub fn render_single_layer(&self, index: usize) -> PixelBuffer {
        let mut canvas = PixelBuffer::new(self.width, self.height);
        if index >= self.layers.len() { return canvas; }
        let layer = &self.layers[index];
        if !layer.visible { return canvas; }
        let mut src = layer.rasterize_placeholder();
        apply_layer_mask(&mut src, layer.mask.as_ref());
        apply_layer_effects(&mut src, &layer.effects);
        blit_transformed(&mut canvas, &src, layer);
        canvas
    }
}

pub fn blit_transformed(dst: &mut PixelBuffer, src: &PixelBuffer, layer: &Layer) {
    let cx = src.width as f32 / 2.0;
    let cy = src.height as f32 / 2.0;
    let angle = deg_to_rad(layer.rotation_degrees);
    let cos_a = angle.cos();
    let sin_a = angle.sin();
    let sx = layer.scale_x.max(0.0001);
    let sy = layer.scale_y.max(0.0001);
    let out_w = (src.width as f32 * sx.abs()).ceil() as i32 + 4;
    let out_h = (src.height as f32 * sy.abs()).ceil() as i32 + 4;
    let radius = ((out_w * out_w + out_h * out_h) as f32).sqrt().ceil() as i32;
    let ox = layer.x as i32 - radius;
    let oy = layer.y as i32 - radius;
    let max_x = layer.x as i32 + radius;
    let max_y = layer.y as i32 + radius;
    for dy in oy..=max_y {
        for dx in ox..=max_x {
            if !dst.in_bounds(dx, dy) { continue; }
            let mut lx = dx as f32 - layer.x - cx * sx;
            let mut ly = dy as f32 - layer.y - cy * sy;
            let rx = cos_a * lx + sin_a * ly;
            let ry = -sin_a * lx + cos_a * ly;
            lx = rx / sx + cx;
            ly = ry / sy + cy;
            let c = bilinear_sample(src, lx, ly);
            if c[3] == 0 { continue; }
            let d = dst.get(dx as u32, dy as u32);
            dst.set(dx as u32, dy as u32, alpha_over(d, c, 1.0, BlendMode::Normal));
        }
    }
}

pub fn apply_layer_mask(img: &mut PixelBuffer, mask: Option<&Mask>) {
    let Some(mask) = mask else { return; };
    for y in 0..img.height {
        for x in 0..img.width {
            let mx = x.min(mask.width.saturating_sub(1));
            let my = y.min(mask.height.saturating_sub(1));
            let m = mask.get(mx, my) as u16;
            let mut c = img.get(x, y);
            c[3] = ((c[3] as u16 * m) / 255) as u8;
            img.set(x, y, c);
        }
    }
}

pub fn resize_mask(mask: &Mask, width: u32, height: u32) -> Mask {
    let mut out = Mask::new(width.max(1), height.max(1), 0);
    for y in 0..out.height {
        for x in 0..out.width {
            let sx = ((x as f32 + 0.5) * mask.width as f32 / out.width as f32).floor() as u32;
            let sy = ((y as f32 + 0.5) * mask.height as f32 / out.height as f32).floor() as u32;
            out.set(x, y, mask.get(sx.min(mask.width - 1), sy.min(mask.height - 1)));
        }
    }
    out
}
