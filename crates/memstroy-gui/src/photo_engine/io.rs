use std::path::Path;

use crate::photo_engine::types::{Layer, PhotoEngineDocument, PixelBuffer};

pub fn load_rgba(path: impl AsRef<Path>) -> Result<PixelBuffer, String> {
    let img = image::open(path.as_ref()).map_err(|e| format!("open image failed: {e}"))?.to_rgba8();
    let (w, h) = img.dimensions();
    PixelBuffer::from_raw(w, h, img.into_raw())
}

pub fn save_rgba(path: impl AsRef<Path>, image: &PixelBuffer) -> Result<(), String> {
    let Some(buf) = image::RgbaImage::from_raw(image.width, image.height, image.data.clone()) else {
        return Err("invalid RGBA buffer".to_string());
    };
    buf.save(path.as_ref()).map_err(|e| format!("save image failed: {e}"))
}

pub fn open_document(path: impl AsRef<Path>) -> Result<PhotoEngineDocument, String> {
    let path_ref = path.as_ref();
    let img = load_rgba(path_ref)?;
    let mut doc = PhotoEngineDocument::new(img.width, img.height, path_ref.file_name().and_then(|s| s.to_str()).unwrap_or("Photo"));
    let id = doc.next_id("background");
    doc.layers.push(Layer::raster(id.clone(), "Background", img));
    doc.selected_layer = Some(id);
    Ok(doc)
}

pub fn export_flattened(doc: &PhotoEngineDocument, path: impl AsRef<Path>) -> Result<(), String> {
    let rendered = doc.render();
    save_rgba(path, &rendered)
}

pub fn import_as_layer(doc: &mut PhotoEngineDocument, path: impl AsRef<Path>) -> Result<String, String> {
    let p = path.as_ref();
    let img = load_rgba(p)?;
    let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("Image Layer").to_string();
    Ok(doc.add_raster_layer(name, img))
}

pub fn export_selected_layer(doc: &PhotoEngineDocument, path: impl AsRef<Path>) -> Result<(), String> {
    let Some(id) = &doc.selected_layer else { return Err("no selected layer".to_string()); };
    let Some(idx) = doc.layers.iter().position(|l| &l.id == id) else { return Err("selected layer not found".to_string()); };
    let img = doc.render_single_layer(idx);
    save_rgba(path, &img)
}
