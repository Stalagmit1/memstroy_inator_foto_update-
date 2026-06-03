use memstroy_core::{PhotoDocument, PhotoLayerKind};

use crate::photo_engine::{BlendMode, Layer, LayerKind, PhotoEngineDocument, PixelBuffer};

pub fn engine_from_photo_document(photo_doc: &PhotoDocument) -> PhotoEngineDocument {
    let mut doc = PhotoEngineDocument::new(photo_doc.width.max(1), photo_doc.height.max(1), photo_doc.name.clone());
    doc.selected_layer = photo_doc.selected_layer.clone();

    for src in &photo_doc.layers {
        let mut layer = match &src.kind {
            PhotoLayerKind::Text { text, size, color } => Layer {
                id: src.id.clone(),
                name: src.name.clone(),
                visible: src.visible,
                locked: false,
                opacity: src.opacity,
                blend_mode: BlendMode::Normal,
                x: src.x,
                y: src.y,
                scale_x: src.scale,
                scale_y: src.scale,
                rotation_degrees: src.rotation,
                skew_x: 0.0,
                skew_y: 0.0,
                mask: None,
                clip_to_below: false,
                effects: Vec::new(),
                kind: LayerKind::TextPlaceholder {
                    text: text.clone(),
                    size: *size,
                    color: [
                        (color[0].clamp(0.0, 1.0) * 255.0) as u8,
                        (color[1].clamp(0.0, 1.0) * 255.0) as u8,
                        (color[2].clamp(0.0, 1.0) * 255.0) as u8,
                        (color[3].clamp(0.0, 1.0) * 255.0) as u8,
                    ],
                },
            },
            PhotoLayerKind::Image { path } => {
                let pixels = crate::photo_engine::io::load_rgba(path).unwrap_or_else(|_| PixelBuffer::solid(320, 200, [70, 70, 80, 255]));
                let mut layer = Layer::raster(src.id.clone(), src.name.clone(), pixels);
                layer.visible = src.visible;
                layer.opacity = src.opacity;
                layer.x = src.x;
                layer.y = src.y;
                layer.scale_x = src.scale;
                layer.scale_y = src.scale;
                layer.rotation_degrees = src.rotation;
                layer
            }
            PhotoLayerKind::Effect { .. } => {
                let mut layer = Layer::raster(src.id.clone(), src.name.clone(), PixelBuffer::solid(280, 80, [90, 60, 120, 180]));
                layer.visible = src.visible;
                layer.opacity = src.opacity;
                layer.x = src.x;
                layer.y = src.y;
                layer.scale_x = src.scale;
                layer.scale_y = src.scale;
                layer.rotation_degrees = src.rotation;
                layer
            }
        };
        layer.opacity = layer.opacity.clamp(0.0, 1.0);
        doc.layers.push(layer);
    }

    doc
}

pub fn export_photo_document_flattened_png(photo_doc: &PhotoDocument, path: impl AsRef<std::path::Path>) -> Result<(), String> {
    let doc = engine_from_photo_document(photo_doc);
    crate::photo_engine::io::export_flattened(&doc, path)
}
