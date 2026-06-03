use crate::photo_engine::selection::blur_mask;
use crate::photo_engine::types::{LayerKind, Mask, PhotoEngineDocument};

pub fn add_reveal_all_mask(doc: &mut PhotoEngineDocument) {
    let doc_w = doc.width;
    let doc_h = doc.height;
    let Some(layer) = doc.selected_layer_mut() else { return; };
    if layer.locked { return; }
    let (w, h) = layer.raster_size().unwrap_or((doc_w, doc_h));
    doc.push_history();
    let layer = doc.selected_layer_mut().unwrap();
    layer.mask = Some(Mask::new(w, h, 255));
}

pub fn add_hide_all_mask(doc: &mut PhotoEngineDocument) {
    let doc_w = doc.width;
    let doc_h = doc.height;
    let Some(layer) = doc.selected_layer_mut() else { return; };
    if layer.locked { return; }
    let (w, h) = layer.raster_size().unwrap_or((doc_w, doc_h));
    doc.push_history();
    let layer = doc.selected_layer_mut().unwrap();
    layer.mask = Some(Mask::new(w, h, 0));
}

pub fn add_mask_from_selection(doc: &mut PhotoEngineDocument) {
    let Some(sel) = doc.selection.mask.clone() else { return; };
    let doc_w = doc.width;
    let doc_h = doc.height;
    let Some(layer) = doc.selected_layer_mut() else { return; };
    if layer.locked { return; }
    let (w, h) = layer.raster_size().unwrap_or((doc_w, doc_h));
    doc.push_history();
    let mut out = Mask::new(w, h, 0);
    for y in 0..h {
        for x in 0..w {
            out.set(x, y, sel.get(x.min(sel.width - 1), y.min(sel.height - 1)));
        }
    }
    doc.selected_layer_mut().unwrap().mask = Some(out);
}

pub fn invert_layer_mask(doc: &mut PhotoEngineDocument) {
    let Some(layer) = doc.selected_layer_mut() else { return; };
    if layer.locked || layer.mask.is_none() { return; }
    doc.push_history();
    if let Some(mask) = &mut doc.selected_layer_mut().unwrap().mask {
        mask.invert();
    }
}

pub fn delete_layer_mask(doc: &mut PhotoEngineDocument) {
    let Some(layer) = doc.selected_layer_mut() else { return; };
    if layer.locked || layer.mask.is_none() { return; }
    doc.push_history();
    doc.selected_layer_mut().unwrap().mask = None;
}

pub fn apply_layer_mask_destructive(doc: &mut PhotoEngineDocument) {
    let Some(layer) = doc.selected_layer_mut() else { return; };
    if layer.locked || layer.mask.is_none() { return; }
    doc.push_history();
    let layer = doc.selected_layer_mut().unwrap();
    let Some(mask) = layer.mask.take() else { return; };
    if let LayerKind::Raster(pixels) = &mut layer.kind {
        for y in 0..pixels.height {
            for x in 0..pixels.width {
                let mut c = pixels.get(x, y);
                let m = mask.get(x.min(mask.width - 1), y.min(mask.height - 1)) as u16;
                c[3] = ((c[3] as u16 * m) / 255) as u8;
                pixels.set(x, y, c);
            }
        }
    }
}

pub fn feather_layer_mask(doc: &mut PhotoEngineDocument, radius: u32) {
    let Some(layer) = doc.selected_layer_mut() else { return; };
    if layer.locked || layer.mask.is_none() { return; }
    doc.push_history();
    if let Some(mask) = &mut doc.selected_layer_mut().unwrap().mask {
        *mask = blur_mask(mask, radius);
    }
}
