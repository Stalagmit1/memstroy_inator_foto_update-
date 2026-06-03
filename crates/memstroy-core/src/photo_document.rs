use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhotoDocument {
    pub name: String,
    pub width: u32,
    pub height: u32,

    #[serde(default)]
    pub layers: Vec<PhotoLayer>,

    #[serde(default)]
    pub selected_layer: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhotoLayer {
    pub id: String,
    pub name: String,

    #[serde(default = "default_visible")]
    pub visible: bool,

    #[serde(default = "default_opacity")]
    pub opacity: f32,

    #[serde(default)]
    pub x: f32,

    #[serde(default)]
    pub y: f32,

    #[serde(default = "default_scale")]
    pub scale: f32,

    #[serde(default)]
    pub rotation: f32,

    pub kind: PhotoLayerKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PhotoLayerKind {
    Image {
        path: String,
    },
    Text {
        text: String,
        size: f32,
        color: [f32; 4],
    },
    Effect {
        effect: PhotoEffect,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PhotoEffect {
    Blur {
        radius: f32,
    },
    BrightnessContrast {
        brightness: f32,
        contrast: f32,
    },
}

fn default_visible() -> bool {
    true
}

fn default_opacity() -> f32 {
    1.0
}

fn default_scale() -> f32 {
    1.0
}