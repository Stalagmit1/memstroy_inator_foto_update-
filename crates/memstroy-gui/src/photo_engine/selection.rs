use crate::photo_engine::types::{Mask, PixelBuffer, PhotoEngineDocument, RectI, SelectionMode};

pub fn select_all(doc: &mut PhotoEngineDocument) {
    doc.push_history();
    doc.selection.mask = Some(Mask::new(doc.width, doc.height, 255));
}

pub fn deselect(doc: &mut PhotoEngineDocument) {
    doc.push_history();
    doc.selection.clear();
}

pub fn invert_selection(doc: &mut PhotoEngineDocument) {
    doc.push_history();
    let mut mask = doc.selection.mask.take().unwrap_or_else(|| Mask::new(doc.width, doc.height, 255));
    mask.invert();
    doc.selection.mask = Some(mask);
}

pub fn rectangle(doc: &mut PhotoEngineDocument, rect: RectI, mode: SelectionMode) {
    let mut m = Mask::new(doc.width, doc.height, 0);
    let full = RectI::new(0, 0, doc.width as i32, doc.height as i32);
    if let Some(r) = rect.intersect(full) {
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                m.set(x as u32, y as u32, 255);
            }
        }
    }
    combine(doc, m, mode);
}

pub fn ellipse(doc: &mut PhotoEngineDocument, rect: RectI, mode: SelectionMode) {
    let mut m = Mask::new(doc.width, doc.height, 0);
    let rx = rect.w as f32 / 2.0;
    let ry = rect.h as f32 / 2.0;
    let cx = rect.x as f32 + rx;
    let cy = rect.y as f32 + ry;
    let full = RectI::new(0, 0, doc.width as i32, doc.height as i32);
    if let Some(r) = rect.intersect(full) {
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                let dx = (x as f32 + 0.5 - cx) / rx.max(1.0);
                let dy = (y as f32 + 0.5 - cy) / ry.max(1.0);
                if dx * dx + dy * dy <= 1.0 {
                    m.set(x as u32, y as u32, 255);
                }
            }
        }
    }
    combine(doc, m, mode);
}

pub fn polygon_lasso(doc: &mut PhotoEngineDocument, points: &[(i32, i32)], mode: SelectionMode) {
    if points.len() < 3 { return; }
    let mut m = Mask::new(doc.width, doc.height, 0);
    let min_x = points.iter().map(|p| p.0).min().unwrap_or(0).max(0);
    let max_x = points.iter().map(|p| p.0).max().unwrap_or(0).min(doc.width as i32 - 1);
    let min_y = points.iter().map(|p| p.1).min().unwrap_or(0).max(0);
    let max_y = points.iter().map(|p| p.1).max().unwrap_or(0).min(doc.height as i32 - 1);
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            if point_in_poly(x as f32 + 0.5, y as f32 + 0.5, points) {
                m.set(x as u32, y as u32, 255);
            }
        }
    }
    combine(doc, m, mode);
}

pub fn magic_wand(doc: &mut PhotoEngineDocument, img: &PixelBuffer, seed_x: u32, seed_y: u32, tolerance: u8, contiguous: bool, mode: SelectionMode) {
    if seed_x >= img.width || seed_y >= img.height { return; }
    let target = img.get(seed_x, seed_y);
    let mut out = Mask::new(doc.width, doc.height, 0);
    if contiguous {
        let mut seen = vec![false; (img.width * img.height) as usize];
        let mut stack = vec![(seed_x, seed_y)];
        while let Some((x, y)) = stack.pop() {
            if x >= img.width || y >= img.height { continue; }
            let i = (y * img.width + x) as usize;
            if seen[i] { continue; }
            seen[i] = true;
            if color_distance(img.get(x, y), target) <= tolerance as i32 {
                if x < doc.width && y < doc.height { out.set(x, y, 255); }
                if x > 0 { stack.push((x - 1, y)); }
                if y > 0 { stack.push((x, y - 1)); }
                if x + 1 < img.width { stack.push((x + 1, y)); }
                if y + 1 < img.height { stack.push((x, y + 1)); }
            }
        }
    } else {
        for y in 0..img.height.min(doc.height) {
            for x in 0..img.width.min(doc.width) {
                if color_distance(img.get(x, y), target) <= tolerance as i32 {
                    out.set(x, y, 255);
                }
            }
        }
    }
    combine(doc, out, mode);
}

pub fn color_range(doc: &mut PhotoEngineDocument, img: &PixelBuffer, color: [u8; 4], tolerance: u8, mode: SelectionMode) {
    let mut out = Mask::new(doc.width, doc.height, 0);
    for y in 0..img.height.min(doc.height) {
        for x in 0..img.width.min(doc.width) {
            if color_distance(img.get(x, y), color) <= tolerance as i32 {
                out.set(x, y, 255);
            }
        }
    }
    combine(doc, out, mode);
}

