use std::f32::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Red,
    Green,
    Blue,
    Alpha,
    Rgb,
    Rgba,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlendMode {
    Normal,
    Dissolve,
    Darken,
    Multiply,
    ColorBurn,
    LinearBurn,
    DarkerColor,
    Lighten,
    Screen,
    ColorDodge,
    LinearDodge,
    LighterColor,
    Overlay,
    SoftLight,
    HardLight,
    VividLight,
    LinearLight,
    PinLight,
    HardMix,
    Difference,
    Exclusion,
    Subtract,
    Divide,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionMode {
    Replace,
    Add,
    Subtract,
    Intersect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResampleMethod {
    Nearest,
    Bilinear,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransformKind {
    Scale,
    Rotate,
    Skew,
    Distort,
    Perspective,
    Warp,
    FlipHorizontal,
    FlipVertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alignment {
    Left,
    CenterX,
    Right,
    Top,
    CenterY,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerEffectKind {
    DropShadow,
    InnerShadow,
    OuterGlow,
    InnerGlow,
    Stroke,
    ColorOverlay,
    GradientOverlay,
}

#[derive(Clone, Debug)]
pub struct LayerEffect {
    pub kind: LayerEffectKind,
    pub enabled: bool,
    pub color: [u8; 4],
    pub opacity: f32,
    pub size: f32,
    pub distance: f32,
    pub angle_degrees: f32,
}

impl Default for LayerEffect {
    fn default() -> Self {
        Self {
            kind: LayerEffectKind::DropShadow,
            enabled: true,
            color: [0, 0, 0, 180],
            opacity: 0.7,
            size: 12.0,
            distance: 8.0,
            angle_degrees: 135.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RectI {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl RectI {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.right() && y < self.bottom()
    }

    pub fn intersect(&self, other: RectI) -> Option<RectI> {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = self.right().min(other.right());
        let y1 = self.bottom().min(other.bottom());
        if x1 > x0 && y1 > y0 {
            Some(RectI::new(x0, y0, x1 - x0, y1 - y0))
        } else {
            None
        }
    }
}

#[derive(Clone, Debug)]
pub struct PixelBuffer {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl PixelBuffer {
    pub fn new(width: u32, height: u32) -> Self {
        let len = width.saturating_mul(height).saturating_mul(4) as usize;
        Self { width, height, data: vec![0; len] }
    }

    pub fn solid(width: u32, height: u32, rgba: [u8; 4]) -> Self {
        let mut out = Self::new(width, height);
        out.clear(rgba);
        out
    }

    pub fn from_raw(width: u32, height: u32, data: Vec<u8>) -> Result<Self, String> {
        let expected = width as usize * height as usize * 4;
        if data.len() != expected {
            return Err(format!("bad RGBA buffer length: got {}, expected {}", data.len(), expected));
        }
        Ok(Self { width, height, data })
    }

    #[inline]
    pub fn idx(&self, x: u32, y: u32) -> usize {
        ((y * self.width + x) * 4) as usize
    }

    #[inline]
    pub fn in_bounds(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && (x as u32) < self.width && (y as u32) < self.height
    }

    #[inline]
    pub fn get(&self, x: u32, y: u32) -> [u8; 4] {
        let i = self.idx(x, y);
        [self.data[i], self.data[i + 1], self.data[i + 2], self.data[i + 3]]
    }

    #[inline]
    pub fn get_checked(&self, x: i32, y: i32) -> [u8; 4] {
        if !self.in_bounds(x, y) {
            return [0, 0, 0, 0];
        }
        self.get(x as u32, y as u32)
    }

    #[inline]
    pub fn set(&mut self, x: u32, y: u32, rgba: [u8; 4]) {
        let i = self.idx(x, y);
        self.data[i] = rgba[0];
        self.data[i + 1] = rgba[1];
        self.data[i + 2] = rgba[2];
        self.data[i + 3] = rgba[3];
    }

    #[inline]
    pub fn set_checked(&mut self, x: i32, y: i32, rgba: [u8; 4]) {
        if self.in_bounds(x, y) {
            self.set(x as u32, y as u32, rgba);
        }
    }

    pub fn clear(&mut self, rgba: [u8; 4]) {
        for px in self.data.chunks_exact_mut(4) {
            px.copy_from_slice(&rgba);
        }
    }

    pub fn fill_rect(&mut self, rect: RectI, rgba: [u8; 4]) {
        let full = RectI::new(0, 0, self.width as i32, self.height as i32);
        let Some(r) = rect.intersect(full) else { return; };
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                self.set(x as u32, y as u32, rgba);
            }
        }
    }

    pub fn sub_image(&self, rect: RectI) -> Self {
        let full = RectI::new(0, 0, self.width as i32, self.height as i32);
        let r = rect.intersect(full).unwrap_or(RectI::new(0, 0, 1, 1));
        let mut out = PixelBuffer::new(r.w as u32, r.h as u32);
        for y in 0..r.h {
            for x in 0..r.w {
                out.set(x as u32, y as u32, self.get((r.x + x) as u32, (r.y + y) as u32));
            }
        }
        out
    }

    pub fn resize_nearest(&self, width: u32, height: u32) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        let mut out = PixelBuffer::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let sx = ((x as f32 + 0.5) * self.width as f32 / width as f32).floor() as u32;
                let sy = ((y as f32 + 0.5) * self.height as f32 / height as f32).floor() as u32;
                out.set(x, y, self.get(sx.min(self.width - 1), sy.min(self.height - 1)));
            }
        }
        out
    }

    pub fn resize_bilinear(&self, width: u32, height: u32) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        let mut out = PixelBuffer::new(width, height);
        let sx_scale = if width > 1 { (self.width - 1) as f32 / (width - 1) as f32 } else { 0.0 };
        let sy_scale = if height > 1 { (self.height - 1) as f32 / (height - 1) as f32 } else { 0.0 };
        for y in 0..height {
            for x in 0..width {
                let sx = x as f32 * sx_scale;
                let sy = y as f32 * sy_scale;
                out.set(x, y, bilinear_sample(self, sx, sy));
            }
        }
        out
    }
}

#[derive(Clone, Debug)]
pub struct Mask {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl Mask {
    pub fn new(width: u32, height: u32, value: u8) -> Self {
        Self { width, height, data: vec![value; width as usize * height as usize] }
    }

    #[inline]
    pub fn idx(&self, x: u32, y: u32) -> usize {
        (y * self.width + x) as usize
    }

    #[inline]
    pub fn get(&self, x: u32, y: u32) -> u8 {
        if x >= self.width || y >= self.height { 0 } else { self.data[self.idx(x, y)] }
    }

    #[inline]
    pub fn set(&mut self, x: u32, y: u32, v: u8) {
        if x < self.width && y < self.height {
            let i = self.idx(x, y);
            self.data[i] = v;
        }
    }

    pub fn invert(&mut self) {
        for v in &mut self.data {
            *v = 255 - *v;
        }
    }

    pub fn clear(&mut self, value: u8) {
        self.data.fill(value);
    }
}

#[derive(Clone, Debug)]
pub enum LayerKind {
    Raster(PixelBuffer),
    TextPlaceholder { text: String, size: f32, color: [u8; 4] },
    ShapeRect { width: u32, height: u32, fill: [u8; 4], stroke: [u8; 4], stroke_width: u32 },
    ShapeEllipse { width: u32, height: u32, fill: [u8; 4], stroke: [u8; 4], stroke_width: u32 },
}

#[derive(Clone, Debug)]
pub struct Layer {
    pub id: String,
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    pub opacity: f32,
    pub blend_mode: BlendMode,
    pub x: f32,
    pub y: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub rotation_degrees: f32,
    pub skew_x: f32,
    pub skew_y: f32,
    pub mask: Option<Mask>,
    pub clip_to_below: bool,
    pub effects: Vec<LayerEffect>,
    pub kind: LayerKind,
}

impl Layer {
    pub fn raster(id: impl Into<String>, name: impl Into<String>, image: PixelBuffer) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            visible: true,
            locked: false,
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            x: 0.0,
            y: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            rotation_degrees: 0.0,
            skew_x: 0.0,
            skew_y: 0.0,
            mask: None,
            clip_to_below: false,
            effects: Vec::new(),
            kind: LayerKind::Raster(image),
        }
    }

    pub fn raster_size(&self) -> Option<(u32, u32)> {
        match &self.kind {
            LayerKind::Raster(p) => Some((p.width, p.height)),
            LayerKind::TextPlaceholder { size, text, .. } => Some(((text.len() as f32 * *size * 0.6).max(1.0) as u32, (*size * 1.4).max(1.0) as u32)),
            LayerKind::ShapeRect { width, height, .. } => Some((*width, *height)),
            LayerKind::ShapeEllipse { width, height, .. } => Some((*width, *height)),
        }
    }

    pub fn rasterize_placeholder(&self) -> PixelBuffer {
        match &self.kind {
            LayerKind::Raster(p) => p.clone(),
            LayerKind::TextPlaceholder { text, size, color } => {
                let w = (text.len() as f32 * *size * 0.6).max(1.0) as u32;
                let h = (*size * 1.4).max(1.0) as u32;
                let mut out = PixelBuffer::new(w, h);
                draw_placeholder_text(&mut out, text, *size, *color);
                out
            }
            LayerKind::ShapeRect { width, height, fill, stroke, stroke_width } => {
                let mut out = PixelBuffer::solid(*width, *height, *fill);
                if *stroke_width > 0 {
                    let sw = *stroke_width as i32;
                    let w = *width as i32;
                    let h = *height as i32;
                    out.fill_rect(RectI::new(0, 0, w, sw), *stroke);
                    out.fill_rect(RectI::new(0, h - sw, w, sw), *stroke);
                    out.fill_rect(RectI::new(0, 0, sw, h), *stroke);
                    out.fill_rect(RectI::new(w - sw, 0, sw, h), *stroke);
                }
                out
            }
            LayerKind::ShapeEllipse { width, height, fill, stroke, stroke_width } => {
                let mut out = PixelBuffer::new(*width, *height);
                let rx = *width as f32 / 2.0;
                let ry = *height as f32 / 2.0;
                let sw = *stroke_width as f32;
                for y in 0..*height {
                    for x in 0..*width {
                        let dx = (x as f32 + 0.5 - rx) / rx.max(1.0);
                        let dy = (y as f32 + 0.5 - ry) / ry.max(1.0);
                        let d = dx * dx + dy * dy;
                        if d <= 1.0 {
                            let edge = ((1.0 - d.sqrt()) * rx.min(ry)) <= sw;
                            out.set(x, y, if edge { *stroke } else { *fill });
                        }
                    }
                }
                out
            }
        }
    }
}

fn draw_placeholder_text(out: &mut PixelBuffer, text: &str, size: f32, color: [u8; 4]) {
    let cell_w = (size * 0.5).max(4.0) as i32;
    let cell_h = (size * 0.9).max(6.0) as i32;
    let mut ox = 0i32;
    for ch in text.chars() {
        if ch == ' ' {
            ox += cell_w;
            continue;
        }
        for y in 0..cell_h {
            for x in 0..cell_w {
                let border = x == 0 || y == 0 || x == cell_w - 1 || y == cell_h - 1;
                let diag = ((x + y + ch as i32) % 7) == 0;
                if border || diag {
                    out.set_checked(ox + x, y + (size * 0.2) as i32, color);
                }
            }
        }
        ox += (size * 0.6).max(4.0) as i32;
    }
}

#[derive(Clone, Debug)]
pub struct Selection {
    pub mask: Option<Mask>,
}

impl Selection {
    pub fn empty() -> Self { Self { mask: None } }
    pub fn all(width: u32, height: u32) -> Self { Self { mask: Some(Mask::new(width, height, 255)) } }
    pub fn clear(&mut self) { self.mask = None; }
    pub fn active(&self) -> bool { self.mask.is_some() }
    pub fn coverage(&self, x: u32, y: u32) -> u8 { self.mask.as_ref().map(|m| m.get(x, y)).unwrap_or(255) }
}

#[derive(Clone, Debug)]
pub struct PhotoEngineDocument {
    pub width: u32,
    pub height: u32,
    pub name: String,
    pub background: [u8; 4],
    pub layers: Vec<Layer>,
    pub selected_layer: Option<String>,
    pub selection: Selection,
    pub undo_stack: Vec<DocumentSnapshot>,
    pub redo_stack: Vec<DocumentSnapshot>,
    pub history_limit: usize,
}

#[derive(Clone, Debug)]
pub struct DocumentSnapshot {
    pub width: u32,
    pub height: u32,
    pub name: String,
    pub background: [u8; 4],
    pub layers: Vec<Layer>,
    pub selected_layer: Option<String>,
    pub selection: Selection,
}

impl PhotoEngineDocument {
    pub fn new(width: u32, height: u32, name: impl Into<String>) -> Self {
        Self {
            width: width.max(1),
            height: height.max(1),
            name: name.into(),
            background: [0, 0, 0, 0],
            layers: Vec::new(),
            selected_layer: None,
            selection: Selection::empty(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            history_limit: 50,
        }
    }

    pub fn snapshot(&self) -> DocumentSnapshot {
        DocumentSnapshot {
            width: self.width,
            height: self.height,
            name: self.name.clone(),
            background: self.background,
            layers: self.layers.clone(),
            selected_layer: self.selected_layer.clone(),
            selection: self.selection.clone(),
        }
    }

    pub fn restore(&mut self, s: DocumentSnapshot) {
        self.width = s.width;
        self.height = s.height;
        self.name = s.name;
        self.background = s.background;
        self.layers = s.layers;
        self.selected_layer = s.selected_layer;
        self.selection = s.selection;
    }

    pub fn push_history(&mut self) {
        self.undo_stack.push(self.snapshot());
        if self.undo_stack.len() > self.history_limit {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    pub fn selected_layer_mut(&mut self) -> Option<&mut Layer> {
        let id = self.selected_layer.clone()?;
        self.layers.iter_mut().find(|l| l.id == id)
    }

    pub fn selected_layer_ref(&self) -> Option<&Layer> {
        let id = self.selected_layer.as_ref()?;
        self.layers.iter().find(|l| &l.id == id)
    }

    pub fn next_id(&self, prefix: &str) -> String {
        let mut n = 1usize;
        loop {
            let id = format!("{}_{}", prefix, n);
            if !self.layers.iter().any(|l| l.id == id) { return id; }
            n += 1;
        }
    }
}

pub fn clamp01(v: f32) -> f32 { v.max(0.0).min(1.0) }
pub fn clamp_u8(v: f32) -> u8 { (v.max(0.0).min(255.0).round()) as u8 }
pub fn deg_to_rad(d: f32) -> f32 { d * PI / 180.0 }

pub fn bilinear_sample(img: &PixelBuffer, x: f32, y: f32) -> [u8; 4] {
    if x < 0.0 || y < 0.0 || x > img.width as f32 - 1.0 || y > img.height as f32 - 1.0 {
        return [0, 0, 0, 0];
    }
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let x1 = (x0 + 1).min(img.width as i32 - 1);
    let y1 = (y0 + 1).min(img.height as i32 - 1);
    let tx = x - x0 as f32;
    let ty = y - y0 as f32;
    let c00 = img.get_checked(x0, y0);
    let c10 = img.get_checked(x1, y0);
    let c01 = img.get_checked(x0, y1);
    let c11 = img.get_checked(x1, y1);
    let mut out = [0u8; 4];
    for i in 0..4 {
        let a = c00[i] as f32 * (1.0 - tx) + c10[i] as f32 * tx;
        let b = c01[i] as f32 * (1.0 - tx) + c11[i] as f32 * tx;
        out[i] = clamp_u8(a * (1.0 - ty) + b * ty);
    }
    out
}

pub fn rgba_to_hsl(c: [u8; 4]) -> (f32, f32, f32) {
    let r = c[0] as f32 / 255.0;
    let g = c[1] as f32 / 255.0;
    let b = c[2] as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) * 0.5;
    if (max - min).abs() < f32::EPSILON {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if (max - r).abs() < f32::EPSILON {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if (max - g).abs() < f32::EPSILON {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } / 6.0;
    (h, s, l)
}

pub fn hsl_to_rgba(h: f32, s: f32, l: f32, a: u8) -> [u8; 4] {
    fn hue_to_rgb(p: f32, q: f32, mut t: f32) -> f32 {
        if t < 0.0 { t += 1.0; }
        if t > 1.0 { t -= 1.0; }
        if t < 1.0 / 6.0 { return p + (q - p) * 6.0 * t; }
        if t < 1.0 / 2.0 { return q; }
        if t < 2.0 / 3.0 { return p + (q - p) * (2.0 / 3.0 - t) * 6.0; }
        p
    }
    if s == 0.0 {
        let v = clamp_u8(l * 255.0);
        return [v, v, v, a];
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    [
        clamp_u8(hue_to_rgb(p, q, h + 1.0 / 3.0) * 255.0),
        clamp_u8(hue_to_rgb(p, q, h) * 255.0),
        clamp_u8(hue_to_rgb(p, q, h - 1.0 / 3.0) * 255.0),
        a,
    ]
}