pub fn feather(doc: &mut PhotoEngineDocument, radius: u32) {
    let Some(mask) = doc.selection.mask.clone() else { return; };
    doc.push_history();
    doc.selection.mask = Some(blur_mask(&mask, radius));
}

pub fn expand(doc: &mut PhotoEngineDocument, pixels: u32) {
    let Some(mask) = doc.selection.mask.clone() else { return; };
    doc.push_history();
    let mut cur = mask;
    for _ in 0..pixels {
        cur = dilate(&cur);
    }
    doc.selection.mask = Some(cur);
}

pub fn contract(doc: &mut PhotoEngineDocument, pixels: u32) {
    let Some(mask) = doc.selection.mask.clone() else { return; };
    doc.push_history();
    let mut cur = mask;
    for _ in 0..pixels {
        cur = erode(&cur);
    }
    doc.selection.mask = Some(cur);
}

fn combine(doc: &mut PhotoEngineDocument, incoming: Mask, mode: SelectionMode) {
    doc.push_history();
    let base = doc.selection.mask.take().unwrap_or_else(|| Mask::new(doc.width, doc.height, 0));
    let mut out = Mask::new(doc.width, doc.height, 0);
    for y in 0..doc.height {
        for x in 0..doc.width {
            let a = base.get(x, y);
            let b = incoming.get(x, y);
            let v = match mode {
                SelectionMode::Replace => b,
                SelectionMode::Add => a.max(b),
                SelectionMode::Subtract => a.saturating_sub(b),
                SelectionMode::Intersect => a.min(b),
            };
            out.set(x, y, v);
        }
    }
    doc.selection.mask = Some(out);
}

fn point_in_poly(x: f32, y: f32, points: &[(i32, i32)]) -> bool {
    let mut inside = false;
    let mut j = points.len() - 1;
    for i in 0..points.len() {
        let xi = points[i].0 as f32;
        let yi = points[i].1 as f32;
        let xj = points[j].0 as f32;
        let yj = points[j].1 as f32;
        let intersect = ((yi > y) != (yj > y)) && (x < (xj - xi) * (y - yi) / ((yj - yi).abs().max(0.0001)) + xi);
        if intersect { inside = !inside; }
        j = i;
    }
    inside
}

fn color_distance(a: [u8; 4], b: [u8; 4]) -> i32 {
    let dr = a[0] as i32 - b[0] as i32;
    let dg = a[1] as i32 - b[1] as i32;
    let db = a[2] as i32 - b[2] as i32;
    ((dr * dr + dg * dg + db * db) as f32).sqrt() as i32
}

pub fn blur_mask(mask: &Mask, radius: u32) -> Mask {
    if radius == 0 { return mask.clone(); }
    let mut out = Mask::new(mask.width, mask.height, 0);
    let r = radius as i32;
    for y in 0..mask.height as i32 {
        for x in 0..mask.width as i32 {
            let mut sum = 0u32;
            let mut count = 0u32;
            for yy in y - r..=y + r {
                for xx in x - r..=x + r {
                    if xx >= 0 && yy >= 0 && (xx as u32) < mask.width && (yy as u32) < mask.height {
                        sum += mask.get(xx as u32, yy as u32) as u32;
                        count += 1;
                    }
                }
            }
            out.set(x as u32, y as u32, (sum / count.max(1)) as u8);
        }
    }
    out
}

fn dilate(mask: &Mask) -> Mask {
    let mut out = mask.clone();
    for y in 0..mask.height as i32 {
        for x in 0..mask.width as i32 {
            let mut v = 0u8;
            for yy in y - 1..=y + 1 {
                for xx in x - 1..=x + 1 {
                    if xx >= 0 && yy >= 0 && (xx as u32) < mask.width && (yy as u32) < mask.height {
                        v = v.max(mask.get(xx as u32, yy as u32));
                    }
                }
            }
            out.set(x as u32, y as u32, v);
        }
    }
    out
}

fn erode(mask: &Mask) -> Mask {
    let mut out = mask.clone();
    for y in 0..mask.height as i32 {
        for x in 0..mask.width as i32 {
            let mut v = 255u8;
            for yy in y - 1..=y + 1 {
                for xx in x - 1..=x + 1 {
                    if xx < 0 || yy < 0 || (xx as u32) >= mask.width || (yy as u32) >= mask.height {
                        v = 0;
                    } else {
                        v = v.min(mask.get(xx as u32, yy as u32));
                    }
                }
            }
            out.set(x as u32, y as u32, v);
        }
    }
    out
}
