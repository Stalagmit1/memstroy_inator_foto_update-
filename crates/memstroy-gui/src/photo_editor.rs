use eframe::egui;
use memstroy_core::{PhotoDocument, PhotoLayer, PhotoLayerKind};
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Write;
use std::sync::Once;

const DEFAULT_IMAGE_W: f32 = 320.0;
const DEFAULT_IMAGE_H: f32 = 200.0;
const EFFECT_PLACEHOLDER_W: f32 = 280.0;
const EFFECT_PLACEHOLDER_H: f32 = 80.0;
const MIN_LAYER_SCALE: f32 = 0.01;
const MAX_LAYER_SCALE: f32 = 100.0;
const MIN_CAMERA_ZOOM: f32 = 0.01;
const MAX_CAMERA_ZOOM: f32 = 64.0;
const EDITOR_BUILD: &str = "";

// Photoshop-like defaults: 100% opacity/flow/hardness, 25% brush spacing,
// and 50% exposure/strength for local retouch tools.
const DEFAULT_BRUSH_SIZE: f32 = 32.0;
const DEFAULT_BRUSH_OPACITY: f32 = 1.0;
const DEFAULT_BRUSH_FLOW: f32 = 1.0;
const DEFAULT_BRUSH_HARDNESS: f32 = 1.0;
const DEFAULT_BRUSH_SPACING: f32 = 0.25;
const DEFAULT_RETOUCH_STRENGTH: f32 = 0.50;
const MAX_BRUSH_STAMPS_PER_EVENT: i32 = 24;
const MAX_RETOUCH_STAMPS_PER_EVENT: i32 = 10;
const MAX_FILTER_STAMPS_PER_EVENT: i32 = 6;
const BRUSH_UNDO_GROUP_MS: u128 = 900;
const BRUSH_UPDATE_WARN_MS: u128 = 90;
const MAX_EXPORT_PIXELS: u64 = 8192 * 8192;
const MAX_LIVE_TEXTURE_PIXELS: u64 = 1024 * 1024;
// Heavy layer FX preview is rendered on a reduced copy, then stretched to the real layer size.
// Export/Bake still uses full resolution through load_baked_image_rgba().
const MAX_EXPENSIVE_FILTER_PREVIEW_PIXELS: u64 = 360 * 360;
const FILTER_PREVIEW_BLUR_KEY_STEP: f32 = 4.0;
const FILTER_PREVIEW_SHARPEN_KEY_STEP: f32 = 2.0;
// Undo snapshots keep only reasonably sized raster images in memory.
// Larger layers still undo their transform/effects, but their disk pixels are not cloned.
const MAX_HISTORY_IMAGE_PIXELS: u64 = 4096 * 4096;
const MAX_UNDO_STATES: usize = 24;
const PAINT_LAYER_PADDING: i32 = 32;
const PHOTO_LOG_RECENT_LIMIT: usize = 160;
const SLOW_OPERATION_WARN_MS: u128 = 250;
const VERY_SLOW_OPERATION_ERROR_MS: u128 = 1200;

static PHOTO_EDITOR_LOG_INIT: Once = Once::new();

thread_local! {
    static IMAGE_TEXTURE_CACHE: RefCell<HashMap<String, CachedTexture>> = RefCell::new(HashMap::new());
    static IMAGE_DIM_CACHE: RefCell<HashMap<String, egui::Vec2>> = RefCell::new(HashMap::new());
    static CANVAS_VIEW_CACHE: RefCell<HashMap<String, CanvasViewState>> = RefCell::new(HashMap::new());
    static LAYER_RUNTIME_FX: RefCell<HashMap<String, LayerRuntimeFx>> = RefCell::new(HashMap::new());
    static ACTIVE_PHOTO_TOOL: RefCell<PhotoTool> = RefCell::new(PhotoTool::Move);
    static DOC_SELECTIONS: RefCell<HashMap<String, SelectionState>> = RefCell::new(HashMap::new());
    static BRUSH_SETTINGS: RefCell<BrushSettings> = RefCell::new(BrushSettings::default());
    static BRUSH_STROKE_STATE: RefCell<BrushStrokeState> = RefCell::new(BrushStrokeState::default());
    static CANVAS_POINTER_STATE: RefCell<CanvasPointerState> = RefCell::new(CanvasPointerState::default());
    static RASTER_FONT_CACHE: RefCell<Option<Option<ab_glyph::FontArc>>> = RefCell::new(None);
    static PHOTO_HISTORY: RefCell<HashMap<String, PhotoHistoryState>> = RefCell::new(HashMap::new());
    static LAYER_CLIPBOARD: RefCell<Option<PhotoLayer>> = RefCell::new(None);
    static PHOTO_RECENT_LOGS: RefCell<Vec<String>> = RefCell::new(Vec::new());
    static LAST_BRUSH_UNDO_CHECKPOINT: RefCell<Option<(String, PhotoTool, u128)>> = RefCell::new(None);
    static LAST_BRUSH_PERF_LOG_MS: RefCell<u128> = RefCell::new(0);
}


#[derive(Clone)]
struct CachedTexture {
    texture: egui::TextureHandle,
    size: egui::Vec2,
}

#[derive(Clone, Copy, Debug)]
struct CanvasViewState {
    initialized: bool,
    zoom: f32,
    pan: egui::Vec2,
    show_grid: bool,
    show_guides: bool,
    show_checker: bool,
}

impl Default for CanvasViewState {
    fn default() -> Self {
        Self {
            initialized: false,
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
            show_grid: true,
            show_guides: true,
            show_checker: true,
        }
    }
}

#[derive(Clone, Debug)]
struct LayerRuntimeFx {
    flip_x: bool,
    flip_y: bool,
    grayscale: bool,
    invert: bool,
    brightness: f32,
    contrast: f32,
    saturation: f32,
    hue_rotate: f32,
    blur: f32,
    sharpen: f32,
    pixelate: f32,
    tint: [f32; 4],
    tint_amount: f32,
    shadow_enabled: bool,
    shadow_dx: f32,
    shadow_dy: f32,
    shadow_opacity: f32,
    stroke_enabled: bool,
    stroke_width: f32,
    stroke_color: [f32; 4],
}

impl Default for LayerRuntimeFx {
    fn default() -> Self {
        Self {
            flip_x: false,
            flip_y: false,
            grayscale: false,
            invert: false,
            brightness: 0.0,
            contrast: 0.0,
            saturation: 1.0,
            hue_rotate: 0.0,
            blur: 0.0,
            sharpen: 0.0,
            pixelate: 1.0,
            tint: [1.0, 1.0, 1.0, 1.0],
            tint_amount: 0.0,
            shadow_enabled: false,
            shadow_dx: 10.0,
            shadow_dy: 10.0,
            shadow_opacity: 0.45,
            stroke_enabled: false,
            stroke_width: 3.0,
            stroke_color: [1.0, 1.0, 1.0, 1.0],
        }
    }
}


#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PhotoTool {
    Move,
    Hand,
    Marquee,
    Crop,
    Brush,
    Eraser,
    Dodge,
    Burn,
    Blur,
    Sharpen,
    Eyedropper,
}

impl PhotoTool {
    fn label(self) -> &'static str {
        match self {
            PhotoTool::Move => "Перемещение",
            PhotoTool::Hand => "Рука / панорама",
            PhotoTool::Marquee => "Прямоугольное выделение",
            PhotoTool::Crop => "Кадрирование",
            PhotoTool::Brush => "Кисть",
            PhotoTool::Eraser => "Ластик",
            PhotoTool::Dodge => "Осветлитель",
            PhotoTool::Burn => "Затемнитель",
            PhotoTool::Blur => "Локальное размытие",
            PhotoTool::Sharpen => "Шакализатор",
            PhotoTool::Eyedropper => "Пипетка",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct SelectionState {
    active: bool,
    dragging: bool,
    start: egui::Pos2,
    end: egui::Pos2,
}

impl Default for SelectionState {
    fn default() -> Self {
        Self {
            active: false,
            dragging: false,
            start: egui::Pos2::ZERO,
            end: egui::Pos2::ZERO,
        }
    }
}

impl SelectionState {
    fn rect(self) -> Option<egui::Rect> {
        if !self.active && !self.dragging {
            return None;
        }
        let min = egui::pos2(self.start.x.min(self.end.x), self.start.y.min(self.end.y));
        let max = egui::pos2(self.start.x.max(self.end.x), self.start.y.max(self.end.y));
        let rect = egui::Rect::from_min_max(min, max);
        if rect.width() < 1.0 || rect.height() < 1.0 {
            None
        } else {
            Some(rect)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RetouchToneRange {
    Shadows,
    Midtones,
    Highlights,
}

impl RetouchToneRange {
    fn label(self) -> &'static str {
        match self {
            RetouchToneRange::Shadows => "Тени",
            RetouchToneRange::Midtones => "Средние тона",
            RetouchToneRange::Highlights => "Света",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct BrushSettings {
    size: f32,
    opacity: f32,
    flow: f32,
    hardness: f32,
    spacing: f32,
    color: [f32; 4],
    strength: f32,
    retouch_range: RetouchToneRange,
    protect_tones: bool,
}

impl Default for BrushSettings {
    fn default() -> Self {
        Self {
            size: DEFAULT_BRUSH_SIZE,
            opacity: DEFAULT_BRUSH_OPACITY,
            flow: DEFAULT_BRUSH_FLOW,
            hardness: DEFAULT_BRUSH_HARDNESS,
            spacing: DEFAULT_BRUSH_SPACING,
            color: [0.0, 0.0, 0.0, 1.0],
            strength: DEFAULT_RETOUCH_STRENGTH,
            retouch_range: RetouchToneRange::Midtones,
            protect_tones: true,
        }
    }
}

#[derive(Clone, Debug)]
struct BrushStrokeState {
    active: bool,
    tool: PhotoTool,
    last_doc_pos: egui::Pos2,
    target_layer_id: Option<String>,
    target_path: Option<std::path::PathBuf>,
    working_image: Option<image::RgbaImage>,
    source_image: Option<image::RgbaImage>,
    dirty: bool,
    paint_ops_since_upload: u32,
}

impl Default for BrushStrokeState {
    fn default() -> Self {
        Self {
            active: false,
            tool: PhotoTool::Brush,
            last_doc_pos: egui::Pos2::ZERO,
            target_layer_id: None,
            target_path: None,
            working_image: None,
            source_image: None,
            dirty: false,
            paint_ops_since_upload: 0,
        }
    }
}

#[derive(Clone)]
struct HistoryImageSnapshot {
    path: String,
    rgba: image::RgbaImage,
}

#[derive(Clone)]
struct EditorSnapshot {
    name: String,
    width: u32,
    height: u32,
    layers: Vec<PhotoLayer>,
    selected_layer: Option<String>,
    fx_by_layer: HashMap<String, LayerRuntimeFx>,
    selection: SelectionState,
    images: Vec<HistoryImageSnapshot>,
    signature: String,
}

#[derive(Default)]
struct PhotoHistoryState {
    undo: Vec<EditorSnapshot>,
    redo: Vec<EditorSnapshot>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CanvasPointerAction {
    None,
    Marquee,
    Crop,
    Paint,
}

#[derive(Clone, Copy, Debug)]
struct CanvasPointerState {
    primary_was_down: bool,
    action: CanvasPointerAction,
}

impl Default for CanvasPointerState {
    fn default() -> Self {
        Self {
            primary_was_down: false,
            action: CanvasPointerAction::None,
        }
    }
}

impl LayerRuntimeFx {
    fn image_cache_suffix(&self) -> String {
        // Quantize expensive preview-only keys. While the user drags Blur/Шакализатор
        // sliders, this prevents rebuilding a full texture for every tiny float delta.
        // The stored FX values remain exact; Bake/Export still use full-resolution values.
        let blur_key = if self.blur > 0.01 {
            quantize_f32(self.blur, FILTER_PREVIEW_BLUR_KEY_STEP)
        } else {
            0.0
        };
        let sharpen_key = if self.sharpen > 0.01 {
            quantize_f32(self.sharpen, FILTER_PREVIEW_SHARPEN_KEY_STEP)
        } else {
            0.0
        };
        format!(
            "fx:{}:{}:{:.2}:{:.2}:{:.2}:{:.1}:{:.1}:{:.1}:{:.1}:{:.2}:{:.2}:{:.2}:{:.2}:{:.2}",
            self.flip_x as u8,
            self.flip_y as u8,
            self.brightness,
            self.contrast,
            self.saturation,
            self.hue_rotate,
            blur_key,
            sharpen_key,
            self.pixelate,
            self.tint[0],
            self.tint[1],
            self.tint[2],
            self.tint[3],
            self.tint_amount,
        ) + &format!(":{}:{}", self.grayscale as u8, self.invert as u8)
    }
}

fn quantize_f32(value: f32, step: f32) -> f32 {
    if step <= 0.0 {
        value
    } else {
        (value / step).round() * step
    }
}

fn has_expensive_layer_preview_fx(fx: &LayerRuntimeFx) -> bool {
    fx.blur > 0.01 || fx.sharpen > 0.01
}

fn downscale_dynamic_for_fast_preview(mut image: image::DynamicImage, max_pixels: u64) -> image::DynamicImage {
    let width = image.width().max(1);
    let height = image.height().max(1);
    let pixels = width as u64 * height as u64;
    if pixels <= max_pixels || max_pixels == 0 {
        return image;
    }

    let ratio = (max_pixels as f32 / pixels as f32).sqrt().clamp(0.05, 1.0);
    let new_w = ((width as f32 * ratio).round() as u32).max(1);
    let new_h = ((height as f32 * ratio).round() as u32).max(1);
    let rgba = image.to_rgba8();
    let resized = image::imageops::resize(&rgba, new_w, new_h, image::imageops::FilterType::Triangle);
    image = image::DynamicImage::ImageRgba8(resized);
    image
}

pub fn show_photo_editor(ctx: &egui::Context, photo_doc: &mut PhotoDocument) {
    install_photo_editor_logging_once();
    apply_photoshop_like_visuals(ctx);
    handle_keyboard_shortcuts(ctx, photo_doc);

    egui::TopBottomPanel::top("photo_editor_menu_bar")
        .exact_height(30.0)
        .show(ctx, |ui| {
            draw_top_bar(ui, photo_doc);
        });

    egui::TopBottomPanel::top("photo_editor_options_bar")
        .exact_height(34.0)
        .show(ctx, |ui| {
            draw_options_bar(ui, photo_doc);
        });

    egui::SidePanel::left("photo_tools_panel")
        .exact_width(112.0)
        .resizable(false)
        .show(ctx, |ui| {
            draw_tools_panel(ui, photo_doc);
        });

    egui::SidePanel::right("photo_right_dock")
        .default_width(365.0)
        .resizable(true)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .id_source("photo_right_dock_scroll_v8")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    draw_layers_and_inspector(ui, photo_doc);
                });
        });

    egui::CentralPanel::default().show(ctx, |ui| {
        draw_canvas(ui, photo_doc);
    });
}
fn apply_photoshop_like_visuals(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();

    let bg = egui::Color32::from_rgb(0x1a, 0x19, 0x12);
    let accent = egui::Color32::from_rgb(0xcc, 0xc1, 0x00);

    visuals.panel_fill = bg;
    visuals.window_fill = bg;
    visuals.extreme_bg_color = bg;
    visuals.faint_bg_color = bg;

    visuals.widgets.noninteractive.bg_fill = bg;
    visuals.widgets.inactive.bg_fill = bg;
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(0x1a, 0x19, 0x12);
    visuals.widgets.active.bg_fill = accent;
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, bg);

    visuals.selection.bg_fill = accent;
    visuals.selection.stroke = egui::Stroke::new(1.0, bg);

    visuals.hyperlink_color = accent;

    ctx.set_visuals(visuals);
}

fn draw_top_bar(ui: &mut egui::Ui, photo_doc: &mut PhotoDocument) {
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.label(egui::RichText::new("Mp").strong().color(egui::Color32::from_rgb(0xcc, 0xc1, 0x00)));
        ui.add_space(10.0);

        ui.menu_button("Файл", |ui| {
            if ui.button("Поместить изображение...").clicked() { add_image_layer(photo_doc); ui.close_menu(); }
            if ui.button("Новый растровый слой").clicked() { add_blank_raster_layer(photo_doc); ui.close_menu(); }
            if ui.button("Экспорт PNG...").clicked() { export_visible_layers_to_png(photo_doc); ui.close_menu(); }
        });
        ui.menu_button("Правка", |ui| {
            let (undo_count, redo_count) = history_counts(photo_doc);
            if ui.add_enabled(undo_count > 0, egui::Button::new("Назад  Ctrl+Z")).clicked() { undo_photo_edit(photo_doc); ui.close_menu(); }
            if ui.add_enabled(redo_count > 0, egui::Button::new("Вперёд  Ctrl+Shift+Z")).clicked() { redo_photo_edit(photo_doc); ui.close_menu(); }
            ui.separator();
            if ui.button("Копировать слой  Ctrl+C").clicked() { copy_selected_layer_to_clipboard(photo_doc); ui.close_menu(); }
            if ui.button("Вырезать слой  Ctrl+X").clicked() { cut_selected_layer_to_clipboard(photo_doc); ui.close_menu(); }
            if ui.button("Вставить слой  Ctrl+V").clicked() { paste_layer_from_clipboard(photo_doc); ui.close_menu(); }
            ui.separator();
            if ui.button("Выделить всё  Ctrl+A").clicked() { select_all_document(photo_doc); ui.close_menu(); }
            if ui.button("Снять выделение  Ctrl+D").clicked() { clear_active_selection(photo_doc); ui.close_menu(); }
            ui.separator();
            if ui.button("Дублировать слой").clicked() { duplicate_selected_layer(photo_doc); ui.close_menu(); }
            if ui.button("Удалить слой").clicked() { delete_selected_layer(photo_doc); ui.close_menu(); }
            if ui.button("Сбросить трансформацию").clicked() { reset_selected_transform(photo_doc); ui.close_menu(); }
        });
        ui.menu_button("Изображение", |ui| {
            if ui.button("Обрезать по выделению").clicked() { crop_document_to_active_selection(photo_doc); ui.close_menu(); }
            if ui.button("Обрезать по слою").clicked() { crop_document_to_selected_layer(photo_doc); ui.close_menu(); }
            if ui.button("Повернуть холст на 90° вправо").clicked() { rotate_canvas_90(photo_doc, true); ui.close_menu(); }
            if ui.button("Повернуть холст на 90° влево").clicked() { rotate_canvas_90(photo_doc, false); ui.close_menu(); }
            if ui.button("Отразить холст по горизонтали").clicked() { flip_canvas(photo_doc, Axis::X); ui.close_menu(); }
            if ui.button("Отразить холст по вертикали").clicked() { flip_canvas(photo_doc, Axis::Y); ui.close_menu(); }
            ui.separator();
            if ui.button("Инвертировать выбранное изображение  Ctrl+I").clicked() { invert_selected_image(photo_doc); ui.close_menu(); }
            if ui.button("Обесцветить выбранное изображение  Ctrl+Shift+U").clicked() { desaturate_selected_image(photo_doc); ui.close_menu(); }
            if ui.button("Автоконтраст").clicked() { auto_contrast_selected_image(photo_doc); ui.close_menu(); }
            if ui.button("Залить цветом кисти").clicked() { add_solid_fill_layer(photo_doc); ui.close_menu(); }
            if ui.button("Градиент от цвета кисти к прозрачности").clicked() { add_foreground_to_transparent_gradient_layer(photo_doc); ui.close_menu(); }
        });
        ui.menu_button("Слой", |ui| {
            if ui.button("Новый текст").clicked() { add_text_layer(photo_doc); ui.close_menu(); }
            if ui.button("Новое изображение").clicked() { add_image_layer(photo_doc); ui.close_menu(); }
            if ui.button("Новый растровый слой").clicked() { add_blank_raster_layer(photo_doc); ui.close_menu(); }
            if ui.button("Слой сплошной заливки").clicked() { add_solid_fill_layer(photo_doc); ui.close_menu(); }
            if ui.button("Слой градиента").clicked() { add_foreground_to_transparent_gradient_layer(photo_doc); ui.close_menu(); }
            ui.separator();
            if ui.button("Выше").clicked() { move_selected_layer(photo_doc, 1); ui.close_menu(); }
            if ui.button("Ниже").clicked() { move_selected_layer(photo_doc, -1); ui.close_menu(); }
            if ui.button("Наверх").clicked() { move_selected_layer_to_end(photo_doc); ui.close_menu(); }
            if ui.button("Вниз").clicked() { move_selected_layer_to_start(photo_doc); ui.close_menu(); }
            ui.separator();
            if ui.button("Объединить с нижним").clicked() { merge_selected_layer_down(photo_doc); ui.close_menu(); }
            if ui.button("Свести видимые слои").clicked() { flatten_visible_layers(photo_doc); ui.close_menu(); }
        });
        ui.menu_button("Текст", |ui| {
            if ui.button("Создать текст").clicked() { add_text_layer(photo_doc); ui.close_menu(); }
            if ui.button("Стиль мемного текста").clicked() { apply_meme_text_style(photo_doc); ui.close_menu(); }
            if ui.button("Повернуть на -15°").clicked() { rotate_selected_layer(photo_doc, -15.0); ui.close_menu(); }
            if ui.button("Повернуть на +15°").clicked() { rotate_selected_layer(photo_doc, 15.0); ui.close_menu(); }
        });
        ui.menu_button("Фильтр", |ui| {
            if ui.button("Размытие кистью").clicked() { set_active_photo_tool(PhotoTool::Blur); ui.close_menu(); }
            if ui.button("Шакализатор кистью").clicked() { set_active_photo_tool(PhotoTool::Sharpen); ui.close_menu(); }
            if ui.button("Применить эффекты к слою").clicked() { bake_selected_image_filters_to_png(photo_doc); ui.close_menu(); }
            ui.separator();
            if ui.button("Инвертировать выбранное изображение  Ctrl+I").clicked() { invert_selected_image(photo_doc); ui.close_menu(); }
            if ui.button("Обесцветить выбранное изображение  Ctrl+Shift+U").clicked() { desaturate_selected_image(photo_doc); ui.close_menu(); }
            if ui.button("Автоконтраст").clicked() { auto_contrast_selected_image(photo_doc); ui.close_menu(); }
            if ui.button("Автоуровни").clicked() { auto_levels_selected_image(photo_doc); ui.close_menu(); }
            if ui.button("Порог 128").clicked() { threshold_selected_image(photo_doc, 128); ui.close_menu(); }
            if ui.button("Постеризация 4 уровня").clicked() { posterize_selected_image(photo_doc, 4); ui.close_menu(); }
        });
        ui.menu_button("Диагностика", |ui| {
            ui.label("Лог зависаний и ошибок пишется во временную папку.");
            ui.monospace(photo_log_path().display().to_string());
            if ui.button("Записать контрольную точку").clicked() { write_diagnostic_checkpoint(photo_doc, "manual menu checkpoint"); ui.close_menu(); }
            if ui.button("Очистить недавний лог").clicked() { clear_recent_photo_logs(); ui.close_menu(); }
        });
        ui.menu_button("Вид", |ui| {
            ui.label("Ctrl + колесо мыши — зум вокруг курсора");
            ui.label("Пробел / средняя кнопка — панорама");
            ui.label("Колесо — прокрутка холста");
        });

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(format!("{} × {}", photo_doc.width, photo_doc.height));
            ui.label(EDITOR_BUILD);
        });
    });
}

fn draw_options_bar(ui: &mut egui::Ui, photo_doc: &mut PhotoDocument) {
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        let (undo_count, redo_count) = history_counts(photo_doc);
        if ui.add_enabled(undo_count > 0, egui::Button::new("Назад")).on_hover_text("Назад / Ctrl+Z").clicked() { undo_photo_edit(photo_doc); }
        if ui.add_enabled(redo_count > 0, egui::Button::new("Вперёд")).on_hover_text("Вперёд / Ctrl+Shift+Z").clicked() { redo_photo_edit(photo_doc); }
        ui.separator();
        ui.label(format!("{} ", active_photo_tool().label()));
        ui.separator();
        let (undo_count, redo_count) = history_counts(photo_doc);
        if ui.add_enabled(undo_count > 0, egui::Button::new("Назад")).on_hover_text("Назад").clicked() { undo_photo_edit(photo_doc); }
        if ui.add_enabled(redo_count > 0, egui::Button::new("Вперёд")).on_hover_text("Вперёд").clicked() { redo_photo_edit(photo_doc); }
        ui.separator();
        if ui.small_button("Текст").on_hover_text("Новый текст").clicked() { add_text_layer(photo_doc); }
        if ui.small_button("Изобр.").on_hover_text("Поместить изображение").clicked() { add_image_layer(photo_doc); }
        if ui.small_button("Растр").on_hover_text("Новый растровый слой для кисти/ластика").clicked() { add_blank_raster_layer(photo_doc); }
        if ui.small_button("Дубль").on_hover_text("Дублировать слой (Ctrl+J)").clicked() { duplicate_selected_layer(photo_doc); }
        if ui.small_button("Удалить").on_hover_text("Удалить").clicked() { delete_selected_layer(photo_doc); }
        if ui.small_button("Копия").on_hover_text("Копировать слой").clicked() { copy_selected_layer_to_clipboard(photo_doc); }
        if ui.small_button("Вставить").on_hover_text("Вставить слой").clicked() { paste_layer_from_clipboard(photo_doc); }
        ui.separator();
        if ui.small_button("Лево").on_hover_text("Выровнять слева").clicked() { align_selected_layer(photo_doc, AlignTarget::Left); }
        if ui.small_button("Центр X").on_hover_text("По центру X").clicked() { align_selected_layer(photo_doc, AlignTarget::CenterX); }
        if ui.small_button("Право").on_hover_text("Выровнять справа").clicked() { align_selected_layer(photo_doc, AlignTarget::Right); }
        if ui.small_button("Верх").on_hover_text("Выровнять сверху").clicked() { align_selected_layer(photo_doc, AlignTarget::Top); }
        if ui.small_button("Центр Y").on_hover_text("По центру Y").clicked() { align_selected_layer(photo_doc, AlignTarget::CenterY); }
        if ui.small_button("Низ").on_hover_text("Выровнять снизу").clicked() { align_selected_layer(photo_doc, AlignTarget::Bottom); }
        if ui.small_button("Распределить X").on_hover_text("Распределить по X").clicked() { distribute_layers(photo_doc, Axis::X); }
        if ui.small_button("Распределить Y").on_hover_text("Распределить по Y").clicked() { distribute_layers(photo_doc, Axis::Y); }
        ui.separator();
        if ui.small_button("Центр").clicked() { center_selected_layer(photo_doc); }
        if ui.small_button("Вписать").clicked() { fit_selected_layer_to_canvas(photo_doc); }
        if ui.small_button("Сброс").clicked() { reset_selected_transform(photo_doc); }
    });
}

fn draw_tools_panel(ui: &mut egui::Ui, _photo_doc: &mut PhotoDocument) {
    ui.vertical_centered(|ui| {
        ui.add_space(6.0);
        tool_icon(ui, PhotoTool::Move, "Видим.", "Перемещение слоя мышкой");
        tool_icon(ui, PhotoTool::Hand, "Рука", "Двигать вид холста");
        tool_icon(ui, PhotoTool::Marquee, "Выделить", "Выделение прямоугольником");
        tool_icon(ui, PhotoTool::Crop, "Кадр", "Кадрирование по выделению");
        tool_icon(ui, PhotoTool::Eyedropper, "Пипетка", "Пипетка");
        ui.separator();
        tool_icon(ui, PhotoTool::Brush, "Кисть", "Кисть по растровому слою или изображению");
        tool_icon(ui, PhotoTool::Eraser, "Ластик", "Ластик по прозрачности");
        tool_icon(ui, PhotoTool::Dodge, "Осветлить", "Осветлить");
        tool_icon(ui, PhotoTool::Burn, "Затемнить", "Затемнить");
        tool_icon(ui, PhotoTool::Blur, "Размытие", "Размыть локально");
        tool_icon(ui, PhotoTool::Sharpen, "Шакализатор", "Локально усиливает резкость и детали");
    });
}

fn tool_icon(ui: &mut egui::Ui, tool: PhotoTool, icon: &str, tip: &str) {
    let active = active_photo_tool() == tool;
    let text_color = if active {
        egui::Color32::from_rgb(0x1a, 0x19, 0x12)
    } else {
        egui::Color32::WHITE
    };
    let button = egui::Button::new(egui::RichText::new(icon).size(12.0).color(text_color))
        .min_size(egui::vec2(104.0, 28.0))
        .fill(if active { egui::Color32::from_rgb(0xcc, 0xc1, 0x00) } else { egui::Color32::from_rgb(0x1a, 0x19, 0x12) });
    if ui.add(button).on_hover_text(tip).clicked() {
        set_active_photo_tool(tool);
    }
}

fn draw_layers_and_inspector(ui: &mut egui::Ui, photo_doc: &mut PhotoDocument) {
    ui.spacing_mut().item_spacing = egui::vec2(6.0, 5.0);

    draw_color_panel(ui);
    ui.separator();
    draw_properties_panel(ui, photo_doc);
    ui.separator();
    draw_adjustments_panel(ui, photo_doc);
    ui.separator();
    draw_diagnostics_panel(ui, photo_doc);
    ui.separator();
    draw_layer_stack_panel(ui, photo_doc);
}

fn draw_color_panel(ui: &mut egui::Ui) {
    ui.heading("Цвет и кисть");
    ui.group(|ui| {
        BRUSH_SETTINGS.with(|settings| {
            let mut brush = *settings.borrow();
            ui.horizontal(|ui| {
                ui.color_edit_button_rgba_unmultiplied(&mut brush.color);
                ui.add(egui::Slider::new(&mut brush.size, 1.0..=512.0).text("Размер, пикс."));
            });
            ui.add(egui::Slider::new(&mut brush.opacity, 0.0..=1.0).text("Непрозрачность"));
            ui.add(egui::Slider::new(&mut brush.flow, 0.01..=1.0).text("Поток"));
            ui.add(egui::Slider::new(&mut brush.hardness, 0.0..=1.0).text("Жёсткость"));
            ui.add(egui::Slider::new(&mut brush.spacing, 0.05..=1.0).text("Интервал"));
            ui.add(egui::Slider::new(&mut brush.strength, 0.01..=1.0).text("Экспозиция / сила"));

            ui.horizontal_wrapped(|ui| {
                ui.label("Диапазон осветления/затемнения:");
                for range in [RetouchToneRange::Shadows, RetouchToneRange::Midtones, RetouchToneRange::Highlights] {
                    if ui.selectable_label(brush.retouch_range == range, range.label()).clicked() {
                        brush.retouch_range = range;
                    }
                }
            });
            ui.checkbox(&mut brush.protect_tones, "Защищать тона");

            ui.horizontal(|ui| {
                if ui.small_button("Чёрный").on_hover_text("Основной цвет: чёрный (D)").clicked() { brush.color = [0.0, 0.0, 0.0, 1.0]; }
                if ui.small_button("Белый").clicked() { brush.color = [1.0, 1.0, 1.0, 1.0]; }
                if ui.small_button("Красный").clicked() { brush.color = [1.0, 0.0, 0.0, 1.0]; }
                if ui.small_button("Синий").clicked() { brush.color = [0.0, 0.25, 1.0, 1.0]; }
                if ui.small_button("Сбросить кисть").clicked() { brush = BrushSettings::default(); }
            });
            *settings.borrow_mut() = brush;
        });
    });
}

fn draw_properties_panel(ui: &mut egui::Ui, photo_doc: &mut PhotoDocument) {
    ui.heading("Свойства");
    ui.group(|ui| {
        ui.horizontal(|ui| {
            ui.label("Документ");
            ui.add(egui::DragValue::new(&mut photo_doc.width).speed(1).clamp_range(1..=100_000).prefix("W "));
            ui.add(egui::DragValue::new(&mut photo_doc.height).speed(1).clamp_range(1..=100_000).prefix("H "));
        });

        if let Some(layer) = selected_layer_mut(photo_doc) {
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("X"); ui.add(egui::DragValue::new(&mut layer.x).speed(1.0));
                ui.label("Y"); ui.add(egui::DragValue::new(&mut layer.y).speed(1.0));
            });
            ui.horizontal(|ui| {
                ui.label("Масштаб"); ui.add(egui::DragValue::new(&mut layer.scale).speed(0.01));
                ui.label("°"); ui.add(egui::DragValue::new(&mut layer.rotation).speed(1.0));
            });
            ui.add(egui::Slider::new(&mut layer.opacity, 0.0..=1.0).text("Непрозрачность"));
            draw_layer_inspector(ui, layer);
        } else {
            ui.label("Выбери слой на холсте или в списке слоёв.");
        }
    });
}

fn draw_adjustments_panel(ui: &mut egui::Ui, photo_doc: &mut PhotoDocument) {
    ui.heading("Действия");
    ui.collapsing("Эффекты слоя", |ui| {
        let selected_id = photo_doc.selected_layer.clone();
        let is_image = selected_layer(photo_doc).map(|l| matches!(l.kind, PhotoLayerKind::Image { .. })).unwrap_or(false);
        if let Some(id) = selected_id {
            draw_runtime_fx_inspector(ui, &id, is_image);
        } else {
            ui.label("Нет выбранного слоя");
        }
    });

    ui.collapsing("Трансформация", |ui| {
        ui.horizontal_wrapped(|ui| {
            if ui.button("Лево").clicked() { nudge_selected_layer(photo_doc, -1.0, 0.0); }
            if ui.button("Право").clicked() { nudge_selected_layer(photo_doc, 1.0, 0.0); }
            if ui.button("Верх").clicked() { nudge_selected_layer(photo_doc, 0.0, -1.0); }
            if ui.button("Низ").clicked() { nudge_selected_layer(photo_doc, 0.0, 1.0); }
            if ui.button("Повернуть на -15°").clicked() { rotate_selected_layer(photo_doc, -15.0); }
            if ui.button("Повернуть на +15°").clicked() { rotate_selected_layer(photo_doc, 15.0); }
            if ui.button("Повернуть на -90°").clicked() { rotate_selected_layer(photo_doc, -90.0); }
            if ui.button("Повернуть на +90°").clicked() { rotate_selected_layer(photo_doc, 90.0); }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("50%").clicked() { scale_selected_layer(photo_doc, 0.5); }
            if ui.button("90%").clicked() { scale_selected_layer(photo_doc, 0.9); }
            if ui.button("110%").clicked() { scale_selected_layer(photo_doc, 1.1); }
            if ui.button("200%").clicked() { scale_selected_layer(photo_doc, 2.0); }
            if ui.button("Центр").clicked() { center_selected_layer(photo_doc); }
            if ui.button("Вписать").clicked() { fit_selected_layer_to_canvas(photo_doc); }
        });
    });

    ui.collapsing("Действия как в Photoshop", |ui| {
        ui.horizontal_wrapped(|ui| {
            if ui.button("Инверсия").clicked() { invert_selected_image(photo_doc); }
            if ui.button("Обесцветить").clicked() { desaturate_selected_image(photo_doc); }
            if ui.button("Автоконтраст").clicked() { auto_contrast_selected_image(photo_doc); }
            if ui.button("Автоуровни").clicked() { auto_levels_selected_image(photo_doc); }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("Порог").clicked() { threshold_selected_image(photo_doc, 128); }
            if ui.button("Постеризация").clicked() { posterize_selected_image(photo_doc, 4); }
            if ui.button("Сплошная заливка").clicked() { add_solid_fill_layer(photo_doc); }
            if ui.button("Градиент").clicked() { add_foreground_to_transparent_gradient_layer(photo_doc); }
            if ui.button("Мемный текст").clicked() { apply_meme_text_style(photo_doc); }
        });
    });

    ui.collapsing("Холст и экспорт", |ui| {
        ui.horizontal_wrapped(|ui| {
            if ui.button("Применить кадрирование").on_hover_text("Применить кадрирование (Enter)").clicked() { crop_document_to_active_selection(photo_doc); }
            if ui.button("Обрезать по слою").clicked() { crop_document_to_selected_layer(photo_doc); }
            if ui.button("Обрезать пустые края").clicked() { trim_document_to_visible_layers(photo_doc); }
            if ui.button("Снять выделение").clicked() { clear_active_selection(photo_doc); }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("Повернуть на 90° вправо").clicked() { rotate_canvas_90(photo_doc, true); }
            if ui.button("Повернуть на 90° влево").clicked() { rotate_canvas_90(photo_doc, false); }
            if ui.button("Отразить по горизонтали").clicked() { flip_canvas(photo_doc, Axis::X); }
            if ui.button("Отразить по вертикали").clicked() { flip_canvas(photo_doc, Axis::Y); }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("Применить эффекты").clicked() { bake_selected_image_filters_to_png(photo_doc); }
            if ui.button("Экспорт PNG").clicked() { export_visible_layers_to_png(photo_doc); }
        });
    });
}


fn draw_diagnostics_panel(ui: &mut egui::Ui, photo_doc: &PhotoDocument) {
    ui.collapsing("Логи / зависания", |ui| {
        ui.label("Лог пишет: запуск, ошибки, долгие операции, сохранение кисти, лаги кисти и контрольные точки.");
        ui.monospace(photo_log_path().display().to_string());
        ui.horizontal_wrapped(|ui| {
            if ui.button("Записать контрольную точку").clicked() {
                write_diagnostic_checkpoint(photo_doc, "manual side-panel checkpoint");
            }
            if ui.button("Очистить недавние").clicked() {
                clear_recent_photo_logs();
            }
            if ui.button("Очистить файл лога").clicked() {
                let _ = std::fs::remove_file(photo_log_path());
                clear_recent_photo_logs();
                append_photo_log("INFO", "log file cleared from UI");
            }
        });
        let lines = recent_photo_logs();
        if lines.is_empty() {
            ui.label("Пока нет записей.");
        } else {
            egui::ScrollArea::vertical()
                .id_source("photo_diagnostics_recent_log_scroll")
                .max_height(130.0)
                .show(ui, |ui| {
                    for line in lines.iter().rev().take(16) {
                        ui.monospace(line);
                    }
                });
        }
    });
}

fn draw_layer_stack_panel(ui: &mut egui::Ui, photo_doc: &mut PhotoDocument) {
    ui.heading("Слои");

    ui.group(|ui| {
        ui.horizontal_wrapped(|ui| {
            if ui.small_button("Текст").on_hover_text("Текст").clicked() { add_text_layer(photo_doc); }
            if ui.small_button("Изобр.").on_hover_text("Изображение").clicked() { add_image_layer(photo_doc); }
            if ui.small_button("Растр").on_hover_text("Растровый слой").clicked() { add_blank_raster_layer(photo_doc); }
            if ui.small_button("Дубль").on_hover_text("Дублировать слой (Ctrl+J)").clicked() { duplicate_selected_layer(photo_doc); }
            if ui.small_button("Удалить").on_hover_text("Удалить").clicked() { delete_selected_layer(photo_doc); }
            if ui.small_button("Верх").on_hover_text("Выше").clicked() { move_selected_layer(photo_doc, 1); }
            if ui.small_button("Низ").on_hover_text("Ниже").clicked() { move_selected_layer(photo_doc, -1); }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.small_button("Показать все").clicked() { set_all_layers_visibility(photo_doc, true); }
            if ui.small_button("Скрыть все").clicked() { set_all_layers_visibility(photo_doc, false); }
            if ui.small_button("Наверх").clicked() { move_selected_layer_to_end(photo_doc); }
            if ui.small_button("Вниз").clicked() { move_selected_layer_to_start(photo_doc); }
            if ui.small_button("Объединить").on_hover_text("Объединить с нижним").clicked() { merge_selected_layer_down(photo_doc); }
            if ui.small_button("Свести").on_hover_text("Свести видимые").clicked() { flatten_visible_layers(photo_doc); }
        });

        ui.separator();
        let mut clicked_layer: Option<String> = None;
        for layer in photo_doc.layers.iter_mut().rev() {
            let selected = photo_doc.selected_layer.as_deref() == Some(layer.id.as_str());
            let row_fill = if selected { egui::Color32::from_rgb(0xcc, 0xc1, 0x00) } else { egui::Color32::from_rgb(0x1a, 0x19, 0x12) };
            egui::Frame::none().fill(row_fill).inner_margin(egui::Margin::same(4.0)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut layer.visible, "");
                    let kind = match &layer.kind {
                        PhotoLayerKind::Text { .. } => "Текст",
                        PhotoLayerKind::Image { .. } => "Изобр.",
                        PhotoLayerKind::Effect { .. } => "Эффекты",
                    };
                    ui.label(kind);
                    if ui.selectable_label(selected, layer.name.clone()).clicked() {
                        clicked_layer = Some(layer.id.clone());
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(format!("{:.0}%", layer.opacity.clamp(0.0, 1.0) * 100.0));
                    });
                });
            });
        }
        if let Some(id) = clicked_layer { photo_doc.selected_layer = Some(id); }
    });
}

fn draw_canvas(ui: &mut egui::Ui, photo_doc: &mut PhotoDocument) {
    let view_key = format!("{}:{}x{}", photo_doc.name, photo_doc.width, photo_doc.height);
    let mut view = CANVAS_VIEW_CACHE.with(|cache| {
        cache
            .borrow_mut()
            .entry(view_key.clone())
            .or_insert_with(CanvasViewState::default)
            .to_owned()
    });

    egui::Frame::none()
        .fill(egui::Color32::from_rgb(0x1a, 0x19, 0x12))
        .inner_margin(egui::Margin::symmetric(8.0, 4.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!(
                    "{} @ {:.0}%  (Слой, RGB/8)",
                    photo_doc.name,
                    view.zoom * 100.0
                ));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.checkbox(&mut view.show_guides, "напр.");
                    ui.checkbox(&mut view.show_grid, "сетка");
                    ui.checkbox(&mut view.show_checker, "прозр.");
                    if ui.small_button("Увеличить").clicked() {
                        view.zoom = (view.zoom * 1.25).clamp(MIN_CAMERA_ZOOM, MAX_CAMERA_ZOOM);
                    }
                    if ui.small_button("Уменьшить").clicked() {
                        view.zoom = (view.zoom / 1.25).clamp(MIN_CAMERA_ZOOM, MAX_CAMERA_ZOOM);
                    }
                    if ui.small_button("100%").clicked() {
                        view.zoom = 1.0;
                        view.pan = egui::Vec2::ZERO;
                        view.initialized = true;
                    }
                    if ui.small_button("Вписать").clicked() {
                        fit_camera_to_view(ui.available_size_before_wrap(), photo_doc, &mut view);
                    }
                });
            });
        });

    let mut viewport_size = ui.available_size_before_wrap();
    viewport_size.x = viewport_size.x.max(300.0);
    viewport_size.y = (viewport_size.y - 24.0).max(300.0);
    if !view.initialized {
        fit_camera_to_view(viewport_size, photo_doc, &mut view);
    }

    let (viewport_rect, response) = ui.allocate_exact_size(viewport_size, egui::Sense::click_and_drag());
    let painter = ui.painter_at(viewport_rect);

    painter.rect_filled(viewport_rect, 0.0, egui::Color32::from_rgb(0x1a, 0x19, 0x12));

    handle_camera_input(ui, photo_doc, viewport_rect, &response, &mut view);

    let canvas_rect = canvas_screen_rect(viewport_rect, photo_doc, &view);

    painter.rect_filled(canvas_rect, 0.0, egui::Color32::from_rgb(0x1a, 0x19, 0x12));
    painter.rect_stroke(
        canvas_rect,
        0.0,
        egui::Stroke::new(1.0, egui::Color32::from_gray(95)),
    );

    if view.show_checker {
        draw_transparency_checker(&painter, canvas_rect, view.zoom);
    }
    if view.show_grid {
        draw_canvas_grid(&painter, canvas_rect, view.zoom);
    }
    if view.show_guides {
        draw_canvas_guides(&painter, canvas_rect);
    }

    let space_down = ui.ctx().input(|input| input.key_down(egui::Key::Space));
    let middle_down = ui
        .ctx()
        .input(|input| input.pointer.button_down(egui::PointerButton::Middle));
    let mut active_tool = active_photo_tool();
    if ui.ctx().input(|input| input.modifiers.alt) {
        active_tool = match active_tool {
            PhotoTool::Dodge => PhotoTool::Burn,
            PhotoTool::Burn => PhotoTool::Dodge,
            other => other,
        };
    }
    let camera_dragging = space_down || middle_down || active_tool == PhotoTool::Hand;

    let handle_is_dragging = if active_tool == PhotoTool::Move {
        draw_selected_overlay_and_handles(ui, &painter, canvas_rect, view.zoom, photo_doc)
    } else {
        false
    };

    let primary_down = ui.ctx().input(|input| input.pointer.button_down(egui::PointerButton::Primary));
    let latest_pointer_pos = ui.ctx().input(|input| input.pointer.latest_pos());

    let primary_pressed = ui.ctx().input(|input| input.pointer.button_pressed(egui::PointerButton::Primary));
    let primary_released = ui.ctx().input(|input| input.pointer.button_released(egui::PointerButton::Primary));
    let primary_was_down = CANVAS_POINTER_STATE.with(|store| store.borrow().primary_was_down);
    let primary_released_robust = primary_released || (primary_was_down && !primary_down);

    if response.clicked() && !camera_dragging {
        if let Some(pointer_pos) = response.interact_pointer_pos() {
            if canvas_rect.contains(pointer_pos) {
                let doc_pos = clamp_doc_pos(photo_doc, screen_to_doc(canvas_rect, view.zoom, pointer_pos));
                match active_tool {
                    PhotoTool::Move => photo_doc.selected_layer = find_top_layer_at_doc_pos(photo_doc, doc_pos),
                    PhotoTool::Eyedropper => pick_color_from_visible_layers(photo_doc, doc_pos),
                    _ => {}
                }
            }
        }
    }

    // Robust Photoshop-like canvas input. Do not rely only on egui's drag_started/dragged:
    // those can be missed when the pointer starts over child handles or leaves the canvas.
    // We track press/down/release manually so Marquee, Crop and Brush strokes are persistent.
    let mut pointer_action = CANVAS_POINTER_STATE.with(|store| store.borrow().action);

    if primary_pressed && !camera_dragging && !handle_is_dragging {
        if let Some(pointer_pos) = latest_pointer_pos {
            if canvas_rect.contains(pointer_pos) {
                let doc_pos = clamp_doc_pos(photo_doc, screen_to_doc(canvas_rect, view.zoom, pointer_pos));
                match active_tool {
                    PhotoTool::Move => {
                        push_undo_checkpoint(photo_doc, "Move/select layer");
                        if let Some(hit_layer) = find_top_layer_at_doc_pos(photo_doc, doc_pos) {
                            photo_doc.selected_layer = Some(hit_layer);
                            if ui.ctx().input(|input| input.modifiers.alt) {
                                duplicate_selected_layer_at_same_position(photo_doc);
                            }
                        }
                        pointer_action = CanvasPointerAction::None;
                    }
                    PhotoTool::Marquee => {
                        begin_document_selection(photo_doc, doc_pos);
                        pointer_action = CanvasPointerAction::Marquee;
                    }
                    PhotoTool::Crop => {
                        begin_document_selection(photo_doc, doc_pos);
                        pointer_action = CanvasPointerAction::Crop;
                    }
                    PhotoTool::Brush | PhotoTool::Eraser | PhotoTool::Dodge | PhotoTool::Burn | PhotoTool::Blur | PhotoTool::Sharpen => {
                        begin_brush_stroke(ui.ctx(), photo_doc, doc_pos, active_tool);
                        pointer_action = CanvasPointerAction::Paint;
                    }
                    PhotoTool::Eyedropper => {
                        pick_color_from_visible_layers(photo_doc, doc_pos);
                        pointer_action = CanvasPointerAction::None;
                    }
                    PhotoTool::Hand => {
                        pointer_action = CanvasPointerAction::None;
                    }
                }
            }
        }
    }

    if primary_down && !camera_dragging && !handle_is_dragging {
        if let Some(pointer_pos) = latest_pointer_pos {
            let inside_canvas = canvas_rect.contains(pointer_pos);
            let doc_pos = clamp_doc_pos(photo_doc, screen_to_doc(canvas_rect, view.zoom, pointer_pos));
            match pointer_action {
                CanvasPointerAction::Marquee | CanvasPointerAction::Crop => {
                    if selection_state(photo_doc).dragging {
                        update_document_selection(photo_doc, doc_pos, ui.ctx().input(|input| input.modifiers.shift));
                        ui.ctx().request_repaint();
                    }
                }
                CanvasPointerAction::Paint => {
                    if inside_canvas {
                        update_brush_stroke(ui.ctx(), photo_doc, doc_pos, active_tool);
                        ui.ctx().request_repaint();
                    }
                }
                CanvasPointerAction::None => {
                    if active_tool == PhotoTool::Move && response.dragged_by(egui::PointerButton::Primary) {
                        let (mut pointer_delta, shift_down) = ui.ctx().input(|input| (input.pointer.delta(), input.modifiers.shift));
                        if pointer_delta != egui::Vec2::ZERO {
                            if shift_down {
                                if pointer_delta.x.abs() >= pointer_delta.y.abs() {
                                    pointer_delta.y = 0.0;
                                } else {
                                    pointer_delta.x = 0.0;
                                }
                            }
                            if let Some(layer) = selected_layer_mut(photo_doc) {
                                layer.x += pointer_delta.x / view.zoom;
                                layer.y += pointer_delta.y / view.zoom;
                            }
                            ui.ctx().request_repaint();
                        }
                    }
                }
            }
        }
    }

    if primary_released_robust || (!primary_down && pointer_action != CanvasPointerAction::None) {
        // Use the last pointer position on release as the final rectangle/stroke point.
        // This makes Crop/Marquee finish even when egui does not deliver a final drag event.
        if let Some(pointer_pos) = latest_pointer_pos {
            let doc_pos = clamp_doc_pos(photo_doc, screen_to_doc(canvas_rect, view.zoom, pointer_pos));
            match pointer_action {
                CanvasPointerAction::Marquee | CanvasPointerAction::Crop => {
                    if selection_state(photo_doc).dragging {
                        update_document_selection(photo_doc, doc_pos, ui.ctx().input(|input| input.modifiers.shift));
                    }
                }
                CanvasPointerAction::Paint => {
                    update_brush_stroke(ui.ctx(), photo_doc, doc_pos, active_tool);
                }
                CanvasPointerAction::None => {}
            }
        }

        match pointer_action {
            CanvasPointerAction::Marquee => finish_document_selection(photo_doc, PhotoTool::Marquee),
            CanvasPointerAction::Crop => finish_document_selection(photo_doc, PhotoTool::Crop),
            CanvasPointerAction::Paint => finish_brush_stroke(photo_doc),
            CanvasPointerAction::None => {}
        }
        pointer_action = CanvasPointerAction::None;
    }

    CANVAS_POINTER_STATE.with(|store| {
        let mut state = store.borrow_mut();
        state.primary_was_down = primary_down;
        state.action = pointer_action;
    });

    for layer in &photo_doc.layers {
        if !layer.visible {
            continue;
        }
        draw_layer(ui.ctx(), &painter, canvas_rect, view.zoom, layer);
    }

    draw_document_selection_overlay(&painter, canvas_rect, view.zoom, photo_doc);

    draw_active_tool_cursor(ui, &painter, canvas_rect, view.zoom, active_tool);

    if active_tool == PhotoTool::Move {
        draw_selected_overlay_and_handles(ui, &painter, canvas_rect, view.zoom, photo_doc);
    }

    ui.horizontal(|ui| {
        ui.label(format!("{:.0}%", view.zoom * 100.0));
        ui.separator();
        ui.label(format!("{} пикс. × {} пикс.", photo_doc.width, photo_doc.height));
        ui.separator();
        ui.label(format!("{}  |  C: рамка кадрирования → Enter применить / Esc отменить, M: выделение, Shift: квадрат, [/] размер кисти", active_photo_tool().label()));
    });

    CANVAS_VIEW_CACHE.with(|cache| {
        cache.borrow_mut().insert(view_key, view);
    });
}

fn handle_camera_input(
    ui: &egui::Ui,
    photo_doc: &PhotoDocument,
    viewport_rect: egui::Rect,
    response: &egui::Response,
    view: &mut CanvasViewState,
) {
    let (smooth_scroll, raw_scroll, zoom_delta, modifiers, pointer_pos, pointer_delta, space_down, middle_down) = ui.ctx().input(|input| {
        (
            input.smooth_scroll_delta,
            input.raw_scroll_delta,
            input.zoom_delta(),
            input.modifiers,
            input.pointer.hover_pos(),
            input.pointer.delta(),
            input.key_down(egui::Key::Space),
            input.pointer.button_down(egui::PointerButton::Middle),
        )
    });

    let pointer_inside = pointer_pos.map(|p| viewport_rect.contains(p)).unwrap_or(false);
    if !pointer_inside && !response.hovered() {
        return;
    }

    let command_or_ctrl = modifiers.command || modifiers.ctrl;
    let scroll = if raw_scroll != egui::Vec2::ZERO { raw_scroll } else { smooth_scroll };

    // Real Photoshop-like zoom:
    // - Ctrl/Cmd + mouse wheel zooms around the cursor.
    // - egui/OS sometimes reports Ctrl+wheel as zoom_delta instead of scroll_delta, so both are handled.
    let mut zoom_factor = 1.0_f32;
    if command_or_ctrl && scroll != egui::Vec2::ZERO {
        let wheel = if scroll.y.abs() >= scroll.x.abs() { scroll.y } else { scroll.x };
        zoom_factor = (wheel * 0.0025).exp().clamp(0.20, 5.0);
    } else if (zoom_delta - 1.0).abs() > 0.001 {
        zoom_factor = zoom_delta.clamp(0.20, 5.0);
    }

    if (zoom_factor - 1.0).abs() > 0.001 {
        let pointer = pointer_pos.unwrap_or_else(|| viewport_rect.center());
        let old_canvas = canvas_screen_rect(viewport_rect, photo_doc, view);
        let doc_under_pointer = screen_to_doc(old_canvas, view.zoom, pointer);
        let old_zoom = view.zoom;
        view.zoom = (view.zoom * zoom_factor).clamp(MIN_CAMERA_ZOOM, MAX_CAMERA_ZOOM);
        if (view.zoom - old_zoom).abs() > f32::EPSILON {
            let new_canvas = canvas_screen_rect(viewport_rect, photo_doc, view);
            let screen_after = doc_to_screen(new_canvas, view.zoom, doc_under_pointer);
            view.pan += pointer - screen_after;
            view.initialized = true;
            ui.ctx().request_repaint();
        }
        return;
    }

    if scroll != egui::Vec2::ZERO {
        if modifiers.shift {
            view.pan.x += scroll.y + scroll.x;
        } else {
            view.pan.y += scroll.y;
            view.pan.x += scroll.x;
        }
        view.initialized = true;
    }

    if response.dragged() && (middle_down || space_down || active_photo_tool() == PhotoTool::Hand) {
        view.pan += pointer_delta;
        view.initialized = true;
        ui.ctx().request_repaint();
    }
}

fn fit_camera_to_view(available: egui::Vec2, photo_doc: &PhotoDocument, view: &mut CanvasViewState) {
    let doc_w = photo_doc.width.max(1) as f32;
    let doc_h = photo_doc.height.max(1) as f32;
    let margin = 64.0;
    let fit_w = (available.x - margin).max(32.0) / doc_w;
    let fit_h = (available.y - margin).max(32.0) / doc_h;
    view.zoom = fit_w.min(fit_h).clamp(MIN_CAMERA_ZOOM, MAX_CAMERA_ZOOM);
    view.pan = egui::Vec2::ZERO;
    view.initialized = true;
}

fn canvas_screen_rect(
    viewport_rect: egui::Rect,
    photo_doc: &PhotoDocument,
    view: &CanvasViewState,
) -> egui::Rect {
    let doc_size = egui::vec2(
        photo_doc.width.max(1) as f32 * view.zoom,
        photo_doc.height.max(1) as f32 * view.zoom,
    );
    egui::Rect::from_center_size(viewport_rect.center() + view.pan, doc_size)
}

fn add_text_layer(photo_doc: &mut PhotoDocument) {
    push_undo_checkpoint(photo_doc, "New text layer");
    let id = make_unique_layer_id(photo_doc, "text");

    photo_doc.layers.push(PhotoLayer {
        id: id.clone(),
        name: "Текстовый слой".to_string(),
        visible: true,
        opacity: 1.0,
        x: 120.0,
        y: 120.0,
        scale: 1.0,
        rotation: 0.0,
        kind: PhotoLayerKind::Text {
            text: "Текст".to_string(),
            size: 64.0,
            color: [1.0, 1.0, 1.0, 1.0],
        },
    });

    photo_doc.selected_layer = Some(id);
}

fn add_image_layer(photo_doc: &mut PhotoDocument) {
    let picked_path = rfd::FileDialog::new()
        .add_filter("Изображение", &["png", "jpg", "jpeg", "webp", "bmp"])
        .pick_file();

    let Some(path) = picked_path else {
        return;
    };

    push_undo_checkpoint(photo_doc, "Place image layer");

    let id = make_unique_layer_id(photo_doc, "image");
    let filename = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("Слой изображения")
        .to_string();

    let path_string = path.display().to_string();
    let image_size = image_dimensions_cached(&path_string).unwrap_or(egui::vec2(DEFAULT_IMAGE_W, DEFAULT_IMAGE_H));
    let doc_w = photo_doc.width.max(1) as f32;
    let doc_h = photo_doc.height.max(1) as f32;
    let fit = (doc_w * 0.55 / image_size.x.max(1.0)).min(doc_h * 0.55 / image_size.y.max(1.0));
    let scale = fit.min(1.0).clamp(MIN_LAYER_SCALE, MAX_LAYER_SCALE);

    photo_doc.layers.push(PhotoLayer {
        id: id.clone(),
        name: filename,
        visible: true,
        opacity: 1.0,
        x: (doc_w - image_size.x * scale) * 0.5,
        y: (doc_h - image_size.y * scale) * 0.5,
        scale,
        rotation: 0.0,
        kind: PhotoLayerKind::Image { path: path_string },
    });

    photo_doc.selected_layer = Some(id);
}


fn add_blank_raster_layer(photo_doc: &mut PhotoDocument) {
    push_undo_checkpoint(photo_doc, "New raster layer");
    let id = make_unique_layer_id(photo_doc, "raster");
    let edit_dir = std::env::temp_dir().join("memstroy_photo_editor_raster_layers");
    if std::fs::create_dir_all(&edit_dir).is_err() {
        return;
    }
    let safe_name = photo_doc
        .name
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect::<String>();
    let target = edit_dir.join(format!("{}_{}.png", safe_name, id));
    let image = image::RgbaImage::from_pixel(
        photo_doc.width.max(1),
        photo_doc.height.max(1),
        image::Rgba([0, 0, 0, 0]),
    );
    if image.save(&target).is_err() {
        return;
    }

    photo_doc.layers.push(PhotoLayer {
        id: id.clone(),
        name: "Растровый слой".to_string(),
        visible: true,
        opacity: 1.0,
        x: 0.0,
        y: 0.0,
        scale: 1.0,
        rotation: 0.0,
        kind: PhotoLayerKind::Image { path: target.display().to_string() },
    });
    photo_doc.selected_layer = Some(id);
    IMAGE_TEXTURE_CACHE.with(|cache| cache.borrow_mut().clear());
    IMAGE_DIM_CACHE.with(|cache| cache.borrow_mut().clear());
}

fn duplicate_selected_layer(photo_doc: &mut PhotoDocument) {
    duplicate_selected_layer_with_offset(photo_doc, egui::vec2(25.0, 25.0));
}

fn duplicate_selected_layer_at_same_position(photo_doc: &mut PhotoDocument) {
    duplicate_selected_layer_with_offset(photo_doc, egui::Vec2::ZERO);
}

fn duplicate_selected_layer_with_offset(photo_doc: &mut PhotoDocument, offset: egui::Vec2) {
    push_undo_checkpoint(photo_doc, "Duplicate layer");
    let Some(selected_id) = photo_doc.selected_layer.clone() else {
        return;
    };

    let Some(layer) = photo_doc.layers.iter().find(|l| l.id == selected_id).cloned() else {
        return;
    };

    let mut new_layer = layer;
    new_layer.id = make_unique_layer_id(photo_doc, "copy");
    new_layer.name = format!("{} копия", new_layer.name);
    new_layer.x += offset.x;
    new_layer.y += offset.y;

    let new_id = new_layer.id.clone();
    photo_doc.layers.push(new_layer);
    photo_doc.selected_layer = Some(new_id);
}

fn delete_selected_layer(photo_doc: &mut PhotoDocument) {
    push_undo_checkpoint(photo_doc, "Delete layer");
    let Some(selected_id) = photo_doc.selected_layer.clone() else {
        return;
    };

    photo_doc.layers.retain(|layer| layer.id != selected_id);
    photo_doc.selected_layer = photo_doc.layers.last().map(|layer| layer.id.clone());
}

fn move_selected_layer(photo_doc: &mut PhotoDocument, direction: i32) {
    push_undo_checkpoint(photo_doc, "Move layer in stack");
    let Some(selected_id) = photo_doc.selected_layer.clone() else {
        return;
    };

    let Some(index) = photo_doc.layers.iter().position(|layer| layer.id == selected_id) else {
        return;
    };

    let new_index = if direction > 0 {
        (index + 1).min(photo_doc.layers.len().saturating_sub(1))
    } else {
        index.saturating_sub(1)
    };

    if index != new_index {
        photo_doc.layers.swap(index, new_index);
    }
}

fn move_selected_layer_to_end(photo_doc: &mut PhotoDocument) {
    push_undo_checkpoint(photo_doc, "Move layer to top");
    let Some(selected_id) = photo_doc.selected_layer.clone() else {
        return;
    };
    let Some(index) = photo_doc.layers.iter().position(|layer| layer.id == selected_id) else {
        return;
    };

    let layer = photo_doc.layers.remove(index);
    photo_doc.layers.push(layer);
}

fn move_selected_layer_to_start(photo_doc: &mut PhotoDocument) {
    push_undo_checkpoint(photo_doc, "Move layer to bottom");
    let Some(selected_id) = photo_doc.selected_layer.clone() else {
        return;
    };
    let Some(index) = photo_doc.layers.iter().position(|layer| layer.id == selected_id) else {
        return;
    };

    let layer = photo_doc.layers.remove(index);
    photo_doc.layers.insert(0, layer);
}

fn selected_layer_mut(photo_doc: &mut PhotoDocument) -> Option<&mut PhotoLayer> {
    let selected_id = photo_doc.selected_layer.clone()?;
    photo_doc.layers.iter_mut().find(|layer| layer.id == selected_id)
}

fn selected_layer(photo_doc: &PhotoDocument) -> Option<&PhotoLayer> {
    let selected_id = photo_doc.selected_layer.as_ref()?;
    photo_doc.layers.iter().find(|layer| &layer.id == selected_id)
}

fn draw_layer_inspector(ui: &mut egui::Ui, layer: &mut PhotoLayer) {
    ui.horizontal(|ui| {
        ui.label("Имя");
        ui.text_edit_singleline(&mut layer.name);
        ui.checkbox(&mut layer.visible, "Видим.");
    });

    normalize_layer_transform(layer);

    match &mut layer.kind {
        PhotoLayerKind::Text { text, size, color } => {
            ui.separator();
            ui.label("Текст");
            ui.text_edit_multiline(text);
            ui.add(egui::Slider::new(size, 6.0..=500.0).text("Размер"));
            ui.color_edit_button_rgba_unmultiplied(color);
            ui.horizontal(|ui| {
                if ui.small_button("Белый").clicked() { *color = [1.0, 1.0, 1.0, 1.0]; }
                if ui.small_button("Чёрный").clicked() { *color = [0.0, 0.0, 0.0, 1.0]; }
                if ui.small_button("Красный").clicked() { *color = [1.0, 0.0, 0.0, 1.0]; }
            });
        }
        PhotoLayerKind::Image { path } => {
            ui.separator();
            ui.label("Изображение");
            let name = std::path::Path::new(path)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(path.as_str());
            ui.label(name);
            if let Some(size) = image_dimensions_cached(path) {
                ui.label(format!("{:.0} × {:.0} px", size.x, size.y));
            } else {
                ui.colored_label(egui::Color32::from_rgb(255, 150, 110), "файл не найден");
            }
            if ui.button("Заменить...").clicked() {
                if let Some(new_path) = rfd::FileDialog::new()
                    .add_filter("Изображение", &["png", "jpg", "jpeg", "webp", "bmp"])
                    .pick_file()
                {
                    *path = new_path.display().to_string();
                    IMAGE_TEXTURE_CACHE.with(|cache| cache.borrow_mut().clear());
                    IMAGE_DIM_CACHE.with(|cache| cache.borrow_mut().clear());
                }
            }
        }
        PhotoLayerKind::Effect { .. } => {
            ui.separator();
            ui.label("Слой эффектов");
        }
    }
}

fn draw_canvas_grid(painter: &egui::Painter, rect: egui::Rect, zoom: f32) {
    let grid_step = pick_grid_step(zoom);
    let stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(42));

    let mut x = rect.left();
    while x <= rect.right() {
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            stroke,
        );
        x += grid_step;
    }

    let mut y = rect.top();
    while y <= rect.bottom() {
        painter.line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            stroke,
        );
        y += grid_step;
    }
}

fn pick_grid_step(zoom: f32) -> f32 {
    let mut doc_step = 100.0;
    while doc_step * zoom < 24.0 {
        doc_step *= 2.0;
    }
    while doc_step * zoom > 120.0 {
        doc_step *= 0.5;
    }
    (doc_step * zoom).max(8.0)
}

fn draw_canvas_guides(painter: &egui::Painter, rect: egui::Rect) {
    let stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(68));
    let center = rect.center();
    painter.line_segment(
        [egui::pos2(center.x, rect.top()), egui::pos2(center.x, rect.bottom())],
        stroke,
    );
    painter.line_segment(
        [egui::pos2(rect.left(), center.y), egui::pos2(rect.right(), center.y)],
        stroke,
    );
}

fn draw_transparency_checker(painter: &egui::Painter, rect: egui::Rect, zoom: f32) {
    let step = (16.0 * zoom).clamp(8.0, 32.0);
    let mut y = rect.top();
    let mut row = 0;

    while y < rect.bottom() {
        let mut x = rect.left();
        let mut col = 0;
        while x < rect.right() {
            let fill = if (row + col) % 2 == 0 {
                egui::Color32::from_gray(28)
            } else {
                egui::Color32::from_gray(36)
            };
            painter.rect_filled(
                egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(step, step)),
                0.0,
                fill,
            );
            x += step;
            col += 1;
        }
        y += step;
        row += 1;
    }
}

fn draw_layer(
    ctx: &egui::Context,
    painter: &egui::Painter,
    canvas_rect: egui::Rect,
    zoom: f32,
    layer: &PhotoLayer,
) {
    let pos = doc_to_screen(canvas_rect, zoom, egui::pos2(layer.x, layer.y));
    let opacity = layer.opacity.clamp(0.0, 1.0);
    let fx = layer_runtime_fx(&layer.id);

    match &layer.kind {
        PhotoLayerKind::Text { text, size, color } => {
            draw_text_layer(painter, pos, text, *size, *color, layer.scale, layer.rotation, opacity, zoom, &fx);
        }
        PhotoLayerKind::Image { path } => {
            draw_image_layer(ctx, painter, canvas_rect, zoom, layer, path, opacity, &fx);
        }
        PhotoLayerKind::Effect { .. } => {
            let size = egui::vec2(EFFECT_PLACEHOLDER_W, EFFECT_PLACEHOLDER_H) * layer.scale * zoom;
            let rect = egui::Rect::from_min_size(pos, size);
            draw_rotated_rect(
                painter,
                rect,
                layer.rotation.to_radians(),
                egui::Color32::from_rgba_unmultiplied(
                    90,
                    60,
                    120,
                    (180.0 * opacity).round().clamp(0.0, 255.0) as u8,
                ),
                egui::Stroke::new(1.0, egui::Color32::from_rgb(170, 130, 210)),
            );
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Слой эффектов",
                egui::FontId::proportional(18.0 * zoom.max(0.5)),
                egui::Color32::WHITE,
            );
        }
    }
}

fn draw_text_layer(
    painter: &egui::Painter,
    pos: egui::Pos2,
    text: &str,
    size: f32,
    color: [f32; 4],
    layer_scale: f32,
    rotation_deg: f32,
    opacity: f32,
    zoom: f32,
    fx: &LayerRuntimeFx,
) {
    let color32 = rgba_array_to_color32(color, opacity);
    let font_size = (size * layer_scale * zoom).max(6.0);
    let angle = rotation_deg.to_radians();

    // v5: текст теперь вращается вокруг центра собственного блока, а не вокруг левого верхнего края.
    // Размер блока считается тем же приближением, что и layer_bounds_doc(), поэтому круглый rotate-handle
    // визуально стоит над серединой текста и реально крутит вокруг середины.
    if fx.shadow_enabled && fx.shadow_opacity > 0.0 {
        let shadow_top_left = pos + egui::vec2(fx.shadow_dx * zoom, fx.shadow_dy * zoom);
        let shadow_color = egui::Color32::from_black_alpha((255.0 * fx.shadow_opacity.clamp(0.0, 1.0) * opacity).round() as u8);
        draw_text_lines_center_pivot(painter, shadow_top_left, text, font_size, angle, shadow_color);
    }

    if fx.stroke_enabled && fx.stroke_width > 0.0 {
        let stroke_color = rgba_array_to_color32(fx.stroke_color, opacity * fx.stroke_color[3].clamp(0.0, 1.0));
        let step = (fx.stroke_width * zoom).max(1.0);
        let offsets = [
            egui::vec2(-step, 0.0),
            egui::vec2(step, 0.0),
            egui::vec2(0.0, -step),
            egui::vec2(0.0, step),
            egui::vec2(-step, -step),
            egui::vec2(step, -step),
            egui::vec2(-step, step),
            egui::vec2(step, step),
        ];
        for offset in offsets {
            draw_text_lines_center_pivot(painter, pos + offset, text, font_size, angle, stroke_color);
        }
    }

    draw_text_lines_center_pivot(painter, pos, text, font_size, angle, color32);
}

fn draw_text_lines_center_pivot(
    painter: &egui::Painter,
    top_left: egui::Pos2,
    text: &str,
    font_size: f32,
    angle: f32,
    color: egui::Color32,
) {
    let lines: Vec<&str> = if text.is_empty() { vec![""] } else { text.lines().collect() };
    let line_height = font_size * 1.15;

    if angle.abs() < 0.001 {
        for (line_index, line) in lines.iter().enumerate() {
            painter.text(
                top_left + egui::vec2(0.0, line_index as f32 * line_height),
                egui::Align2::LEFT_TOP,
                *line,
                egui::FontId::proportional(font_size),
                color,
            );
        }
        return;
    }

    let max_chars = lines.iter().map(|line| line.chars().count()).max().unwrap_or(1).max(1);
    let block_size = egui::vec2(
        (max_chars as f32 * font_size * 0.58).max(20.0),
        (lines.len().max(1) as f32 * line_height).max(20.0),
    );
    let pivot = top_left + block_size * 0.5;

    for (line_index, line) in lines.iter().enumerate() {
        let local = egui::vec2(-block_size.x * 0.5, -block_size.y * 0.5 + line_index as f32 * line_height);
        let line_pos = pivot + rotate_vec2(local, angle);
        let galley = painter.layout_no_wrap((*line).to_owned(), egui::FontId::proportional(font_size), color);
        let mut shape = egui::epaint::TextShape::new(line_pos, galley, color);
        shape.angle = angle;
        painter.add(shape);
    }
}

fn draw_image_layer(
    ctx: &egui::Context,
    painter: &egui::Painter,
    canvas_rect: egui::Rect,
    zoom: f32,
    layer: &PhotoLayer,
    path: &str,
    opacity: f32,
    fx: &LayerRuntimeFx,
) {
    let Some(cached) = load_image_texture(ctx, path, fx) else {
        draw_missing_image_placeholder(painter, canvas_rect, zoom, layer, path, opacity);
        return;
    };

    let doc_size = cached.size * layer.scale;
    let screen_size = doc_size * zoom;
    let pos = doc_to_screen(canvas_rect, zoom, egui::pos2(layer.x, layer.y));
    let rect = egui::Rect::from_min_size(pos, screen_size);
    let angle = layer.rotation.to_radians();

    if fx.shadow_enabled && fx.shadow_opacity > 0.0 {
        let shadow_rect = rect.translate(egui::vec2(fx.shadow_dx * zoom, fx.shadow_dy * zoom));
        // Preview shadows must use the image alpha, not a filled rectangle. This keeps
        // paint layers with transparent padding usable for shadows/overlays.
        draw_rotated_texture(
            painter,
            shadow_rect,
            angle,
            cached.texture.id(),
            egui::Color32::from_black_alpha((255.0 * fx.shadow_opacity.clamp(0.0, 1.0) * opacity).round() as u8),
        );
    }

    draw_rotated_texture(
        painter,
        rect,
        angle,
        cached.texture.id(),
        egui::Color32::from_white_alpha((255.0 * opacity).round().clamp(0.0, 255.0) as u8),
    );

    if fx.stroke_enabled && fx.stroke_width > 0.0 {
        let points = rotated_rect_points(rect, angle);
        for edge in 0..4 {
            painter.line_segment(
                [points[edge], points[(edge + 1) % 4]],
                egui::Stroke::new(fx.stroke_width * zoom, rgba_array_to_color32(fx.stroke_color, opacity)),
            );
        }
    }
}

fn draw_missing_image_placeholder(
    painter: &egui::Painter,
    canvas_rect: egui::Rect,
    zoom: f32,
    layer: &PhotoLayer,
    path: &str,
    opacity: f32,
) {
    let size = egui::vec2(DEFAULT_IMAGE_W, DEFAULT_IMAGE_H) * layer.scale * zoom;
    let pos = doc_to_screen(canvas_rect, zoom, egui::pos2(layer.x, layer.y));
    let rect = egui::Rect::from_min_size(pos, size);

    draw_rotated_rect(
        painter,
        rect,
        layer.rotation.to_radians(),
        egui::Color32::from_rgba_unmultiplied(80, 45, 45, (220.0 * opacity).round().clamp(0.0, 255.0) as u8),
        egui::Stroke::new(1.0, egui::Color32::from_rgb(220, 120, 120)),
    );

    let label = std::path::Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("Missing image");

    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        format!("Изображение не загружено\n{}", label),
        egui::FontId::proportional(14.0 * zoom.max(0.65)),
        egui::Color32::WHITE,
    );
}

fn load_image_texture(ctx: &egui::Context, path: &str, fx: &LayerRuntimeFx) -> Option<CachedTexture> {
    let path_key = normalize_path_key(path);
    let key = format!("{}::{}", path_key, fx.image_cache_suffix());

    if let Some(cached) = IMAGE_TEXTURE_CACHE.with(|cache| cache.borrow().get(&key).cloned()) {
        return Some(cached);
    }

    let mut dyn_image = image::open(path).ok()?;
    let original_w = dyn_image.width().max(1);
    let original_h = dyn_image.height().max(1);
    let fast_filter_preview = has_expensive_layer_preview_fx(fx);

    // Быстрый предпросмотр: размытие и Шакализатор очень дорогие на полном размере.
    // В окне редактора считаем маленькую копию и растягиваем её до реального размера слоя.
    // При "Применить эффекты" и экспорте используется полный размер через load_baked_image_rgba().
    if fast_filter_preview {
        dyn_image = downscale_dynamic_for_fast_preview(dyn_image, MAX_EXPENSIVE_FILTER_PREVIEW_PIXELS);
    }

    if fx.flip_x {
        dyn_image = dyn_image.fliph();
    }
    if fx.flip_y {
        dyn_image = dyn_image.flipv();
    }
    if fx.grayscale {
        dyn_image = dyn_image.grayscale();
    }
    if fx.invert {
        dyn_image.invert();
    }
    if fx.brightness.abs() > 0.01 {
        dyn_image = dyn_image.brighten(fx.brightness.round().clamp(-255.0, 255.0) as i32);
    }
    if fx.contrast.abs() > 0.01 {
        dyn_image = dyn_image.adjust_contrast(fx.contrast.clamp(-100.0, 100.0));
    }
    if fx.hue_rotate.abs() > 0.1 {
        dyn_image = dyn_image.huerotate(fx.hue_rotate.round() as i32);
    }
    if fx.blur > 0.01 {
        let preview_blur = if fast_filter_preview {
            // На уменьшенном предпросмотре большой радиус визуально уже заметен,
            // а полный радиус 40 может подвесить интерфейс во время движения слайдера.
            (fx.blur * 0.35).clamp(0.0, 8.0)
        } else {
            fx.blur.clamp(0.0, 40.0)
        };
        dyn_image = dyn_image.blur(preview_blur);
    }
    if fx.sharpen > 0.01 {
        let preview_sharpen = if fast_filter_preview {
            (fx.sharpen * 0.25).clamp(0.0, 3.0)
        } else {
            fx.sharpen.clamp(0.0, 20.0)
        };
        dyn_image = dyn_image.unsharpen(preview_sharpen, 1);
    }
    if fx.pixelate > 1.01 {
        let factor = fx.pixelate.clamp(1.0, 64.0);
        let small_w = ((dyn_image.width() as f32 / factor).round() as u32).max(1);
        let small_h = ((dyn_image.height() as f32 / factor).round() as u32).max(1);
        let rgba = dyn_image.to_rgba8();
        let small = image::imageops::resize(&rgba, small_w, small_h, image::imageops::FilterType::Nearest);
        let large = image::imageops::resize(&small, dyn_image.width(), dyn_image.height(), image::imageops::FilterType::Nearest);
        dyn_image = image::DynamicImage::ImageRgba8(large);
    }

    let mut rgba = dyn_image.to_rgba8();
    apply_runtime_color_matrix(&mut rgba, fx);
    let (width, height) = rgba.dimensions();
    if width == 0 || height == 0 {
        return None;
    }

    let color_image = egui::ColorImage::from_rgba_unmultiplied(
        [width as usize, height as usize],
        rgba.as_raw(),
    );

    let texture = ctx.load_texture(
        format!("photo_editor_image::{}", key),
        color_image,
        egui::TextureOptions::LINEAR,
    );

    let cached = CachedTexture {
        texture,
        size: egui::vec2(original_w as f32, original_h as f32),
    };

    IMAGE_TEXTURE_CACHE.with(|cache| {
        cache.borrow_mut().insert(key, cached.clone());
    });
    IMAGE_DIM_CACHE.with(|cache| {
        cache.borrow_mut().insert(path_key, cached.size);
    });

    Some(cached)
}

fn apply_runtime_color_matrix(image: &mut image::RgbaImage, fx: &LayerRuntimeFx) {
    let saturation = fx.saturation.clamp(0.0, 4.0);
    let tint_amount = fx.tint_amount.clamp(0.0, 1.0);
    let tint = [
        fx.tint[0].clamp(0.0, 1.0),
        fx.tint[1].clamp(0.0, 1.0),
        fx.tint[2].clamp(0.0, 1.0),
    ];

    if (saturation - 1.0).abs() <= 0.001 && tint_amount <= 0.001 {
        return;
    }

    for pixel in image.pixels_mut() {
        let mut r = pixel[0] as f32 / 255.0;
        let mut g = pixel[1] as f32 / 255.0;
        let mut b = pixel[2] as f32 / 255.0;
        let luma = r * 0.2126 + g * 0.7152 + b * 0.0722;
        r = (luma + (r - luma) * saturation).clamp(0.0, 1.0);
        g = (luma + (g - luma) * saturation).clamp(0.0, 1.0);
        b = (luma + (b - luma) * saturation).clamp(0.0, 1.0);

        if tint_amount > 0.001 {
            r = (r * (1.0 - tint_amount) + tint[0] * tint_amount).clamp(0.0, 1.0);
            g = (g * (1.0 - tint_amount) + tint[1] * tint_amount).clamp(0.0, 1.0);
            b = (b * (1.0 - tint_amount) + tint[2] * tint_amount).clamp(0.0, 1.0);
        }

        pixel[0] = (r * 255.0).round() as u8;
        pixel[1] = (g * 255.0).round() as u8;
        pixel[2] = (b * 255.0).round() as u8;
    }
}

fn image_dimensions_cached(path: &str) -> Option<egui::Vec2> {
    let key = normalize_path_key(path);

    if let Some(size) = IMAGE_DIM_CACHE.with(|cache| cache.borrow().get(&key).copied()) {
        return Some(size);
    }

    let (width, height) = image::image_dimensions(path).ok()?;
    let size = egui::vec2(width as f32, height as f32);
    IMAGE_DIM_CACHE.with(|cache| {
        cache.borrow_mut().insert(key, size);
    });
    Some(size)
}

fn normalize_path_key(path: &str) -> String {
    std::path::Path::new(path)
        .canonicalize()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| path.to_string())
}

fn draw_selected_overlay_and_handles(
    ui: &mut egui::Ui,
    painter: &egui::Painter,
    canvas_rect: egui::Rect,
    zoom: f32,
    photo_doc: &mut PhotoDocument,
) -> bool {
    let Some(layer) = selected_layer(photo_doc).cloned() else {
        return false;
    };

    if !layer.visible {
        return false;
    }

    let bounds_doc = layer_bounds_doc(&layer);
    let bounds = doc_rect_to_screen(canvas_rect, zoom, bounds_doc);
    let points = rotated_rect_points(bounds, layer.rotation.to_radians());

    for edge in 0..4 {
        painter.line_segment(
            [points[edge], points[(edge + 1) % 4]],
            egui::Stroke::new(2.0, egui::Color32::from_rgb(0xcc, 0xc1, 0x00)),
        );
    }

    // v5: visible transform pivot. Rotation uses this center for text, images and effect placeholders.
    painter.circle_stroke(
        bounds.center(),
        4.0,
        egui::Stroke::new(1.5, egui::Color32::from_rgb(255, 230, 120)),
    );

    let handle_size = 10.0;
    let scale_handle = egui::Rect::from_center_size(points[2], egui::vec2(handle_size, handle_size));
    painter.rect_filled(scale_handle, 1.0, egui::Color32::from_rgb(0xcc, 0xc1, 0x00));

    let top_center = midpoint(points[0], points[1]);
    let mut dir = top_center - bounds.center();
    if dir.length_sq() <= 0.0001 {
        dir = egui::vec2(0.0, -1.0);
    } else {
        dir = dir.normalized();
    }
    let rotate_center = top_center + dir * 30.0;
    let rotate_handle = egui::Rect::from_center_size(rotate_center, egui::vec2(16.0, 16.0));
    painter.circle_filled(rotate_center, 7.0, egui::Color32::from_rgb(255, 190, 75));
    painter.line_segment(
        [top_center, rotate_center],
        egui::Stroke::new(1.0, egui::Color32::from_rgb(255, 190, 75)),
    );

    let id = egui::Id::new("selected_layer_handles").with(layer.id.clone());
    let scale_response = ui.interact(scale_handle, id.with("scale"), egui::Sense::drag());
    let rotate_response = ui.interact(rotate_handle, id.with("rotate"), egui::Sense::drag());

    if scale_response.dragged() {
        let pointer_delta = ui.ctx().input(|input| input.pointer.delta());
        if let Some(active_layer) = selected_layer_mut(photo_doc) {
            let amount = (pointer_delta.x + pointer_delta.y) * 0.006 / zoom.max(0.001);
            active_layer.scale = (active_layer.scale + amount).clamp(MIN_LAYER_SCALE, MAX_LAYER_SCALE);
        }
        return true;
    }

    if rotate_response.dragged() {
        let Some(pointer) = rotate_response.interact_pointer_pos() else {
            return true;
        };
        if let Some(active_layer) = selected_layer_mut(photo_doc) {
            let active_size = layer_size_doc(active_layer);
            let center = doc_to_screen(
                canvas_rect,
                zoom,
                egui::pos2(
                    active_layer.x + active_size.x * 0.5,
                    active_layer.y + active_size.y * 0.5,
                ),
            );
            let v = pointer - center;
            active_layer.rotation = v.y.atan2(v.x).to_degrees() + 90.0;
            normalize_layer_transform(active_layer);
        }
        return true;
    }

    false
}

fn draw_rotated_texture(
    painter: &egui::Painter,
    rect: egui::Rect,
    angle: f32,
    texture_id: egui::TextureId,
    color: egui::Color32,
) {
    let points = rotated_rect_points(rect, angle);
    let uvs = [
        egui::pos2(0.0, 0.0),
        egui::pos2(1.0, 0.0),
        egui::pos2(1.0, 1.0),
        egui::pos2(0.0, 1.0),
    ];

    let mut mesh = egui::epaint::Mesh::with_texture(texture_id);
    for i in 0..4 {
        mesh.vertices.push(egui::epaint::Vertex {
            pos: points[i],
            uv: uvs[i],
            color,
        });
    }
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    painter.add(egui::Shape::mesh(mesh));
}

fn draw_rotated_rect(
    painter: &egui::Painter,
    rect: egui::Rect,
    angle: f32,
    fill: egui::Color32,
    stroke: egui::Stroke,
) {
    let points = rotated_rect_points(rect, angle);
    painter.add(egui::Shape::convex_polygon(points.to_vec(), fill, stroke));
}

fn rotated_rect_points(rect: egui::Rect, angle: f32) -> [egui::Pos2; 4] {
    let center = rect.center();
    let corners = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
    ];

    let sin = angle.sin();
    let cos = angle.cos();
    let mut out = [center; 4];

    for (index, point) in corners.iter().enumerate() {
        let dx = point.x - center.x;
        let dy = point.y - center.y;
        out[index] = egui::pos2(center.x + dx * cos - dy * sin, center.y + dx * sin + dy * cos);
    }

    out
}

fn rotate_vec2(v: egui::Vec2, angle: f32) -> egui::Vec2 {
    let sin = angle.sin();
    let cos = angle.cos();
    egui::vec2(v.x * cos - v.y * sin, v.x * sin + v.y * cos)
}

fn midpoint(a: egui::Pos2, b: egui::Pos2) -> egui::Pos2 {
    egui::pos2((a.x + b.x) * 0.5, (a.y + b.y) * 0.5)
}

fn layer_bounds_doc(layer: &PhotoLayer) -> egui::Rect {
    let size = layer_size_doc(layer);
    egui::Rect::from_min_size(egui::pos2(layer.x, layer.y), size)
}

fn image_layer_bounds_doc_from_dims(layer: &PhotoLayer, image_w: u32, image_h: u32) -> egui::Rect {
    // Do not use cached dimensions while painting. During a live brush stroke the backing
    // PNG may be expanded in memory before it is saved to disk, so IMAGE_DIM_CACHE can be
    // stale. Pixel tools must use the actual working image dimensions they receive.
    let scale = layer.scale.max(MIN_LAYER_SCALE);
    egui::Rect::from_min_size(
        egui::pos2(layer.x, layer.y),
        egui::vec2(image_w.max(1) as f32 * scale, image_h.max(1) as f32 * scale),
    )
}

fn layer_size_doc(layer: &PhotoLayer) -> egui::Vec2 {
    match &layer.kind {
        PhotoLayerKind::Text { text, size, .. } => {
            let max_chars = text.lines().map(|line| line.chars().count()).max().unwrap_or(1).max(1);
            let line_count = text.lines().count().max(1);
            let width = max_chars as f32 * *size * 0.58 * layer.scale;
            let height = line_count as f32 * *size * 1.15 * layer.scale;
            egui::vec2(width.max(20.0), height.max(20.0))
        }
        PhotoLayerKind::Image { path } => {
            let base = image_dimensions_cached(path).unwrap_or(egui::vec2(DEFAULT_IMAGE_W, DEFAULT_IMAGE_H));
            egui::vec2(base.x * layer.scale, base.y * layer.scale)
        }
        PhotoLayerKind::Effect { .. } => egui::vec2(
            EFFECT_PLACEHOLDER_W * layer.scale,
            EFFECT_PLACEHOLDER_H * layer.scale,
        ),
    }
}

fn doc_rect_to_screen(canvas_rect: egui::Rect, zoom: f32, rect: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_max(
        doc_to_screen(canvas_rect, zoom, rect.min),
        doc_to_screen(canvas_rect, zoom, rect.max),
    )
}

fn doc_to_screen(canvas_rect: egui::Rect, zoom: f32, pos: egui::Pos2) -> egui::Pos2 {
    canvas_rect.left_top() + egui::vec2(pos.x * zoom, pos.y * zoom)
}

fn screen_to_doc(canvas_rect: egui::Rect, zoom: f32, pos: egui::Pos2) -> egui::Pos2 {
    let delta = pos - canvas_rect.left_top();
    egui::pos2(delta.x / zoom, delta.y / zoom)
}

fn find_top_layer_at_doc_pos(photo_doc: &PhotoDocument, pos: egui::Pos2) -> Option<String> {
    for layer in photo_doc.layers.iter().rev() {
        if !layer.visible {
            continue;
        }
        if layer_contains_doc_pos(layer, pos) {
            return Some(layer.id.clone());
        }
    }
    None
}

fn layer_contains_doc_pos(layer: &PhotoLayer, pos: egui::Pos2) -> bool {
    let rect = layer_bounds_doc(layer);
    let center = rect.center();
    let local = pos - center;
    let unrotated = rotate_vec2(local, -layer.rotation.to_radians());
    let test = center + unrotated;
    rect.contains(test)
}

fn layer_runtime_fx(layer_id: &str) -> LayerRuntimeFx {
    LAYER_RUNTIME_FX.with(|store| {
        store
            .borrow_mut()
            .entry(layer_id.to_string())
            .or_insert_with(LayerRuntimeFx::default)
            .clone()
    })
}


fn photo_log_path() -> std::path::PathBuf {
    std::env::temp_dir()
        .join("memstroy_photo_editor_logs")
        .join("photo_editor.log")
}

fn install_photo_editor_logging_once() {
    PHOTO_EDITOR_LOG_INIT.call_once(|| {
        append_photo_log("INFO", &format!("{} session started", EDITOR_BUILD));
        std::panic::set_hook(Box::new(|panic_info| {
            append_photo_log("PANIC", &format!("photo editor panic: {}", panic_info));
        }));
    });
}

fn append_photo_log(level: &str, message: &str) {
    let timestamp = chrono_like_stamp();
    let line = format!("[{timestamp}] [{level}] {message}");
    PHOTO_RECENT_LOGS.with(|logs| {
        let mut logs = logs.borrow_mut();
        logs.push(line.clone());
        if logs.len() > PHOTO_LOG_RECENT_LIMIT {
            let remove_count = logs.len() - PHOTO_LOG_RECENT_LIMIT;
            logs.drain(0..remove_count);
        }
    });
    let path = photo_log_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "{}", line);
    }
}

fn recent_photo_logs() -> Vec<String> {
    PHOTO_RECENT_LOGS.with(|logs| logs.borrow().clone())
}

fn clear_recent_photo_logs() {
    PHOTO_RECENT_LOGS.with(|logs| logs.borrow_mut().clear());
}

fn log_operation_duration(operation: &str, elapsed: std::time::Duration) {
    let ms = elapsed.as_millis();
    if ms >= VERY_SLOW_OPERATION_ERROR_MS {
        append_photo_log("ERROR", &format!("very slow operation: {operation} took {ms} ms"));
    } else if ms >= SLOW_OPERATION_WARN_MS {
        append_photo_log("WARN", &format!("slow operation: {operation} took {ms} ms"));
    }
}

fn write_diagnostic_checkpoint(photo_doc: &PhotoDocument, reason: &str) {
    let (undo_count, redo_count) = history_counts(photo_doc);
    let selected = photo_doc.selected_layer.clone().unwrap_or_else(|| "<none>".to_string());
    append_photo_log("INFO", &format!(
        "checkpoint: {reason}; doc='{}' {}x{}, layers={}, selected={}, undo={}, redo={}, tool={}",
        photo_doc.name,
        photo_doc.width,
        photo_doc.height,
        photo_doc.layers.len(),
        selected,
        undo_count,
        redo_count,
        active_photo_tool().label()
    ));
}

fn selected_or_document_rect(photo_doc: &PhotoDocument) -> egui::Rect {
    selection_state(photo_doc)
        .rect()
        .map(|rect| clamp_rect_to_document(photo_doc, rect))
        .unwrap_or_else(|| egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(photo_doc.width.max(1) as f32, photo_doc.height.max(1) as f32),
        ))
}

fn add_solid_fill_layer(photo_doc: &mut PhotoDocument) {
    push_undo_checkpoint(photo_doc, "Solid fill layer");
    let rect = selected_or_document_rect(photo_doc);
    if rect.width() <= 0.0 || rect.height() <= 0.0 { return; }
    let color = BRUSH_SETTINGS.with(|store| store.borrow().color);
    let width = rect.width().ceil().max(1.0) as u32;
    let height = rect.height().ceil().max(1.0) as u32;
    let px = image::Rgba([
        (color[0].clamp(0.0, 1.0) * 255.0).round() as u8,
        (color[1].clamp(0.0, 1.0) * 255.0).round() as u8,
        (color[2].clamp(0.0, 1.0) * 255.0).round() as u8,
        (color[3].clamp(0.0, 1.0) * 255.0).round() as u8,
    ]);
    let image = image::RgbaImage::from_pixel(width, height, px);
    if let Some(id) = add_generated_image_layer(photo_doc, "Solid Fill", "solid_fill", rect.min, image) {
        photo_doc.selected_layer = Some(id);
        append_photo_log("INFO", "solid fill layer added");
    }
}

fn add_foreground_to_transparent_gradient_layer(photo_doc: &mut PhotoDocument) {
    push_undo_checkpoint(photo_doc, "Gradient layer");
    let rect = selected_or_document_rect(photo_doc);
    if rect.width() <= 0.0 || rect.height() <= 0.0 { return; }
    let color = BRUSH_SETTINGS.with(|store| store.borrow().color);
    let width = rect.width().ceil().max(1.0) as u32;
    let height = rect.height().ceil().max(1.0) as u32;
    let mut image = image::RgbaImage::from_pixel(width, height, image::Rgba([0, 0, 0, 0]));
    for y in 0..height {
        let _ = y;
        for x in 0..width {
            let t = if width <= 1 { 1.0 } else { 1.0 - (x as f32 / (width - 1) as f32) };
            image.put_pixel(x, y, image::Rgba([
                (color[0].clamp(0.0, 1.0) * 255.0).round() as u8,
                (color[1].clamp(0.0, 1.0) * 255.0).round() as u8,
                (color[2].clamp(0.0, 1.0) * 255.0).round() as u8,
                (color[3].clamp(0.0, 1.0) * t * 255.0).round().clamp(0.0, 255.0) as u8,
            ]));
        }
    }
    if let Some(id) = add_generated_image_layer(photo_doc, "Gradient Fill", "gradient", rect.min, image) {
        photo_doc.selected_layer = Some(id);
        append_photo_log("INFO", "gradient layer added");
    }
}

fn add_generated_image_layer(
    photo_doc: &mut PhotoDocument,
    name: &str,
    prefix: &str,
    pos: egui::Pos2,
    image: image::RgbaImage,
) -> Option<String> {
    let dir = std::env::temp_dir().join("memstroy_photo_editor_generated_layers");
    std::fs::create_dir_all(&dir).ok()?;
    let id = make_unique_layer_id(photo_doc, prefix);
    let safe_name = photo_doc.name.chars().map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' }).collect::<String>();
    let path = dir.join(format!("{}_{}_{}.png", safe_name, prefix, chrono_like_stamp()));
    image.save(&path).ok()?;
    photo_doc.layers.push(PhotoLayer {
        id: id.clone(),
        name: name.to_string(),
        visible: true,
        opacity: 1.0,
        x: pos.x,
        y: pos.y,
        scale: 1.0,
        rotation: 0.0,
        kind: PhotoLayerKind::Image { path: path.display().to_string() },
    });
    IMAGE_TEXTURE_CACHE.with(|cache| cache.borrow_mut().clear());
    IMAGE_DIM_CACHE.with(|cache| cache.borrow_mut().clear());
    Some(id)
}

fn apply_selected_image_pixels(photo_doc: &mut PhotoDocument, label: &str, mut f: impl FnMut(&mut image::RgbaImage)) {
    push_undo_checkpoint(photo_doc, label);
    let Some(layer) = selected_image_layer_mut(photo_doc) else {
        append_photo_log("WARN", &format!("{label}: no selected image layer"));
        return;
    };
    let Some(path) = ensure_selected_image_editable_png(layer) else {
        append_photo_log("ERROR", &format!("{label}: cannot prepare editable PNG"));
        return;
    };
    let start = std::time::Instant::now();
    let Ok(mut rgba) = image::open(&path).map(|img| img.to_rgba8()) else {
        append_photo_log("ERROR", &format!("{label}: cannot open {}", path.display()));
        return;
    };
    f(&mut rgba);
    match rgba.save(&path) {
        Ok(_) => {
            IMAGE_TEXTURE_CACHE.with(|cache| cache.borrow_mut().clear());
            IMAGE_DIM_CACHE.with(|cache| cache.borrow_mut().clear());
            append_photo_log("INFO", &format!("{label}: applied to {}", path.display()));
            log_operation_duration(label, start.elapsed());
        }
        Err(err) => append_photo_log("ERROR", &format!("{label}: save failed: {err}")),
    }
}

fn invert_selected_image(photo_doc: &mut PhotoDocument) {
    apply_selected_image_pixels(photo_doc, "Invert image", |rgba| {
        for p in rgba.pixels_mut() {
            p[0] = 255 - p[0];
            p[1] = 255 - p[1];
            p[2] = 255 - p[2];
        }
    });
}

fn desaturate_selected_image(photo_doc: &mut PhotoDocument) {
    apply_selected_image_pixels(photo_doc, "Desaturate image", |rgba| {
        for p in rgba.pixels_mut() {
            let gray = (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32).round() as u8;
            p[0] = gray;
            p[1] = gray;
            p[2] = gray;
        }
    });
}

fn auto_contrast_selected_image(photo_doc: &mut PhotoDocument) {
    apply_selected_image_pixels(photo_doc, "Auto contrast", |rgba| {
        let mut min_v = 255u8;
        let mut max_v = 0u8;
        for p in rgba.pixels() {
            if p[3] == 0 { continue; }
            min_v = min_v.min(p[0]).min(p[1]).min(p[2]);
            max_v = max_v.max(p[0]).max(p[1]).max(p[2]);
        }
        if max_v <= min_v { return; }
        let scale = 255.0 / (max_v as f32 - min_v as f32);
        for p in rgba.pixels_mut() {
            if p[3] == 0 { continue; }
            for c in 0..3 {
                p[c] = (((p[c].saturating_sub(min_v)) as f32 * scale).round()).clamp(0.0, 255.0) as u8;
            }
        }
    });
}

fn auto_levels_selected_image(photo_doc: &mut PhotoDocument) {
    apply_selected_image_pixels(photo_doc, "Auto levels", |rgba| {
        let mut min_c = [255u8; 3];
        let mut max_c = [0u8; 3];
        for p in rgba.pixels() {
            if p[3] == 0 { continue; }
            for c in 0..3 {
                min_c[c] = min_c[c].min(p[c]);
                max_c[c] = max_c[c].max(p[c]);
            }
        }
        for p in rgba.pixels_mut() {
            if p[3] == 0 { continue; }
            for c in 0..3 {
                if max_c[c] > min_c[c] {
                    let scale = 255.0 / (max_c[c] as f32 - min_c[c] as f32);
                    p[c] = (((p[c].saturating_sub(min_c[c])) as f32 * scale).round()).clamp(0.0, 255.0) as u8;
                }
            }
        }
    });
}

fn threshold_selected_image(photo_doc: &mut PhotoDocument, threshold: u8) {
    apply_selected_image_pixels(photo_doc, "Threshold", |rgba| {
        for p in rgba.pixels_mut() {
            if p[3] == 0 { continue; }
            let gray = (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32).round() as u8;
            let v = if gray >= threshold { 255 } else { 0 };
            p[0] = v;
            p[1] = v;
            p[2] = v;
        }
    });
}

fn posterize_selected_image(photo_doc: &mut PhotoDocument, levels: u8) {
    let levels = levels.clamp(2, 32);
    apply_selected_image_pixels(photo_doc, "Posterize", move |rgba| {
        let denom = (levels - 1).max(1) as f32;
        for p in rgba.pixels_mut() {
            if p[3] == 0 { continue; }
            for c in 0..3 {
                let normalized = p[c] as f32 / 255.0;
                let stepped = (normalized * denom).round() / denom;
                p[c] = (stepped * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    });
}

fn apply_meme_text_style(photo_doc: &mut PhotoDocument) {
    push_undo_checkpoint(photo_doc, "Meme text style");
    let Some(selected_id) = photo_doc.selected_layer.clone() else { return; };
    let Some(layer) = photo_doc.layers.iter_mut().find(|layer| layer.id == selected_id) else { return; };
    let PhotoLayerKind::Text { size, color, .. } = &mut layer.kind else {
        append_photo_log("WARN", "Meme text style: selected layer is not text");
        return;
    };
    *size = (*size).max(72.0);
    *color = [1.0, 1.0, 1.0, 1.0];
    mutate_layer_runtime_fx(&selected_id, |fx| {
        fx.stroke_enabled = true;
        fx.stroke_width = 6.0;
        fx.stroke_color = [0.0, 0.0, 0.0, 1.0];
        fx.shadow_enabled = true;
        fx.shadow_dx = 4.0;
        fx.shadow_dy = 4.0;
        fx.shadow_opacity = 0.45;
    });
    append_photo_log("INFO", "meme text style applied");
}

fn mutate_layer_runtime_fx(layer_id: &str, f: impl FnOnce(&mut LayerRuntimeFx)) {
    LAYER_RUNTIME_FX.with(|store| {
        let mut store = store.borrow_mut();
        let fx = store.entry(layer_id.to_string()).or_insert_with(LayerRuntimeFx::default);
        f(fx);
    });
    // Не очищаем кэш текстур на каждый пиксель движения слайдера.
    // Ключ кэша уже содержит значения эффектов, поэтому старый предпросмотр не ломает новый.
    // Это убирает главный лаг у "Размытия по Гауссу" и "Шакализатора" в панели эффектов.
}

fn reset_layer_runtime_fx(layer_id: &str) {
    LAYER_RUNTIME_FX.with(|store| {
        store.borrow_mut().insert(layer_id.to_string(), LayerRuntimeFx::default());
    });
}

fn draw_runtime_fx_inspector(ui: &mut egui::Ui, layer_id: &str, is_image_layer: bool) {
    let mut fx = layer_runtime_fx(layer_id);
    let mut changed = false;

    ui.collapsing("Стили слоя / предпросмотр эффектов", |ui| {
        changed |= ui.checkbox(&mut fx.shadow_enabled, "Тень").changed();
        if fx.shadow_enabled {
            changed |= ui.add(egui::Slider::new(&mut fx.shadow_dx, -200.0..=200.0).text("Тень X")).changed();
            changed |= ui.add(egui::Slider::new(&mut fx.shadow_dy, -200.0..=200.0).text("Тень Y")).changed();
            changed |= ui.add(egui::Slider::new(&mut fx.shadow_opacity, 0.0..=1.0).text("Непрозрачность тени")).changed();
        }

        changed |= ui.checkbox(&mut fx.stroke_enabled, "Обводка").changed();
        if fx.stroke_enabled {
            changed |= ui.add(egui::Slider::new(&mut fx.stroke_width, 0.0..=64.0).text("Толщина обводки")).changed();
            ui.label("Цвет обводки RGBA");
            changed |= ui.add(egui::Slider::new(&mut fx.stroke_color[0], 0.0..=1.0).text("R")).changed();
            changed |= ui.add(egui::Slider::new(&mut fx.stroke_color[1], 0.0..=1.0).text("G")).changed();
            changed |= ui.add(egui::Slider::new(&mut fx.stroke_color[2], 0.0..=1.0).text("B")).changed();
            changed |= ui.add(egui::Slider::new(&mut fx.stroke_color[3], 0.0..=1.0).text("A")).changed();
        }

        if is_image_layer {
            ui.separator();
            ui.label("Фильтры изображения / коррекция (быстрый предпросмотр, полное качество после применения)");
            ui.horizontal(|ui| {
                changed |= ui.checkbox(&mut fx.flip_x, "Отразить X").changed();
                changed |= ui.checkbox(&mut fx.flip_y, "Отразить Y").changed();
            });
            ui.horizontal(|ui| {
                changed |= ui.checkbox(&mut fx.grayscale, "Ч/б").changed();
                changed |= ui.checkbox(&mut fx.invert, "Инверсия").changed();
            });
            changed |= ui.add(egui::Slider::new(&mut fx.brightness, -255.0..=255.0).text("Яркость")).changed();
            changed |= ui.add(egui::Slider::new(&mut fx.contrast, -100.0..=100.0).text("Контраст")).changed();
            changed |= ui.add(egui::Slider::new(&mut fx.saturation, 0.0..=4.0).text("Насыщенность")).changed();
            changed |= ui.add(egui::Slider::new(&mut fx.hue_rotate, -180.0..=180.0).text("Сдвиг оттенка")).changed();
            changed |= ui.add(egui::Slider::new(&mut fx.blur, 0.0..=40.0).text("Размытие по Гауссу")).changed();
            changed |= ui.add(egui::Slider::new(&mut fx.sharpen, 0.0..=20.0).text("Шакализатор")).changed();
            changed |= ui.add(egui::Slider::new(&mut fx.pixelate, 1.0..=64.0).text("Пикселизация")).changed();
            changed |= ui.add(egui::Slider::new(&mut fx.tint_amount, 0.0..=1.0).text("Сила тонирования")).changed();
            if fx.tint_amount > 0.0 {
                ui.label("Тонирование RGB");
                changed |= ui.add(egui::Slider::new(&mut fx.tint[0], 0.0..=1.0).text("R")).changed();
                changed |= ui.add(egui::Slider::new(&mut fx.tint[1], 0.0..=1.0).text("G")).changed();
                changed |= ui.add(egui::Slider::new(&mut fx.tint[2], 0.0..=1.0).text("B")).changed();
            }
        }

        ui.horizontal(|ui| {
            if ui.button("Сбросить эффекты").clicked() {
                reset_layer_runtime_fx(layer_id);
                changed = false;
            }
            if ui.button("Обычная тень").clicked() {
                fx.shadow_enabled = true;
                fx.shadow_dx = 12.0;
                fx.shadow_dy = 12.0;
                fx.shadow_opacity = 0.45;
                changed = true;
            }
        });
    });

    if changed {
        let new_fx = fx.clone();
        mutate_layer_runtime_fx(layer_id, |stored| *stored = new_fx);
    }
}

fn rgba_array_to_color32(rgba: [f32; 4], opacity: f32) -> egui::Color32 {
    egui::Rgba::from_rgba_unmultiplied(
        rgba[0].clamp(0.0, 1.0),
        rgba[1].clamp(0.0, 1.0),
        rgba[2].clamp(0.0, 1.0),
        (rgba[3].clamp(0.0, 1.0) * opacity.clamp(0.0, 1.0)).clamp(0.0, 1.0),
    )
    .into()
}



fn history_key(photo_doc: &PhotoDocument) -> String {
    photo_doc.name.clone()
}

fn capture_history_images(layers: &[PhotoLayer]) -> Vec<HistoryImageSnapshot> {
    let mut snapshots = Vec::new();
    for layer in layers {
        let PhotoLayerKind::Image { path } = &layer.kind else { continue; };
        let Ok((w, h)) = image::image_dimensions(path) else { continue; };
        let pixels = w as u64 * h as u64;
        if pixels == 0 || pixels > MAX_HISTORY_IMAGE_PIXELS {
            continue;
        }
        let Ok(rgba) = image::open(path).map(|img| img.to_rgba8()) else { continue; };
        snapshots.push(HistoryImageSnapshot { path: path.clone(), rgba });
    }
    snapshots
}

fn snapshot_signature_from_parts(
    name: &str,
    width: u32,
    height: u32,
    layers: &[PhotoLayer],
    selected_layer: &Option<String>,
    fx_by_layer: &HashMap<String, LayerRuntimeFx>,
    selection: SelectionState,
) -> String {
    let mut sig = format!("{}|{}x{}|sel={:?}|selection={:?};", name, width, height, selected_layer, selection);
    for layer in layers {
        sig.push_str(&format!(
            "L:{}:{}:{}:{:.3}:{:.3}:{:.3}:{:.3}:{:.3}:",
            layer.id, layer.name, layer.visible, layer.opacity, layer.x, layer.y, layer.scale, layer.rotation
        ));
        match &layer.kind {
            PhotoLayerKind::Text { text, size, color } => sig.push_str(&format!("T:{}:{:.3}:{:?};", text, size, color)),
            PhotoLayerKind::Image { path } => {
                let meta = std::fs::metadata(path);
                let stamp = meta.ok().and_then(|m| {
                    let len = m.len();
                    let modified = m.modified().ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_nanos())
                        .unwrap_or(0);
                    Some(format!("{}:{}", len, modified))
                }).unwrap_or_else(|| "missing".to_string());
                sig.push_str(&format!("I:{}:{};", path, stamp));
            }
            PhotoLayerKind::Effect { .. } => sig.push_str("E;"),
        }
    }
    let mut fx_keys: Vec<_> = fx_by_layer.keys().cloned().collect();
    fx_keys.sort();
    for key in fx_keys {
        if let Some(fx) = fx_by_layer.get(&key) {
            sig.push_str(&format!(
                "FX:{}:{}:{}:{}:{}:{:.2}:{:.2}:{:.2}:{:.2}:{:.2}:{:.2}:{:?}:{:.2}:{:.2}:{:.2}:{:?};",
                key,
                fx.flip_x,
                fx.flip_y,
                fx.grayscale,
                fx.invert,
                fx.brightness,
                fx.contrast,
                fx.saturation,
                fx.hue_rotate,
                fx.blur,
                fx.sharpen,
                fx.tint,
                fx.tint_amount,
                fx.shadow_dx,
                fx.shadow_dy,
                fx.stroke_color,
            ));
        }
    }
    sig
}

fn capture_editor_snapshot(photo_doc: &PhotoDocument) -> EditorSnapshot {
    let fx_by_layer = LAYER_RUNTIME_FX.with(|store| store.borrow().clone());
    let selection = selection_state(photo_doc);
    let layers = photo_doc.layers.clone();
    let images = capture_history_images(&layers);
    let signature = snapshot_signature_from_parts(
        &photo_doc.name,
        photo_doc.width,
        photo_doc.height,
        &layers,
        &photo_doc.selected_layer,
        &fx_by_layer,
        selection,
    );
    EditorSnapshot {
        name: photo_doc.name.clone(),
        width: photo_doc.width,
        height: photo_doc.height,
        layers,
        selected_layer: photo_doc.selected_layer.clone(),
        fx_by_layer,
        selection,
        images,
        signature,
    }
}

fn restore_editor_snapshot(photo_doc: &mut PhotoDocument, snapshot: &EditorSnapshot) {
    photo_doc.name = snapshot.name.clone();
    photo_doc.width = snapshot.width;
    photo_doc.height = snapshot.height;
    photo_doc.layers = snapshot.layers.clone();
    photo_doc.selected_layer = snapshot.selected_layer.clone();

    for image_snapshot in &snapshot.images {
        let path = std::path::PathBuf::from(&image_snapshot.path);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = image_snapshot.rgba.save(&path);
    }

    LAYER_RUNTIME_FX.with(|store| {
        *store.borrow_mut() = snapshot.fx_by_layer.clone();
    });
    set_selection_state(photo_doc, snapshot.selection);
    IMAGE_TEXTURE_CACHE.with(|cache| cache.borrow_mut().clear());
    IMAGE_DIM_CACHE.with(|cache| cache.borrow_mut().clear());
}

fn push_undo_checkpoint(photo_doc: &PhotoDocument, label: &str) {
    let start = std::time::Instant::now();
    let key = history_key(photo_doc);
    let snapshot = capture_editor_snapshot(photo_doc);
    PHOTO_HISTORY.with(|store| {
        let mut store = store.borrow_mut();
        let history = store.entry(key).or_insert_with(PhotoHistoryState::default);
        if history.undo.last().map(|prev| prev.signature.as_str()) == Some(snapshot.signature.as_str()) {
            return;
        }
        history.undo.push(snapshot);
        if history.undo.len() > MAX_UNDO_STATES {
            history.undo.remove(0);
        }
        history.redo.clear();
    });
    log_operation_duration(&format!("undo checkpoint: {}", label), start.elapsed());
}

fn push_brush_undo_checkpoint(photo_doc: &PhotoDocument, tool: PhotoTool) {
    // Brush/retouch tools can create many tiny strokes per second. Cloning the same
    // raster into History for every micro-stroke is what made v24 feel laggy.
    // Group rapid strokes of the same tool into one undo state, Photoshop-style enough
    // for editing, but much cheaper for the UI thread.
    let now = chrono_like_stamp();
    let key = history_key(photo_doc);
    let should_push = LAST_BRUSH_UNDO_CHECKPOINT.with(|store| {
        let last = store.borrow();
        match last.as_ref() {
            Some((last_key, last_tool, last_ms)) => {
                last_key != &key || *last_tool != tool || now.saturating_sub(*last_ms) > BRUSH_UNDO_GROUP_MS
            }
            None => true,
        }
    });

    if should_push {
        push_undo_checkpoint(photo_doc, "Brush/retouch stroke group");
    }

    LAST_BRUSH_UNDO_CHECKPOINT.with(|store| {
        *store.borrow_mut() = Some((key, tool, now));
    });
}

fn log_brush_update_if_slow(tool: PhotoTool, elapsed: std::time::Duration, radius: i32, stamps: i32) {
    let ms = elapsed.as_millis();
    if ms < BRUSH_UPDATE_WARN_MS {
        return;
    }
    let now = chrono_like_stamp();
    let should_log = LAST_BRUSH_PERF_LOG_MS.with(|store| {
        let mut last = store.borrow_mut();
        if now.saturating_sub(*last) >= 1000 {
            *last = now;
            true
        } else {
            false
        }
    });
    if should_log {
        append_photo_log(
            "WARN",
            &format!("brush update lag: tool={} took {} ms, radius={}, stamps={}", tool.label(), ms, radius, stamps),
        );
    }
}

fn undo_photo_edit(photo_doc: &mut PhotoDocument) -> bool {
    let key = history_key(photo_doc);
    let previous = PHOTO_HISTORY.with(|store| {
        let mut store = store.borrow_mut();
        let history = store.entry(key).or_insert_with(PhotoHistoryState::default);
        let previous = history.undo.pop()?;
        let current = capture_editor_snapshot(photo_doc);
        history.redo.push(current);
        Some(previous)
    });
    if let Some(snapshot) = previous {
        restore_editor_snapshot(photo_doc, &snapshot);
        append_photo_log("INFO", "undo applied");
        true
    } else {
        false
    }
}

fn redo_photo_edit(photo_doc: &mut PhotoDocument) -> bool {
    let key = history_key(photo_doc);
    let next = PHOTO_HISTORY.with(|store| {
        let mut store = store.borrow_mut();
        let history = store.entry(key).or_insert_with(PhotoHistoryState::default);
        let next = history.redo.pop()?;
        let current = capture_editor_snapshot(photo_doc);
        history.undo.push(current);
        if history.undo.len() > MAX_UNDO_STATES {
            history.undo.remove(0);
        }
        Some(next)
    });
    if let Some(snapshot) = next {
        restore_editor_snapshot(photo_doc, &snapshot);
        append_photo_log("INFO", "redo applied");
        true
    } else {
        false
    }
}

fn history_counts(photo_doc: &PhotoDocument) -> (usize, usize) {
    let key = history_key(photo_doc);
    PHOTO_HISTORY.with(|store| {
        let store = store.borrow();
        store.get(&key).map(|h| (h.undo.len(), h.redo.len())).unwrap_or((0, 0))
    })
}

fn copy_selected_layer_to_clipboard(photo_doc: &PhotoDocument) {
    if let Some(layer) = selected_layer(photo_doc).cloned() {
        LAYER_CLIPBOARD.with(|clipboard| *clipboard.borrow_mut() = Some(layer));
    }
}

fn paste_layer_from_clipboard(photo_doc: &mut PhotoDocument) {
    push_undo_checkpoint(photo_doc, "Paste layer");
    let layer = LAYER_CLIPBOARD.with(|clipboard| clipboard.borrow().clone());
    let Some(mut layer) = layer else { return; };
    layer.id = make_unique_layer_id(photo_doc, "paste");
    layer.name = format!("{} Paste", layer.name);
    layer.x += 20.0;
    layer.y += 20.0;
    let new_id = layer.id.clone();
    photo_doc.layers.push(layer);
    photo_doc.selected_layer = Some(new_id);
}

fn cut_selected_layer_to_clipboard(photo_doc: &mut PhotoDocument) {
    push_undo_checkpoint(photo_doc, "Cut layer");
    copy_selected_layer_to_clipboard(photo_doc);
    delete_selected_layer(photo_doc);
}

fn select_all_document(photo_doc: &PhotoDocument) {
    push_undo_checkpoint(photo_doc, "Select all");
    set_selection_state(photo_doc, SelectionState {
        active: true,
        dragging: false,
        start: egui::pos2(0.0, 0.0),
        end: egui::pos2(photo_doc.width.max(1) as f32, photo_doc.height.max(1) as f32),
    });
}

fn save_rendered_layer_png(photo_doc: &PhotoDocument, prefix: &str, image: &image::RgbaImage) -> Option<(std::path::PathBuf, egui::Vec2)> {
    let Some((min_x, min_y, max_x, max_y)) = alpha_bounds(image) else {
        let dir = std::env::temp_dir().join("memstroy_photo_editor_merged_layers");
        std::fs::create_dir_all(&dir).ok()?;
        let path = dir.join(format!("{}_{}_empty.png", photo_doc.name, prefix));
        transparent_canvas(1, 1).save(&path).ok()?;
        return Some((path, egui::vec2(0.0, 0.0)));
    };
    let w = (max_x - min_x + 1).max(1);
    let h = (max_y - min_y + 1).max(1);
    let cropped = image::imageops::crop_imm(image, min_x, min_y, w, h).to_image();
    let dir = std::env::temp_dir().join("memstroy_photo_editor_merged_layers");
    std::fs::create_dir_all(&dir).ok()?;
    let safe_name = photo_doc.name.chars().map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' }).collect::<String>();
    let path = dir.join(format!("{}_{}_{}.png", safe_name, prefix, chrono_like_stamp()));
    cropped.save(&path).ok()?;
    Some((path, egui::vec2(min_x as f32, min_y as f32)))
}

fn chrono_like_stamp() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn merge_selected_layer_down(photo_doc: &mut PhotoDocument) {
    push_undo_checkpoint(photo_doc, "Merge down");
    let Some(selected_id) = photo_doc.selected_layer.clone() else { return; };
    let Some(index) = photo_doc.layers.iter().position(|layer| layer.id == selected_id) else { return; };
    if index == 0 { return; }
    let below_index = index - 1;
    let mut layers = vec![photo_doc.layers[below_index].clone(), photo_doc.layers[index].clone()];
    for layer in &mut layers { layer.visible = true; }
    let fx_by_layer = LAYER_RUNTIME_FX.with(|store| store.borrow().clone());
    let rendered = render_layers_to_rgba(photo_doc.width.max(1), photo_doc.height.max(1), &layers, &fx_by_layer);
    let Some((path, offset)) = save_rendered_layer_png(photo_doc, "merge_down", &rendered) else { return; };
    let id = make_unique_layer_id(photo_doc, "merged");
    let merged = PhotoLayer {
        id: id.clone(),
        name: "Merged Layer".to_string(),
        visible: true,
        opacity: 1.0,
        x: offset.x,
        y: offset.y,
        scale: 1.0,
        rotation: 0.0,
        kind: PhotoLayerKind::Image { path: path.display().to_string() },
    };
    photo_doc.layers.remove(index);
    photo_doc.layers.remove(below_index);
    photo_doc.layers.insert(below_index, merged);
    photo_doc.selected_layer = Some(id);
    IMAGE_TEXTURE_CACHE.with(|cache| cache.borrow_mut().clear());
    IMAGE_DIM_CACHE.with(|cache| cache.borrow_mut().clear());
}

fn flatten_visible_layers(photo_doc: &mut PhotoDocument) {
    push_undo_checkpoint(photo_doc, "Flatten visible");
    if photo_doc.layers.is_empty() { return; }
    let rendered = render_visible_layers_to_rgba(photo_doc);
    let Some((path, offset)) = save_rendered_layer_png(photo_doc, "flatten", &rendered) else { return; };
    let id = make_unique_layer_id(photo_doc, "flattened");
    photo_doc.layers.clear();
    photo_doc.layers.push(PhotoLayer {
        id: id.clone(),
        name: "Flattened Image".to_string(),
        visible: true,
        opacity: 1.0,
        x: offset.x,
        y: offset.y,
        scale: 1.0,
        rotation: 0.0,
        kind: PhotoLayerKind::Image { path: path.display().to_string() },
    });
    photo_doc.selected_layer = Some(id);
    LAYER_RUNTIME_FX.with(|store| store.borrow_mut().clear());
    IMAGE_TEXTURE_CACHE.with(|cache| cache.borrow_mut().clear());
    IMAGE_DIM_CACHE.with(|cache| cache.borrow_mut().clear());
}

fn active_photo_tool() -> PhotoTool {
    ACTIVE_PHOTO_TOOL.with(|tool| *tool.borrow())
}

fn set_active_photo_tool(tool: PhotoTool) {
    ACTIVE_PHOTO_TOOL.with(|store| *store.borrow_mut() = tool);
    append_photo_log("INFO", &format!("tool selected: {}", tool.label()));
}

fn is_paint_tool(tool: PhotoTool) -> bool {
    matches!(
        tool,
        PhotoTool::Brush
            | PhotoTool::Eraser
            | PhotoTool::Dodge
            | PhotoTool::Burn
            | PhotoTool::Blur
            | PhotoTool::Sharpen
    )
}

fn clamp_doc_pos(photo_doc: &PhotoDocument, pos: egui::Pos2) -> egui::Pos2 {
    egui::pos2(
        pos.x.clamp(0.0, photo_doc.width.max(1) as f32),
        pos.y.clamp(0.0, photo_doc.height.max(1) as f32),
    )
}

fn draw_active_tool_picker(ui: &mut egui::Ui) {
    ui.collapsing("Активный инструмент", |ui| {
        let current = active_photo_tool();
        ui.label(format!("Текущий: {}", current.label()));
        let tools = [
            PhotoTool::Move,
            PhotoTool::Hand,
            PhotoTool::Marquee,
            PhotoTool::Crop,
            PhotoTool::Brush,
            PhotoTool::Eraser,
            PhotoTool::Dodge,
            PhotoTool::Burn,
            PhotoTool::Blur,
            PhotoTool::Sharpen,
            PhotoTool::Eyedropper,
        ];
        for chunk in tools.chunks(3) {
            ui.horizontal(|ui| {
                for tool in chunk {
                    if ui.selectable_label(current == *tool, tool.label()).clicked() {
                        set_active_photo_tool(*tool);
                    }
                }
            });
        }
    });
}

fn draw_brush_settings_panel(ui: &mut egui::Ui) {
    ui.collapsing("Кисть / локальная ретушь", |ui| {
        let mut settings = BRUSH_SETTINGS.with(|store| *store.borrow());
        let mut changed = false;
        changed |= ui.add(egui::Slider::new(&mut settings.size, 1.0..=512.0).text("Brush size px")).changed();
        changed |= ui.add(egui::Slider::new(&mut settings.opacity, 0.0..=1.0).text("Непрозрачность")).changed();
        changed |= ui.add(egui::Slider::new(&mut settings.flow, 0.01..=1.0).text("Поток")).changed();
        changed |= ui.add(egui::Slider::new(&mut settings.hardness, 0.0..=1.0).text("Жёсткость")).changed();
        changed |= ui.add(egui::Slider::new(&mut settings.spacing, 0.05..=1.0).text("Интервал")).changed();
        changed |= ui.add(egui::Slider::new(&mut settings.strength, 0.01..=1.0).text("Retouch exposure/strength")).changed();
        ui.label("Цвет кисти RGBA");
        changed |= ui.add(egui::Slider::new(&mut settings.color[0], 0.0..=1.0).text("R")).changed();
        changed |= ui.add(egui::Slider::new(&mut settings.color[1], 0.0..=1.0).text("G")).changed();
        changed |= ui.add(egui::Slider::new(&mut settings.color[2], 0.0..=1.0).text("B")).changed();
        changed |= ui.add(egui::Slider::new(&mut settings.color[3], 0.0..=1.0).text("A")).changed();
        ui.horizontal_wrapped(|ui| {
            ui.label("Диапазон:");
            for range in [RetouchToneRange::Shadows, RetouchToneRange::Midtones, RetouchToneRange::Highlights] {
                if ui.selectable_label(settings.retouch_range == range, range.label()).clicked() {
                    settings.retouch_range = range;
                    changed = true;
                }
            }
        });
        changed |= ui.checkbox(&mut settings.protect_tones, "Защищать тона").changed();
        ui.horizontal(|ui| {
            if ui.button("Чёрный").clicked() { settings.color = [0.0, 0.0, 0.0, 1.0]; changed = true; }
            if ui.button("Белый").clicked() { settings.color = [1.0, 1.0, 1.0, 1.0]; changed = true; }
            if ui.button("Красный").clicked() { settings.color = [1.0, 0.0, 0.0, 1.0]; changed = true; }
            if ui.button("Сброс").clicked() { settings = BrushSettings::default(); changed = true; }
        });
        if changed {
            BRUSH_SETTINGS.with(|store| *store.borrow_mut() = settings);
        }
    });
}


fn draw_active_tool_cursor(
    ui: &egui::Ui,
    painter: &egui::Painter,
    canvas_rect: egui::Rect,
    zoom: f32,
    tool: PhotoTool,
) {
    let Some(pointer) = ui.ctx().pointer_hover_pos() else { return; };
    if !canvas_rect.contains(pointer) { return; }
    match tool {
        PhotoTool::Brush | PhotoTool::Eraser | PhotoTool::Dodge | PhotoTool::Burn | PhotoTool::Blur | PhotoTool::Sharpen => {
            let settings = BRUSH_SETTINGS.with(|store| *store.borrow());
            let r = (settings.size * 0.5 * zoom).max(2.0);
            painter.circle_stroke(pointer, r, egui::Stroke::new(1.5, egui::Color32::WHITE));
            painter.circle_stroke(pointer, r + 1.0, egui::Stroke::new(1.0, egui::Color32::BLACK));
            if settings.hardness < 0.99 {
                painter.circle_stroke(pointer, (r * settings.hardness.clamp(0.0, 1.0)).max(1.0), egui::Stroke::new(1.0, egui::Color32::from_gray(160)));
            }
        }
        PhotoTool::Eyedropper => {
            painter.circle_stroke(pointer, 8.0, egui::Stroke::new(1.5, egui::Color32::from_rgb(0xcc, 0xc1, 0x00)));
            painter.line_segment([pointer + egui::vec2(-10.0, 0.0), pointer + egui::vec2(10.0, 0.0)], egui::Stroke::new(1.0, egui::Color32::from_rgb(0xcc, 0xc1, 0x00)));
            painter.line_segment([pointer + egui::vec2(0.0, -10.0), pointer + egui::vec2(0.0, 10.0)], egui::Stroke::new(1.0, egui::Color32::from_rgb(0xcc, 0xc1, 0x00)));
        }
        _ => {}
    }
}

fn selection_key(photo_doc: &PhotoDocument) -> String {
    // Stable key: selections must not disappear just because crop changes document dimensions.
    photo_doc.name.clone()
}

fn selection_state(photo_doc: &PhotoDocument) -> SelectionState {
    let key = selection_key(photo_doc);
    DOC_SELECTIONS.with(|store| store.borrow().get(&key).copied().unwrap_or_default())
}

fn set_selection_state(photo_doc: &PhotoDocument, state: SelectionState) {
    let key = selection_key(photo_doc);
    DOC_SELECTIONS.with(|store| {
        store.borrow_mut().insert(key, state);
    });
}

fn begin_document_selection(photo_doc: &PhotoDocument, pos: egui::Pos2) {
    let pos = clamp_doc_pos(photo_doc, pos);
    set_selection_state(photo_doc, SelectionState { active: false, dragging: true, start: pos, end: pos });
}

fn update_document_selection(photo_doc: &PhotoDocument, pos: egui::Pos2, constrain_square: bool) {
    let mut state = selection_state(photo_doc);
    let mut end = clamp_doc_pos(photo_doc, pos);
    if constrain_square {
        let delta = end - state.start;
        let side = delta.x.abs().min(delta.y.abs());
        end = egui::pos2(
            state.start.x + side * delta.x.signum(),
            state.start.y + side * delta.y.signum(),
        );
        end = clamp_doc_pos(photo_doc, end);
    }
    state.end = end;
    state.dragging = true;
    set_selection_state(photo_doc, state);
}

fn finish_document_selection(photo_doc: &mut PhotoDocument, tool: PhotoTool) {
    let mut state = selection_state(photo_doc);
    if state.dragging {
        state.dragging = false;
        state.active = state.rect().is_some();
        set_selection_state(photo_doc, state);

        // Standard behavior: Marquee only leaves a selection. Crop leaves a crop frame
        // and is applied explicitly by Enter or the Crop button, not on mouse release.
        if tool == PhotoTool::Crop && state.active {
            set_active_photo_tool(PhotoTool::Crop);
        }
    }
}

fn clear_active_selection(photo_doc: &PhotoDocument) {
    set_selection_state(photo_doc, SelectionState::default());
}

fn draw_document_selection_overlay(
    painter: &egui::Painter,
    canvas_rect: egui::Rect,
    zoom: f32,
    photo_doc: &PhotoDocument,
) {
    let Some(rect_doc) = selection_state(photo_doc).rect() else { return; };
    let rect = doc_rect_to_screen(canvas_rect, zoom, rect_doc);
    let stroke = egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 230, 90));
    painter.rect_stroke(rect, 0.0, stroke);
    painter.rect_filled(rect, 0.0, egui::Color32::from_rgba_unmultiplied(255, 230, 90, 24));
    painter.text(
        rect.left_top() + egui::vec2(6.0, 6.0),
        egui::Align2::LEFT_TOP,
        format!("selection {:.0}×{:.0}", rect_doc.width(), rect_doc.height()),
        egui::FontId::proportional(12.0),
        egui::Color32::from_rgb(255, 245, 170),
    );
}

fn crop_document_to_active_selection(photo_doc: &mut PhotoDocument) {
    push_undo_checkpoint(photo_doc, "Crop document");
    let Some(rect) = selection_state(photo_doc).rect() else { return; };
    let rect = clamp_rect_to_document(photo_doc, rect);
    if rect.width() <= 1.0 || rect.height() <= 1.0 {
        clear_active_selection(photo_doc);
        return;
    }

    // Standard, layer-safe crop: crop the canvas and move existing layers.
    // Do NOT create a flattened "whole photo" raster layer. That broke layer workflows
    // for shadows, paint overlays, text and effects.
    crop_document_to_rect(photo_doc, rect);
    clear_active_selection(photo_doc);
}

fn clamp_rect_to_document(photo_doc: &PhotoDocument, rect: egui::Rect) -> egui::Rect {
    let min = clamp_doc_pos(photo_doc, rect.min);
    let max = clamp_doc_pos(photo_doc, rect.max);
    egui::Rect::from_min_max(
        egui::pos2(min.x.min(max.x), min.y.min(max.y)),
        egui::pos2(min.x.max(max.x), min.y.max(max.y)),
    )
}

fn crop_document_to_rect(photo_doc: &mut PhotoDocument, rect: egui::Rect) {
    if rect.width() <= 1.0 || rect.height() <= 1.0 {
        return;
    }
    let shift = egui::vec2(rect.min.x, rect.min.y);
    for layer in &mut photo_doc.layers {
        layer.x -= shift.x;
        layer.y -= shift.y;
    }
    photo_doc.width = rect.width().ceil().max(1.0) as u32;
    photo_doc.height = rect.height().ceil().max(1.0) as u32;
}

fn create_cropped_image_layer_from_rect(photo_doc: &mut PhotoDocument, rect: egui::Rect) -> Option<String> {
    let rect = clamp_rect_to_document(photo_doc, rect);
    let x = rect.min.x.floor().max(0.0) as u32;
    let y = rect.min.y.floor().max(0.0) as u32;
    let w = rect.width().ceil().max(1.0) as u32;
    let h = rect.height().ceil().max(1.0) as u32;
    if w == 0 || h == 0 {
        return None;
    }

    // Create the crop layer from the visible composition, not only from the selected image.
    // This makes Crop useful even when the selection covers multiple layers or text.
    let full = render_visible_layers_to_rgba(photo_doc);
    if x >= full.width() || y >= full.height() {
        return None;
    }
    let crop_w = w.min(full.width().saturating_sub(x)).max(1);
    let crop_h = h.min(full.height().saturating_sub(y)).max(1);
    let cropped = image::imageops::crop_imm(&full, x, y, crop_w, crop_h).to_image();

    let out_dir = std::env::temp_dir().join("memstroy_photo_editor_crop_layers");
    std::fs::create_dir_all(&out_dir).ok()?;
    let safe_name = photo_doc
        .name
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect::<String>();
    let id = make_unique_layer_id(photo_doc, "crop");
    let out_path = out_dir.join(format!("{}_{}.png", safe_name, id));
    cropped.save(&out_path).ok()?;

    let new_id = id.clone();
    photo_doc.layers.push(PhotoLayer {
        id,
        name: "Кадрирование".to_string(),
        visible: true,
        opacity: 1.0,
        x: rect.min.x,
        y: rect.min.y,
        scale: 1.0,
        rotation: 0.0,
        kind: PhotoLayerKind::Image { path: out_path.display().to_string() },
    });
    photo_doc.selected_layer = Some(new_id.clone());
    IMAGE_TEXTURE_CACHE.with(|cache| cache.borrow_mut().clear());
    IMAGE_DIM_CACHE.with(|cache| cache.borrow_mut().clear());
    Some(new_id)
}

fn choose_crop_source_layer_index(photo_doc: &PhotoDocument, rect: egui::Rect) -> Option<usize> {
    if let Some(selected_id) = photo_doc.selected_layer.as_ref() {
        if let Some(index) = photo_doc.layers.iter().position(|layer| {
            layer.id == *selected_id
                && layer.visible
                && matches!(&layer.kind, PhotoLayerKind::Image { .. })
                && rects_intersect(layer_bounds_doc(layer), rect)
        }) {
            return Some(index);
        }
    }

    photo_doc.layers.iter().enumerate().rev().find_map(|(index, layer)| {
        if layer.visible && matches!(&layer.kind, PhotoLayerKind::Image { .. }) && rects_intersect(layer_bounds_doc(layer), rect) {
            Some(index)
        } else {
            None
        }
    })
}

fn rects_intersect(a: egui::Rect, b: egui::Rect) -> bool {
    a.min.x < b.max.x && a.max.x > b.min.x && a.min.y < b.max.y && a.max.y > b.min.y
}

fn rect_intersection(a: egui::Rect, b: egui::Rect) -> Option<egui::Rect> {
    let min = egui::pos2(a.min.x.max(b.min.x), a.min.y.max(b.min.y));
    let max = egui::pos2(a.max.x.min(b.max.x), a.max.y.min(b.max.y));
    if max.x > min.x && max.y > min.y {
        Some(egui::Rect::from_min_max(min, max))
    } else {
        None
    }
}

fn selection_rect_to_image_crop(
    layer: &PhotoLayer,
    selection_rect: egui::Rect,
    image_w: u32,
    image_h: u32,
) -> Option<(u32, u32, u32, u32, egui::Rect)> {
    let doc_intersection = rect_intersection(layer_bounds_doc(layer), selection_rect)?;
    let bounds = layer_bounds_doc(layer);
    let center = bounds.center();
    let inv_angle = -layer.rotation.to_radians();
    let scale = layer.scale.max(MIN_LAYER_SCALE);

    let corners = [
        doc_intersection.min,
        egui::pos2(doc_intersection.max.x, doc_intersection.min.y),
        doc_intersection.max,
        egui::pos2(doc_intersection.min.x, doc_intersection.max.y),
    ];

    let mut min_px = f32::INFINITY;
    let mut min_py = f32::INFINITY;
    let mut max_px = f32::NEG_INFINITY;
    let mut max_py = f32::NEG_INFINITY;

    for corner in corners {
        let local = corner - center;
        let unrotated = center + rotate_vec2(local, inv_angle);
        let px = (unrotated.x - layer.x) / scale;
        let py = (unrotated.y - layer.y) / scale;
        min_px = min_px.min(px);
        min_py = min_py.min(py);
        max_px = max_px.max(px);
        max_py = max_py.max(py);
    }

    let x0 = min_px.floor().clamp(0.0, image_w as f32) as u32;
    let y0 = min_py.floor().clamp(0.0, image_h as f32) as u32;
    let x1 = max_px.ceil().clamp(0.0, image_w as f32) as u32;
    let y1 = max_py.ceil().clamp(0.0, image_h as f32) as u32;
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some((x0, y0, x1 - x0, y1 - y0, doc_intersection))
}

fn selected_image_layer_mut(photo_doc: &mut PhotoDocument) -> Option<&mut PhotoLayer> {
    let selected_id = photo_doc.selected_layer.clone()?;
    photo_doc.layers.iter_mut().find(|layer| {
        layer.id == selected_id && matches!(&layer.kind, PhotoLayerKind::Image { .. })
    })
}

fn ensure_selected_image_editable_png(layer: &mut PhotoLayer) -> Option<std::path::PathBuf> {
    let layer_id = layer.id.clone();
    let PhotoLayerKind::Image { path } = &mut layer.kind else { return None; };
    let current = std::path::PathBuf::from(path.as_str());
    let edit_dir = std::env::temp_dir().join("memstroy_photo_editor_pixel_edits");
    std::fs::create_dir_all(&edit_dir).ok()?;

    // Files created by this editor are already editable PNGs. Do not copy paint/raster
    // layers into pixel_edits: that changes their path marker and disables alpha-safe
    // brush expansion, which was the main reason long brush strokes broke.
    if current.exists() && is_editor_raster_layer_path(path) {
        return Some(current);
    }
    if current.starts_with(&edit_dir) && current.exists() {
        return Some(current);
    }

    let image = image::open(&current).ok()?;
    let safe_id = layer_id.chars().map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' }).collect::<String>();
    let target = edit_dir.join(format!("{}_editable.png", safe_id));
    image.save(&target).ok()?;
    *path = target.display().to_string();
    IMAGE_TEXTURE_CACHE.with(|cache| cache.borrow_mut().clear());
    IMAGE_DIM_CACHE.with(|cache| cache.borrow_mut().clear());
    Some(target)
}

fn doc_pos_to_image_pixel(layer: &PhotoLayer, doc_pos: egui::Pos2, image_w: u32, image_h: u32) -> Option<(i32, i32)> {
    if image_w == 0 || image_h == 0 { return None; }
    let bounds = image_layer_bounds_doc_from_dims(layer, image_w, image_h);
    let center = bounds.center();
    let local = doc_pos - center;
    let unrotated = rotate_vec2(local, -layer.rotation.to_radians());
    let unrotated_doc = center + unrotated;
    let px = ((unrotated_doc.x - layer.x) / layer.scale.max(MIN_LAYER_SCALE)).round() as i32;
    let py = ((unrotated_doc.y - layer.y) / layer.scale.max(MIN_LAYER_SCALE)).round() as i32;
    if px >= 0 && py >= 0 && px < image_w as i32 && py < image_h as i32 {
        Some((px, py))
    } else {
        None
    }
}


fn selected_image_layer_id(photo_doc: &PhotoDocument) -> Option<String> {
    let selected_id = photo_doc.selected_layer.as_ref()?;
    let layer = photo_doc.layers.iter().find(|layer| &layer.id == selected_id)?;
    if matches!(&layer.kind, PhotoLayerKind::Image { .. }) {
        Some(selected_id.clone())
    } else {
        None
    }
}

fn image_layer_pixel_at_doc_pos(layer: &PhotoLayer, doc_pos: egui::Pos2) -> Option<(std::path::PathBuf, i32, i32)> {
    let PhotoLayerKind::Image { path } = &layer.kind else { return None; };
    let path_buf = std::path::PathBuf::from(path.as_str());
    let (w, h) = image::image_dimensions(&path_buf).ok()?;
    let (x, y) = doc_pos_to_image_pixel(layer, doc_pos, w, h)?;
    Some((path_buf, x, y))
}

fn image_layer_visible_pixel_at_doc_pos(layer: &PhotoLayer, doc_pos: egui::Pos2) -> bool {
    let PhotoLayerKind::Image { path } = &layer.kind else { return false; };
    let Ok(rgba) = image::open(path).map(|img| img.to_rgba8()) else { return false; };
    let (w, h) = rgba.dimensions();
    let Some((x, y)) = doc_pos_to_image_pixel(layer, doc_pos, w, h) else { return false; };
    rgba.get_pixel(x as u32, y as u32)[3] > 0
}

fn top_image_layer_id_at_doc_pos(photo_doc: &PhotoDocument, doc_pos: egui::Pos2) -> Option<String> {
    for layer in photo_doc.layers.iter().rev() {
        if !layer.visible { continue; }
        // Transparent paint-layer padding must not block retouch tools from reaching
        // the real photo layer below. Hit-test by alpha, not only by rectangle bounds.
        if matches!(&layer.kind, PhotoLayerKind::Image { .. }) && image_layer_visible_pixel_at_doc_pos(layer, doc_pos) {
            return Some(layer.id.clone());
        }
    }
    None
}

fn selected_image_layer_path(photo_doc: &PhotoDocument) -> Option<String> {
    let id = photo_doc.selected_layer.as_ref()?;
    let layer = photo_doc.layers.iter().find(|layer| &layer.id == id)?;
    let PhotoLayerKind::Image { path } = &layer.kind else { return None; };
    Some(path.clone())
}

fn is_editor_raster_layer_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    normalized.contains("memstroy_photo_editor_raster_layers")
        || normalized.contains("memstroy_photo_editor_pixel_edits")
        || normalized.contains("memstroy_photo_editor_paint_layers")
}

fn is_alpha_safe_paint_layer_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    normalized.contains("memstroy_photo_editor_raster_layers")
        || normalized.contains("memstroy_photo_editor_paint_layers")
}

fn ensure_paint_target_layer_id(photo_doc: &mut PhotoDocument, doc_pos: egui::Pos2, tool: PhotoTool) -> Option<String> {
    // Standard rule: pixel tools work on a pixel layer. To fix the broken-layer bug,
    // Brush paints to a transparent paint/raster layer only; it never flattens the whole
    // composition into a new photo-sized layer. Retouch tools (dodge/burn/blur/шакализатор)
    // edit the active image layer, or the top image under the cursor if no valid image is active.
    if tool == PhotoTool::Brush {
        if let Some(selected_id) = selected_image_layer_id(photo_doc) {
            if let Some(path) = selected_image_layer_path(photo_doc) {
                if is_alpha_safe_paint_layer_path(&path) {
                    return Some(selected_id);
                }
            }
        }
        return add_paint_layer_around_doc_pos(photo_doc, doc_pos);
    }

    if let Some(selected_id) = selected_image_layer_id(photo_doc) {
        if let Some(layer) = photo_doc.layers.iter().find(|layer| layer.id == selected_id) {
            if image_layer_pixel_at_doc_pos(layer, doc_pos).is_some() {
                return Some(selected_id);
            }
        }
    }

    if let Some(hit_id) = top_image_layer_id_at_doc_pos(photo_doc, doc_pos) {
        photo_doc.selected_layer = Some(hit_id.clone());
        return Some(hit_id);
    }

    None
}

fn add_blank_raster_layer_named(photo_doc: &mut PhotoDocument, layer_name: &str) {
    add_blank_raster_layer(photo_doc);
    if let Some(layer) = selected_layer_mut(photo_doc) {
        layer.name = layer_name.to_string();
    }
}

fn brush_radius_doc() -> f32 {
    BRUSH_SETTINGS.with(|store| store.borrow().size.max(1.0) * 0.5)
}

fn add_paint_layer_around_doc_pos(photo_doc: &mut PhotoDocument, doc_pos: egui::Pos2) -> Option<String> {
    let id = make_unique_layer_id(photo_doc, "paint");
    let edit_dir = std::env::temp_dir().join("memstroy_photo_editor_paint_layers");
    std::fs::create_dir_all(&edit_dir).ok()?;

    let radius = brush_radius_doc().ceil() as i32 + PAINT_LAYER_PADDING;
    let doc_w = photo_doc.width.max(1) as i32;
    let doc_h = photo_doc.height.max(1) as i32;
    let cx = doc_pos.x.round() as i32;
    let cy = doc_pos.y.round() as i32;
    let min_x = (cx - radius).clamp(0, doc_w.saturating_sub(1));
    let min_y = (cy - radius).clamp(0, doc_h.saturating_sub(1));
    let max_x = (cx + radius).clamp(min_x + 1, doc_w);
    let max_y = (cy + radius).clamp(min_y + 1, doc_h);
    let width = (max_x - min_x).max(1) as u32;
    let height = (max_y - min_y).max(1) as u32;

    let safe_name = photo_doc
        .name
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect::<String>();
    let target = edit_dir.join(format!("{}_{}.png", safe_name, id));
    let image = image::RgbaImage::from_pixel(width, height, image::Rgba([0, 0, 0, 0]));
    image.save(&target).ok()?;

    photo_doc.layers.push(PhotoLayer {
        id: id.clone(),
        name: "Paint Layer".to_string(),
        visible: true,
        opacity: 1.0,
        x: min_x as f32,
        y: min_y as f32,
        scale: 1.0,
        rotation: 0.0,
        kind: PhotoLayerKind::Image { path: target.display().to_string() },
    });
    photo_doc.selected_layer = Some(id.clone());
    IMAGE_TEXTURE_CACHE.with(|cache| cache.borrow_mut().clear());
    IMAGE_DIM_CACHE.with(|cache| cache.borrow_mut().clear());
    Some(id)
}

fn transparent_canvas(width: u32, height: u32) -> image::RgbaImage {
    image::RgbaImage::from_pixel(width.max(1), height.max(1), image::Rgba([0, 0, 0, 0]))
}

fn copy_rgba_into(dst: &mut image::RgbaImage, src: &image::RgbaImage, offset_x: i32, offset_y: i32) {
    let (dw, dh) = dst.dimensions();
    let (sw, sh) = src.dimensions();
    for sy in 0..sh as i32 {
        for sx in 0..sw as i32 {
            let dx = sx + offset_x;
            let dy = sy + offset_y;
            if dx < 0 || dy < 0 || dx >= dw as i32 || dy >= dh as i32 { continue; }
            let p = *src.get_pixel(sx as u32, sy as u32);
            dst.put_pixel(dx as u32, dy as u32, p);
        }
    }
}

fn ensure_alpha_paint_layer_contains_rect(layer: &mut PhotoLayer, rgba: image::RgbaImage, doc_rect: egui::Rect) -> image::RgbaImage {
    let PhotoLayerKind::Image { path } = &layer.kind else { return rgba; };
    if !is_alpha_safe_paint_layer_path(path) { return rgba; }
    if layer.rotation.abs() > 0.001 || (layer.scale - 1.0).abs() > 0.001 { return rgba; }

    let (image_w, image_h) = rgba.dimensions();
    let current = image_layer_bounds_doc_from_dims(layer, image_w, image_h);
    let union = current.union(doc_rect);
    let min_x = union.min.x.floor().max(0.0);
    let min_y = union.min.y.floor().max(0.0);
    let max_x = union.max.x.ceil().max(min_x + 1.0);
    let max_y = union.max.y.ceil().max(min_y + 1.0);
    if min_x >= current.min.x.floor()
        && min_y >= current.min.y.floor()
        && max_x <= current.max.x.ceil()
        && max_y <= current.max.y.ceil()
    {
        return rgba;
    }

    let new_w = (max_x - min_x).ceil().max(1.0) as u32;
    let new_h = (max_y - min_y).ceil().max(1.0) as u32;
    let mut expanded = transparent_canvas(new_w, new_h);
    let offset_x = (layer.x - min_x).round() as i32;
    let offset_y = (layer.y - min_y).round() as i32;
    copy_rgba_into(&mut expanded, &rgba, offset_x, offset_y);
    layer.x = min_x;
    layer.y = min_y;
    layer.scale = 1.0;
    layer.rotation = 0.0;
    expanded
}

fn brush_segment_doc_rect(from: egui::Pos2, to: egui::Pos2) -> egui::Rect {
    let settings = BRUSH_SETTINGS.with(|store| *store.borrow());
    let pad = settings.size.max(1.0) * 0.5 + PAINT_LAYER_PADDING as f32 + 2.0;
    egui::Rect::from_min_max(
        egui::pos2(from.x.min(to.x) - pad, from.y.min(to.y) - pad),
        egui::pos2(from.x.max(to.x) + pad, from.y.max(to.y) + pad),
    )
}

fn expand_alpha_paint_layer_for_segment(
    photo_doc: &mut PhotoDocument,
    target_id: &str,
    rgba: image::RgbaImage,
    from: egui::Pos2,
    to: egui::Pos2,
) -> (Option<PhotoLayer>, image::RgbaImage, bool) {
    let Some(layer) = photo_doc.layers.iter_mut().find(|layer| layer.id == target_id) else {
        return (None, rgba, false);
    };

    let before_x = layer.x;
    let before_y = layer.y;
    let before_size = rgba.dimensions();
    let expanded = ensure_alpha_paint_layer_contains_rect(layer, rgba, brush_segment_doc_rect(from, to));
    let after_size = expanded.dimensions();
    let expanded_changed = (layer.x - before_x).abs() > f32::EPSILON
        || (layer.y - before_y).abs() > f32::EPSILON
        || before_size != after_size;

    (Some(layer.clone()), expanded, expanded_changed)
}


fn alpha_bounds(image: &image::RgbaImage) -> Option<(u32, u32, u32, u32)> {
    let (w, h) = image.dimensions();
    let mut min_x = w;
    let mut min_y = h;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    let mut found = false;
    for y in 0..h {
        for x in 0..w {
            if image.get_pixel(x, y)[3] > 0 {
                found = true;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }
    if found { Some((min_x, min_y, max_x, max_y)) } else { None }
}

fn trim_alpha_paint_layer_after_stroke(photo_doc: &mut PhotoDocument, target_id: &str, padding: i32) {
    let Some(layer) = photo_doc.layers.iter_mut().find(|layer| layer.id == target_id) else { return; };
    let PhotoLayerKind::Image { path } = &mut layer.kind else { return; };
    if !is_alpha_safe_paint_layer_path(path) { return; }
    if layer.rotation.abs() > 0.001 || (layer.scale - 1.0).abs() > 0.001 { return; }
    let Ok(image) = image::open(path.as_str()).map(|img| img.to_rgba8()) else { return; };
    let Some((min_x, min_y, max_x, max_y)) = alpha_bounds(&image) else {
        let tiny = transparent_canvas(1, 1);
        if tiny.save(path.as_str()).is_ok() {
            IMAGE_DIM_CACHE.with(|cache| cache.borrow_mut().insert(normalize_path_key(path), egui::vec2(1.0, 1.0)));
        }
        return;
    };
    let (w, h) = image.dimensions();
    let pad = padding.max(0) as u32;
    let crop_x = min_x.saturating_sub(pad);
    let crop_y = min_y.saturating_sub(pad);
    let crop_max_x = (max_x + pad + 1).min(w);
    let crop_max_y = (max_y + pad + 1).min(h);
    let crop_w = crop_max_x.saturating_sub(crop_x).max(1);
    let crop_h = crop_max_y.saturating_sub(crop_y).max(1);
    if crop_x == 0 && crop_y == 0 && crop_w == w && crop_h == h { return; }
    let cropped = image::imageops::crop_imm(&image, crop_x, crop_y, crop_w, crop_h).to_image();
    if cropped.save(path.as_str()).is_ok() {
        layer.x += crop_x as f32;
        layer.y += crop_y as f32;
        IMAGE_DIM_CACHE.with(|cache| cache.borrow_mut().insert(normalize_path_key(path), egui::vec2(crop_w as f32, crop_h as f32)));
    }
}

fn prepare_paint_stroke(
    photo_doc: &mut PhotoDocument,
    doc_pos: egui::Pos2,
    tool: PhotoTool,
) -> Option<(String, std::path::PathBuf, PhotoLayer, image::RgbaImage)> {
    let target_id = ensure_paint_target_layer_id(photo_doc, doc_pos, tool)?;
    photo_doc.selected_layer = Some(target_id.clone());

    let settings = BRUSH_SETTINGS.with(|store| *store.borrow());
    let brush_pad = settings.size.max(1.0) * 0.5 + 2.0;
    let doc_rect = egui::Rect::from_min_max(
        egui::pos2(doc_pos.x - brush_pad, doc_pos.y - brush_pad),
        egui::pos2(doc_pos.x + brush_pad, doc_pos.y + brush_pad),
    );

    let (path, layer_clone, rgba) = {
        let layer = selected_image_layer_mut(photo_doc)?;
        let path = ensure_selected_image_editable_png(layer)?;
        let rgba = image::open(&path).ok()?.to_rgba8();
        let rgba = if tool == PhotoTool::Brush {
            ensure_alpha_paint_layer_contains_rect(layer, rgba, doc_rect)
        } else {
            rgba
        };
        (path, layer.clone(), rgba)
    };

    Some((target_id, path, layer_clone, rgba))
}

fn begin_brush_stroke(ctx: &egui::Context, photo_doc: &mut PhotoDocument, doc_pos: egui::Pos2, tool: PhotoTool) {
    append_photo_log("INFO", &format!("brush stroke begin: {} at {:.1},{:.1}", tool.label(), doc_pos.x, doc_pos.y));
    push_brush_undo_checkpoint(photo_doc, tool);
    let doc_pos = clamp_doc_pos(photo_doc, doc_pos);
    let Some((target_id, path, layer, mut rgba)) = prepare_paint_stroke(photo_doc, doc_pos, tool) else { return; };
    let source_image = if matches!(tool, PhotoTool::Blur | PhotoTool::Sharpen) { Some(rgba.clone()) } else { None };
    let changed = paint_line_on_rgba(&mut rgba, source_image.as_ref(), &layer, doc_pos, doc_pos, tool);
    if changed {
        update_live_stroke_texture(ctx, path.to_string_lossy().as_ref(), &rgba, &layer_runtime_fx(&target_id));
    }
    BRUSH_STROKE_STATE.with(|store| {
        *store.borrow_mut() = BrushStrokeState {
            active: true,
            tool,
            last_doc_pos: doc_pos,
            target_layer_id: Some(target_id),
            target_path: Some(path),
            working_image: Some(rgba),
            source_image,
            dirty: changed,
            paint_ops_since_upload: 0,
        };
    });
}

fn update_brush_stroke(ctx: &egui::Context, photo_doc: &mut PhotoDocument, doc_pos: egui::Pos2, tool: PhotoTool) {
    let update_start = std::time::Instant::now();
    let doc_pos = clamp_doc_pos(photo_doc, doc_pos);
    let mut restart = false;
    let mut perf_radius = 0i32;
    let mut perf_stamps = 0i32;

    BRUSH_STROKE_STATE.with(|store| {
        let mut state = store.borrow_mut();
        if !state.active || state.tool != tool {
            restart = true;
            return;
        }

        let Some(target_id) = state.target_layer_id.clone() else {
            restart = true;
            return;
        };
        let Some(base_layer) = photo_doc.layers.iter().find(|layer| layer.id == target_id).cloned() else {
            restart = true;
            return;
        };

        let fx = layer_runtime_fx(&target_id);
        let last_pos = state.last_doc_pos;
        let target_path = state.target_path.clone();
        let Some(mut image) = state.working_image.take() else {
            restart = true;
            return;
        };

        // The brush layer is intentionally small and alpha-safe. During a long drag the
        // cursor may leave the current PNG bounds; earlier versions silently dropped those
        // samples, which made drawing look completely broken. Expand the backing PNG before
        // stamping the next segment, but keep it as a small layer instead of flattening the
        // whole photo.
        let mut layer_for_paint = base_layer;
        let mut forced_upload = false;
        if tool == PhotoTool::Brush {
            let (expanded_layer, expanded_image, expanded) =
                expand_alpha_paint_layer_for_segment(photo_doc, &target_id, image, last_pos, doc_pos);
            image = expanded_image;
            if let Some(layer) = expanded_layer {
                layer_for_paint = layer;
            }
            forced_upload = expanded;
        }

        let source = state.source_image.as_ref();
        let (changed, radius_used, stamps_used) = paint_line_on_rgba_detailed(&mut image, source, &layer_for_paint, last_pos, doc_pos, tool);
        perf_radius = radius_used;
        perf_stamps = stamps_used;
        if changed || forced_upload {
            state.dirty = state.dirty || changed;
            state.paint_ops_since_upload = state.paint_ops_since_upload.saturating_add(1);

            // Do not upload the whole image to the GPU on every mouse event.
            let upload_every = match tool {
                PhotoTool::Blur | PhotoTool::Sharpen => 1_000_000,
                PhotoTool::Dodge | PhotoTool::Burn => 24,
                _ => 10,
            };
            if forced_upload || state.paint_ops_since_upload >= upload_every {
                if let Some(path) = target_path.as_ref() {
                    update_live_stroke_texture(ctx, path.to_string_lossy().as_ref(), &image, &fx);
                }
                state.paint_ops_since_upload = 0;
            }
        }

        state.last_doc_pos = doc_pos;
        state.working_image = Some(image);
    });

    log_brush_update_if_slow(tool, update_start.elapsed(), perf_radius, perf_stamps);

    if restart {
        begin_brush_stroke(ctx, photo_doc, doc_pos, tool);
    }
}

fn finish_brush_stroke(photo_doc: &mut PhotoDocument) {
    let mut saved_target: Option<String> = None;
    let mut saved_path: Option<std::path::PathBuf> = None;

    BRUSH_STROKE_STATE.with(|store| {
        let mut state = store.borrow_mut();
        if let (Some(path), Some(image)) = (&state.target_path, &state.working_image) {
            if state.dirty {
                let save_start = std::time::Instant::now();
                if image.save(path).is_ok() {
                    log_operation_duration("brush stroke PNG save", save_start.elapsed());
                    saved_target = state.target_layer_id.clone();
                    saved_path = Some(path.clone());
                } else {
                    append_photo_log("ERROR", &format!("brush stroke save failed: {}", path.display()));
                }
            }
        }
        *state = BrushStrokeState::default();
    });

    if let Some(target_id) = saved_target.as_deref() {
        trim_alpha_paint_layer_after_stroke(photo_doc, target_id, PAINT_LAYER_PADDING);
    }
    if saved_path.is_some() {
        append_photo_log("INFO", "brush stroke saved");
        IMAGE_TEXTURE_CACHE.with(|cache| cache.borrow_mut().clear());
        IMAGE_DIM_CACHE.with(|cache| cache.borrow_mut().clear());
    }
}

fn paint_line_on_rgba(
    rgba: &mut image::RgbaImage,
    source_for_filter: Option<&image::RgbaImage>,
    layer: &PhotoLayer,
    from: egui::Pos2,
    to: egui::Pos2,
    tool: PhotoTool,
) -> bool {
    paint_line_on_rgba_detailed(rgba, source_for_filter, layer, from, to, tool).0
}

fn paint_line_on_rgba_detailed(
    rgba: &mut image::RgbaImage,
    source_for_filter: Option<&image::RgbaImage>,
    layer: &PhotoLayer,
    from: egui::Pos2,
    to: egui::Pos2,
    tool: PhotoTool,
) -> (bool, i32, i32) {
    let settings = BRUSH_SETTINGS.with(|store| *store.borrow());
    let (w, h) = rgba.dimensions();
    let Some((x0, y0)) = doc_pos_to_image_pixel(layer, from, w, h) else { return (false, 0, 0); };
    let Some((x1, y1)) = doc_pos_to_image_pixel(layer, to, w, h) else { return (false, 0, 0); };
    let base_radius = (settings.size / layer.scale.max(MIN_LAYER_SCALE) * 0.5).round().max(1.0) as i32;

    // Keep the UI responsive. Blur/Шакализатор are much more expensive than normal
    // paint because every stamped pixel samples neighbours. A very large live radius
    // freezes egui; cap live radius and use wider spacing for these tools.
    let radius = match tool {
        PhotoTool::Blur | PhotoTool::Sharpen => base_radius.min(24),
        PhotoTool::Dodge | PhotoTool::Burn => base_radius.min(48),
        _ => base_radius.min(128),
    };

    let dx = x1 - x0;
    let dy = y1 - y0;
    let distance = ((dx * dx + dy * dy) as f32).sqrt();
    let spacing_factor = match tool {
        PhotoTool::Blur | PhotoTool::Sharpen => settings.spacing.max(0.65),
        PhotoTool::Dodge | PhotoTool::Burn => settings.spacing.max(0.45),
        _ => settings.spacing.clamp(0.10, 1.0),
    };
    let spacing = ((radius as f32 * 2.0) * spacing_factor).max(1.0);
    let steps = (distance / spacing).ceil().max(1.0) as i32;
    let max_steps = match tool {
        PhotoTool::Blur | PhotoTool::Sharpen => MAX_FILTER_STAMPS_PER_EVENT,
        PhotoTool::Dodge | PhotoTool::Burn => MAX_RETOUCH_STAMPS_PER_EVENT,
        _ => MAX_BRUSH_STAMPS_PER_EVENT,
    };
    let steps = steps.min(max_steps).max(1);

    match tool {
        PhotoTool::Blur => {
            let source_fallback;
            let source = if let Some(source) = source_for_filter {
                source
            } else {
                source_fallback = rgba.clone();
                &source_fallback
            };
            for i in 0..=steps {
                let t = i as f32 / steps as f32;
                let cx = (x0 as f32 + dx as f32 * t).round() as i32;
                let cy = (y0 as f32 + dy as f32 * t).round() as i32;
                blur_circle_from_source(rgba, source, cx, cy, radius, settings.strength, settings.hardness);
            }
        }
        PhotoTool::Sharpen => {
            let source_fallback;
            let source = if let Some(source) = source_for_filter {
                source
            } else {
                source_fallback = rgba.clone();
                &source_fallback
            };
            for i in 0..=steps {
                let t = i as f32 / steps as f32;
                let cx = (x0 as f32 + dx as f32 * t).round() as i32;
                let cy = (y0 as f32 + dy as f32 * t).round() as i32;
                sharpen_circle_from_source(rgba, source, cx, cy, radius, settings.strength, settings.hardness);
            }
        }
        _ => {
            for i in 0..=steps {
                let t = i as f32 / steps as f32;
                let cx = (x0 as f32 + dx as f32 * t).round() as i32;
                let cy = (y0 as f32 + dy as f32 * t).round() as i32;
                apply_tool_circle(rgba, cx, cy, radius, tool, settings);
            }
        }
    }
    (true, radius, steps)
}

fn update_live_stroke_texture(ctx: &egui::Context, path: &str, rgba: &image::RgbaImage, fx: &LayerRuntimeFx) {
    let key = format!("{}::{}", normalize_path_key(path), fx.image_cache_suffix());
    let (width, height) = rgba.dimensions();
    if width == 0 || height == 0 { return; }
    // Uploading a full 4K/8K layer to the GPU on every mouse move is the main source
    // of brush lag. Small paint layers still update live; large photo-retouch layers
    // update on stroke finish when the cache is cleared.
    if width as u64 * height as u64 > MAX_LIVE_TEXTURE_PIXELS {
        return;
    }
    let color_image = egui::ColorImage::from_rgba_unmultiplied(
        [width as usize, height as usize],
        rgba.as_raw(),
    );
    IMAGE_TEXTURE_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(cached) = cache.get_mut(&key) {
            cached.texture.set(color_image, egui::TextureOptions::LINEAR);
            cached.size = egui::vec2(width as f32, height as f32);
        } else {
            let texture = ctx.load_texture(
                format!("photo_editor_live_stroke::{}", key),
                color_image,
                egui::TextureOptions::LINEAR,
            );
            cache.insert(key, CachedTexture {
                texture,
                size: egui::vec2(width as f32, height as f32),
            });
        }
    });
    IMAGE_DIM_CACHE.with(|cache| {
        cache.borrow_mut().insert(normalize_path_key(path), egui::vec2(width as f32, height as f32));
    });
}

fn apply_tool_circle(
    rgba: &mut image::RgbaImage,
    cx: i32,
    cy: i32,
    radius: i32,
    tool: PhotoTool,
    settings: BrushSettings,
) {
    match tool {
        PhotoTool::Brush => paint_rgba_circle(rgba, cx, cy, radius, settings.color, settings.opacity, settings.flow, settings.hardness),
        PhotoTool::Eraser => erase_alpha_circle(rgba, cx, cy, radius, settings.opacity, settings.flow, settings.hardness),
        PhotoTool::Dodge => dodge_burn_circle(rgba, cx, cy, radius, settings.strength, settings.hardness, settings.retouch_range, settings.protect_tones, true),
        PhotoTool::Burn => dodge_burn_circle(rgba, cx, cy, radius, settings.strength, settings.hardness, settings.retouch_range, settings.protect_tones, false),
        PhotoTool::Blur => blur_circle(rgba, cx, cy, radius, settings.strength, settings.hardness),
        PhotoTool::Sharpen => sharpen_circle(rgba, cx, cy, radius, settings.strength, settings.hardness),
        _ => {}
    }
}

fn paint_selected_image_at_doc_pos(photo_doc: &mut PhotoDocument, doc_pos: egui::Pos2, tool: PhotoTool) {
    let settings = BRUSH_SETTINGS.with(|store| *store.borrow());
    let Some(layer) = selected_image_layer_mut(photo_doc) else { return; };
    let Some(path) = ensure_selected_image_editable_png(layer) else { return; };
    let Ok(mut rgba) = image::open(&path).map(|img| img.to_rgba8()) else { return; };
    let (w, h) = rgba.dimensions();
    let Some((cx, cy)) = doc_pos_to_image_pixel(layer, doc_pos, w, h) else { return; };
    let radius = (settings.size / layer.scale.max(MIN_LAYER_SCALE) * 0.5).round().max(1.0) as i32;

    apply_tool_circle(&mut rgba, cx, cy, radius, tool, settings);

    if rgba.save(&path).is_ok() {
        IMAGE_TEXTURE_CACHE.with(|cache| cache.borrow_mut().clear());
        IMAGE_DIM_CACHE.with(|cache| cache.borrow_mut().clear());
    }
}

fn brush_mask_weight(distance_norm: f32, hardness: f32) -> f32 {
    if distance_norm >= 1.0 {
        return 0.0;
    }
    let hardness = hardness.clamp(0.0, 1.0);
    if hardness >= 0.999 || distance_norm <= hardness {
        return 1.0;
    }
    let t = ((distance_norm - hardness) / (1.0 - hardness).max(0.001)).clamp(0.0, 1.0);
    // Smooth feather between the hard core and the edge.
    1.0 - (t * t * (3.0 - 2.0 * t))
}

fn for_each_pixel_in_circle_mut(
    image: &mut image::RgbaImage,
    cx: i32,
    cy: i32,
    radius: i32,
    hardness: f32,
    mut f: impl FnMut(&mut image::Rgba<u8>, f32),
) {
    let (w, h) = image.dimensions();
    if w == 0 || h == 0 { return; }
    let r2 = (radius * radius).max(1);
    let min_x = (cx - radius).max(0);
    let max_x = (cx + radius).min(w as i32 - 1);
    let min_y = (cy - radius).max(0);
    let max_y = (cy + radius).min(h as i32 - 1);
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let dx = x - cx;
            let dy = y - cy;
            let d2 = dx * dx + dy * dy;
            if d2 <= r2 {
                let distance_norm = (d2 as f32 / r2 as f32).sqrt();
                let weight = brush_mask_weight(distance_norm, hardness);
                if weight <= 0.001 { continue; }
                let pixel = image.get_pixel_mut(x as u32, y as u32);
                f(pixel, weight);
            }
        }
    }
}

fn paint_rgba_circle(
    image: &mut image::RgbaImage,
    cx: i32,
    cy: i32,
    radius: i32,
    color: [f32; 4],
    opacity: f32,
    flow: f32,
    hardness: f32,
) {
    for_each_pixel_in_circle_mut(image, cx, cy, radius, hardness, |pixel, mask| {
        let src_a = (opacity * flow * color[3].clamp(0.0, 1.0) * mask).clamp(0.0, 1.0);
        if src_a <= 0.001 { return; }

        // Straight-alpha source-over. Older builds wrote premultiplied-looking RGB
        // into transparent paint layers, so semi-transparent strokes rendered too dark
        // and looked broken after export/composite.
        let dst_a = pixel[3] as f32 / 255.0;
        let out_a = src_a + dst_a * (1.0 - src_a);
        let src_rgb = [color[0].clamp(0.0, 1.0), color[1].clamp(0.0, 1.0), color[2].clamp(0.0, 1.0)];
        for c in 0..3 {
            let dst_rgb = pixel[c] as f32 / 255.0;
            let out_rgb = if out_a > 0.0 {
                (src_rgb[c] * src_a + dst_rgb * dst_a * (1.0 - src_a)) / out_a
            } else {
                0.0
            };
            pixel[c] = (out_rgb * 255.0).round().clamp(0.0, 255.0) as u8;
        }
        pixel[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
    });
}

fn erase_alpha_circle(image: &mut image::RgbaImage, cx: i32, cy: i32, radius: i32, opacity: f32, flow: f32, hardness: f32) {
    for_each_pixel_in_circle_mut(image, cx, cy, radius, hardness, |pixel, mask| {
        let a = (opacity * flow * mask).clamp(0.0, 1.0);
        pixel[3] = (pixel[3] as f32 * (1.0 - a)).round().clamp(0.0, 255.0) as u8;
    });
}

fn srgb_luma(pixel: &image::Rgba<u8>) -> f32 {
    let r = pixel[0] as f32 / 255.0;
    let g = pixel[1] as f32 / 255.0;
    let b = pixel[2] as f32 / 255.0;
    (r * 0.2126 + g * 0.7152 + b * 0.0722).clamp(0.0, 1.0)
}

fn tone_range_weight(luma: f32, range: RetouchToneRange) -> f32 {
    match range {
        RetouchToneRange::Shadows => (1.0 - luma * 2.0).clamp(0.0, 1.0),
        RetouchToneRange::Midtones => (1.0 - (luma - 0.5).abs() * 2.0).clamp(0.0, 1.0),
        RetouchToneRange::Highlights => (luma * 2.0 - 1.0).clamp(0.0, 1.0),
    }
}

fn dodge_burn_circle(
    image: &mut image::RgbaImage,
    cx: i32,
    cy: i32,
    radius: i32,
    exposure: f32,
    hardness: f32,
    range: RetouchToneRange,
    protect_tones: bool,
    lighten: bool,
) {
    for_each_pixel_in_circle_mut(image, cx, cy, radius, hardness, |pixel, mask| {
        if pixel[3] == 0 { return; }
        let luma = srgb_luma(pixel);
        let range_weight = tone_range_weight(luma, range);
        let amount = (exposure * mask * range_weight).clamp(0.0, 1.0);
        if amount <= 0.001 { return; }

        if protect_tones {
            let new_luma = if lighten {
                1.0 - (1.0 - luma) * (1.0 - amount)
            } else {
                luma * (1.0 - amount)
            };
            if luma <= 0.001 {
                let v = (new_luma * 255.0).round().clamp(0.0, 255.0) as u8;
                pixel[0] = v;
                pixel[1] = v;
                pixel[2] = v;
            } else {
                let scale = (new_luma / luma).clamp(0.0, 8.0);
                for c in 0..3 {
                    let v = pixel[c] as f32 / 255.0;
                    pixel[c] = (v * scale * 255.0).round().clamp(0.0, 255.0) as u8;
                }
            }
        } else {
            for c in 0..3 {
                let v = pixel[c] as f32 / 255.0;
                let out = if lighten {
                    1.0 - (1.0 - v) * (1.0 - amount)
                } else {
                    v * (1.0 - amount)
                };
                pixel[c] = (out * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    });
}

fn blur_circle(image: &mut image::RgbaImage, cx: i32, cy: i32, radius: i32, strength: f32, hardness: f32) {
    let source = image.clone();
    blur_circle_from_source(image, &source, cx, cy, radius, strength, hardness);
}

fn blur_circle_from_source(
    image: &mut image::RgbaImage,
    source: &image::RgbaImage,
    cx: i32,
    cy: i32,
    radius: i32,
    strength: f32,
    hardness: f32,
) {
    let (w, h) = image.dimensions();
    if w == 0 || h == 0 { return; }
    let sample_r = (1.0 + strength.clamp(0.0, 1.0) * 3.0).round().clamp(1.0, 4.0) as i32;
    let min_x = (cx - radius).max(0);
    let max_x = (cx + radius).min(w as i32 - 1);
    let min_y = (cy - radius).max(0);
    let max_y = (cy + radius).min(h as i32 - 1);
    let r2 = (radius * radius).max(1);
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let dx = x - cx;
            let dy = y - cy;
            let d2 = dx * dx + dy * dy;
            if d2 > r2 { continue; }
            let mask = brush_mask_weight((d2 as f32 / r2 as f32).sqrt(), hardness);
            let blend = (strength * mask).clamp(0.0, 1.0);
            if blend <= 0.001 { continue; }

            let mut acc = [0u32; 4];
            let mut count = 0u32;
            for sy in (y - sample_r).max(0)..=(y + sample_r).min(h as i32 - 1) {
                for sx in (x - sample_r).max(0)..=(x + sample_r).min(w as i32 - 1) {
                    let p = source.get_pixel(sx as u32, sy as u32);
                    for c in 0..4 { acc[c] += p[c] as u32; }
                    count += 1;
                }
            }
            if count == 0 { continue; }
            let pixel = image.get_pixel_mut(x as u32, y as u32);
            for c in 0..4 {
                let avg = acc[c] as f32 / count as f32;
                pixel[c] = (pixel[c] as f32 * (1.0 - blend) + avg * blend).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

fn sharpen_circle(image: &mut image::RgbaImage, cx: i32, cy: i32, radius: i32, strength: f32, hardness: f32) {
    let source = image.clone();
    sharpen_circle_from_source(image, &source, cx, cy, radius, strength, hardness);
}

fn sharpen_circle_from_source(
    image: &mut image::RgbaImage,
    source: &image::RgbaImage,
    cx: i32,
    cy: i32,
    radius: i32,
    strength: f32,
    hardness: f32,
) {
    let (w, h) = image.dimensions();
    if w < 3 || h < 3 { return; }
    let min_x = (cx - radius).max(1);
    let max_x = (cx + radius).min(w as i32 - 2);
    let min_y = (cy - radius).max(1);
    let max_y = (cy + radius).min(h as i32 - 2);
    let r2 = (radius * radius).max(1);
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let dx = x - cx;
            let dy = y - cy;
            let d2 = dx * dx + dy * dy;
            if d2 > r2 { continue; }
            let mask = brush_mask_weight((d2 as f32 / r2 as f32).sqrt(), hardness);
            let amount = (strength * mask * 1.35).clamp(0.0, 1.5);
            if amount <= 0.001 { continue; }

            let center = source.get_pixel(x as u32, y as u32);
            let mut acc = [0u32; 3];
            let mut count = 0u32;
            for sy in y - 1..=y + 1 {
                for sx in x - 1..=x + 1 {
                    if sx == x && sy == y { continue; }
                    let p = source.get_pixel(sx as u32, sy as u32);
                    for c in 0..3 { acc[c] += p[c] as u32; }
                    count += 1;
                }
            }
            if count == 0 { continue; }
            let pixel = image.get_pixel_mut(x as u32, y as u32);
            for c in 0..3 {
                let avg = acc[c] as f32 / count as f32;
                let out = center[c] as f32 + (center[c] as f32 - avg) * amount;
                pixel[c] = out.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

fn pick_color_from_visible_layers(photo_doc: &mut PhotoDocument, doc_pos: egui::Pos2) {
    // Photoshop-like behavior: the eyedropper samples the visible result under the cursor.
    // Here we sample the topmost visible image/raster pixel under the cursor; text layers return their text color.
    for layer in photo_doc.layers.iter().rev() {
        if !layer.visible { continue; }
        match &layer.kind {
            PhotoLayerKind::Image { path } => {
                let Ok(rgba) = image::open(path).map(|img| img.to_rgba8()) else { continue; };
                let (w, h) = rgba.dimensions();
                let Some((x, y)) = doc_pos_to_image_pixel(layer, doc_pos, w, h) else { continue; };
                let p = rgba.get_pixel(x as u32, y as u32);
                if p[3] == 0 { continue; }
                BRUSH_SETTINGS.with(|store| {
                    let mut s = store.borrow_mut();
                    s.color = [
                        p[0] as f32 / 255.0,
                        p[1] as f32 / 255.0,
                        p[2] as f32 / 255.0,
                        p[3] as f32 / 255.0,
                    ];
                });
                photo_doc.selected_layer = Some(layer.id.clone());
                return;
            }
            PhotoLayerKind::Text { color, .. } => {
                if layer_contains_doc_pos(layer, doc_pos) {
                    BRUSH_SETTINGS.with(|store| store.borrow_mut().color = *color);
                    photo_doc.selected_layer = Some(layer.id.clone());
                    return;
                }
            }
            PhotoLayerKind::Effect { .. } => {}
        }
    }
}

fn bake_selected_image_filters_to_png(photo_doc: &mut PhotoDocument) {
    push_undo_checkpoint(photo_doc, "Bake layer effects");
    let Some(layer) = selected_image_layer_mut(photo_doc) else { return; };
    let layer_id = layer.id.clone();
    let fx = layer_runtime_fx(&layer_id);
    let PhotoLayerKind::Image { path } = &mut layer.kind else { return; };
    let Some(cached_image) = load_baked_image_rgba(path, &fx) else { return; };
    let bake_dir = std::env::temp_dir().join("memstroy_photo_editor_baked_fx");
    if std::fs::create_dir_all(&bake_dir).is_err() { return; }
    let safe_id = layer_id.chars().map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' }).collect::<String>();
    let target = bake_dir.join(format!("{}_baked.png", safe_id));
    if cached_image.save(&target).is_ok() {
        *path = target.display().to_string();
        reset_layer_runtime_fx(&layer_id);
        IMAGE_TEXTURE_CACHE.with(|cache| cache.borrow_mut().clear());
        IMAGE_DIM_CACHE.with(|cache| cache.borrow_mut().clear());
    }
}

fn load_baked_image_rgba(path: &str, fx: &LayerRuntimeFx) -> Option<image::RgbaImage> {
    let mut dyn_image = image::open(path).ok()?;
    if fx.flip_x { dyn_image = dyn_image.fliph(); }
    if fx.flip_y { dyn_image = dyn_image.flipv(); }
    if fx.grayscale { dyn_image = dyn_image.grayscale(); }
    if fx.invert { dyn_image.invert(); }
    if fx.brightness.abs() > 0.01 { dyn_image = dyn_image.brighten(fx.brightness.round().clamp(-255.0, 255.0) as i32); }
    if fx.contrast.abs() > 0.01 { dyn_image = dyn_image.adjust_contrast(fx.contrast.clamp(-100.0, 100.0)); }
    if fx.hue_rotate.abs() > 0.1 { dyn_image = dyn_image.huerotate(fx.hue_rotate.round() as i32); }
    if fx.blur > 0.01 { dyn_image = dyn_image.blur(fx.blur.clamp(0.0, 40.0)); }
    if fx.sharpen > 0.01 { dyn_image = dyn_image.unsharpen(fx.sharpen.clamp(0.0, 20.0), 1); }
    if fx.pixelate > 1.01 {
        let factor = fx.pixelate.clamp(1.0, 64.0);
        let small_w = ((dyn_image.width() as f32 / factor).round() as u32).max(1);
        let small_h = ((dyn_image.height() as f32 / factor).round() as u32).max(1);
        let rgba = dyn_image.to_rgba8();
        let small = image::imageops::resize(&rgba, small_w, small_h, image::imageops::FilterType::Nearest);
        let large = image::imageops::resize(&small, dyn_image.width(), dyn_image.height(), image::imageops::FilterType::Nearest);
        dyn_image = image::DynamicImage::ImageRgba8(large);
    }
    let mut rgba = dyn_image.to_rgba8();
    apply_runtime_color_matrix(&mut rgba, fx);
    Some(rgba)
}

fn export_visible_image_layers_to_png(photo_doc: &PhotoDocument) {
    export_visible_layers_to_png(photo_doc);
}

fn export_visible_layers_to_png(photo_doc: &PhotoDocument) {
    append_photo_log("INFO", "export dialog opened");
    let Some(target) = rfd::FileDialog::new()
        .add_filter("PNG", &["png"])
        .set_file_name(format!("{}_render.png", photo_doc.name))
        .save_file() else { return; };

    let width = photo_doc.width.max(1);
    let height = photo_doc.height.max(1);
    if width as u64 * height as u64 > MAX_EXPORT_PIXELS {
        append_photo_log("ERROR", &format!("export skipped: document is too large {}x{}", width, height));
        eprintln!("Export skipped: document is too large ({}x{}).", width, height);
        return;
    }
    let layers = photo_doc.layers.clone();
    let fx_by_layer: HashMap<String, LayerRuntimeFx> = layers
        .iter()
        .map(|layer| (layer.id.clone(), layer_runtime_fx(&layer.id)))
        .collect();

    append_photo_log("INFO", &format!("export started: {}x{} -> {}", width, height, target.display()));
    std::thread::spawn(move || {
        let start = std::time::Instant::now();
        let canvas = render_layers_to_rgba(width, height, &layers, &fx_by_layer);
        match canvas.save(&target) {
            Ok(_) => append_photo_log("INFO", &format!("PNG export finished in {} ms: {}", start.elapsed().as_millis(), target.display())),
            Err(err) => {
                append_photo_log("ERROR", &format!("PNG export failed: {}", err));
                eprintln!("PNG export failed: {err}");
            }
        }
    });
}

fn render_visible_layers_to_rgba(photo_doc: &PhotoDocument) -> image::RgbaImage {
    let fx_by_layer: HashMap<String, LayerRuntimeFx> = photo_doc
        .layers
        .iter()
        .map(|layer| (layer.id.clone(), layer_runtime_fx(&layer.id)))
        .collect();
    render_layers_to_rgba(photo_doc.width.max(1), photo_doc.height.max(1), &photo_doc.layers, &fx_by_layer)
}

fn render_layers_to_rgba(
    width: u32,
    height: u32,
    layers: &[PhotoLayer],
    fx_by_layer: &HashMap<String, LayerRuntimeFx>,
) -> image::RgbaImage {
    let render_start = std::time::Instant::now();
    let mut canvas = image::RgbaImage::from_pixel(
        width.max(1),
        height.max(1),
        image::Rgba([0, 0, 0, 0]),
    );

    for layer in layers {
        if !layer.visible { continue; }
        let fx = fx_by_layer.get(&layer.id).cloned().unwrap_or_default();
        match &layer.kind {
            PhotoLayerKind::Image { path } => {
                let Some(src) = load_baked_image_rgba(path, &fx) else { continue; };
                if fx.shadow_enabled && fx.shadow_opacity > 0.0 {
                    composite_image_layer_shadow_from_alpha(&mut canvas, &src, layer, &fx);
                }
                composite_image_layer_bilinear(&mut canvas, &src, layer);
                if fx.stroke_enabled && fx.stroke_width > 0.0 {
                    composite_image_layer_outline_from_alpha(&mut canvas, &src, layer, &fx);
                }
            }
            PhotoLayerKind::Text { text, size, color } => {
                if fx.shadow_enabled && fx.shadow_opacity > 0.0 {
                    let mut shadow_layer = layer.clone();
                    shadow_layer.x += fx.shadow_dx;
                    shadow_layer.y += fx.shadow_dy;
                    render_text_layer_to_rgba(&mut canvas, &shadow_layer, text, *size, [0.0, 0.0, 0.0, fx.shadow_opacity], &LayerRuntimeFx::default());
                }
                if fx.stroke_enabled && fx.stroke_width > 0.0 {
                    let stroke = fx.stroke_width.max(1.0);
                    let offsets = [
                        (-stroke, 0.0), (stroke, 0.0), (0.0, -stroke), (0.0, stroke),
                        (-stroke, -stroke), (stroke, -stroke), (-stroke, stroke), (stroke, stroke),
                    ];
                    for (dx, dy) in offsets {
                        let mut stroke_layer = layer.clone();
                        stroke_layer.x += dx;
                        stroke_layer.y += dy;
                        render_text_layer_to_rgba(&mut canvas, &stroke_layer, text, *size, fx.stroke_color, &LayerRuntimeFx::default());
                    }
                }
                render_text_layer_to_rgba(&mut canvas, layer, text, *size, *color, &fx);
            }
            PhotoLayerKind::Effect { .. } => {}
        }
    }

    log_operation_duration(&format!("render {}x{} with {} layer(s)", width, height, layers.len()), render_start.elapsed());
    canvas
}


fn composite_image_layer_nearest(canvas: &mut image::RgbaImage, src: &image::RgbaImage, layer: &PhotoLayer) {
    let (cw, ch) = canvas.dimensions();
    let (sw, sh) = src.dimensions();
    let bounds = rotated_layer_aabb_doc(layer);
    let min_x = bounds.min.x.floor().max(0.0) as i32;
    let max_x = bounds.max.x.ceil().min(cw as f32) as i32;
    let min_y = bounds.min.y.floor().max(0.0) as i32;
    let max_y = bounds.max.y.ceil().min(ch as f32) as i32;
    for y in min_y.max(0)..max_y.min(ch as i32) {
        for x in min_x.max(0)..max_x.min(cw as i32) {
            let doc = egui::pos2(x as f32 + 0.5, y as f32 + 0.5);
            let Some((sx, sy)) = doc_pos_to_image_pixel(layer, doc, sw, sh) else { continue; };
            let src_p = src.get_pixel(sx as u32, sy as u32);
            let a = (src_p[3] as f32 / 255.0) * layer.opacity.clamp(0.0, 1.0);
            if a <= 0.0 { continue; }
            blend_pixel(canvas, x as u32, y as u32, [src_p[0], src_p[1], src_p[2], (a * 255.0).round() as u8]);
        }
    }
}

fn composite_image_layer_bilinear(canvas: &mut image::RgbaImage, src: &image::RgbaImage, layer: &PhotoLayer) {
    let (cw, ch) = canvas.dimensions();
    let (sw, sh) = src.dimensions();
    if sw == 0 || sh == 0 { return; }
    let bounds = rotated_layer_aabb_doc(layer);
    let min_x = bounds.min.x.floor().max(0.0) as i32;
    let max_x = bounds.max.x.ceil().min(cw as f32) as i32;
    let min_y = bounds.min.y.floor().max(0.0) as i32;
    let max_y = bounds.max.y.ceil().min(ch as f32) as i32;
    for y in min_y.max(0)..max_y.min(ch as i32) {
        for x in min_x.max(0)..max_x.min(cw as i32) {
            let doc = egui::pos2(x as f32 + 0.5, y as f32 + 0.5);
            let Some((sx, sy)) = doc_pos_to_image_sample(layer, doc, sw, sh) else { continue; };
            let src_p = sample_bilinear_rgba(src, sx, sy);
            let a = (src_p[3] as f32 / 255.0) * layer.opacity.clamp(0.0, 1.0);
            if a <= 0.0 { continue; }
            blend_pixel(canvas, x as u32, y as u32, [src_p[0], src_p[1], src_p[2], (a * 255.0).round() as u8]);
        }
    }
}

fn sample_bilinear_rgba(src: &image::RgbaImage, x: f32, y: f32) -> [u8; 4] {
    let (w, h) = src.dimensions();
    let x0 = x.floor().clamp(0.0, (w - 1) as f32) as u32;
    let y0 = y.floor().clamp(0.0, (h - 1) as f32) as u32;
    let x1 = (x0 + 1).min(w - 1);
    let y1 = (y0 + 1).min(h - 1);
    let tx = (x - x0 as f32).clamp(0.0, 1.0);
    let ty = (y - y0 as f32).clamp(0.0, 1.0);
    let p00 = src.get_pixel(x0, y0);
    let p10 = src.get_pixel(x1, y0);
    let p01 = src.get_pixel(x0, y1);
    let p11 = src.get_pixel(x1, y1);
    let mut out = [0u8; 4];
    for c in 0..4 {
        let a = p00[c] as f32 * (1.0 - tx) + p10[c] as f32 * tx;
        let b = p01[c] as f32 * (1.0 - tx) + p11[c] as f32 * tx;
        out[c] = (a * (1.0 - ty) + b * ty).round().clamp(0.0, 255.0) as u8;
    }
    out
}


fn composite_image_layer_shadow_from_alpha(canvas: &mut image::RgbaImage, src: &image::RgbaImage, layer: &PhotoLayer, fx: &LayerRuntimeFx) {
    let alpha = (fx.shadow_opacity.clamp(0.0, 1.0) * layer.opacity.clamp(0.0, 1.0)).clamp(0.0, 1.0);
    if alpha <= 0.001 { return; }
    composite_image_alpha_as_color(
        canvas,
        src,
        layer,
        egui::vec2(fx.shadow_dx, fx.shadow_dy),
        [0, 0, 0, (alpha * 255.0).round().clamp(0.0, 255.0) as u8],
    );
}

fn composite_image_layer_outline_from_alpha(canvas: &mut image::RgbaImage, src: &image::RgbaImage, layer: &PhotoLayer, fx: &LayerRuntimeFx) {
    let stroke = fx.stroke_width.round().clamp(1.0, 48.0) as i32;
    let color = rgba_array_to_u8(fx.stroke_color, layer.opacity.clamp(0.0, 1.0));
    if color[3] == 0 { return; }

    // Alpha-aware outline: draw dilated alpha around the actual pixels, not around the
    // PNG rectangle. This fixes paint layers/shadows when the backing PNG is transparent.
    for dy in -stroke..=stroke {
        for dx in -stroke..=stroke {
            if dx == 0 && dy == 0 { continue; }
            if dx * dx + dy * dy > stroke * stroke { continue; }
            composite_image_alpha_as_color(canvas, src, layer, egui::vec2(dx as f32, dy as f32), color);
        }
    }
}

fn composite_image_alpha_as_color(
    canvas: &mut image::RgbaImage,
    src: &image::RgbaImage,
    layer: &PhotoLayer,
    offset: egui::Vec2,
    color: [u8; 4],
) {
    let (cw, ch) = canvas.dimensions();
    let (sw, sh) = src.dimensions();
    if cw == 0 || ch == 0 || sw == 0 || sh == 0 || color[3] == 0 { return; }

    let mut shifted = layer.clone();
    shifted.x += offset.x;
    shifted.y += offset.y;
    let bounds = rotated_layer_aabb_doc(&shifted);
    let min_x = bounds.min.x.floor().max(0.0) as i32;
    let max_x = bounds.max.x.ceil().min(cw as f32) as i32;
    let min_y = bounds.min.y.floor().max(0.0) as i32;
    let max_y = bounds.max.y.ceil().min(ch as f32) as i32;

    for y in min_y.max(0)..max_y.min(ch as i32) {
        for x in min_x.max(0)..max_x.min(cw as i32) {
            let doc = egui::pos2(x as f32 + 0.5, y as f32 + 0.5);
            let Some((sx, sy)) = doc_pos_to_image_sample(&shifted, doc, sw, sh) else { continue; };
            let src_alpha = sample_bilinear_rgba(src, sx, sy)[3] as f32 / 255.0;
            if src_alpha <= 0.001 { continue; }
            let a = (color[3] as f32 / 255.0 * src_alpha).clamp(0.0, 1.0);
            blend_pixel(canvas, x as u32, y as u32, [color[0], color[1], color[2], (a * 255.0).round() as u8]);
        }
    }
}

fn doc_pos_to_image_sample(layer: &PhotoLayer, doc: egui::Pos2, image_w: u32, image_h: u32) -> Option<(f32, f32)> {
    if image_w == 0 || image_h == 0 { return None; }
    let bounds_doc = image_layer_bounds_doc_from_dims(layer, image_w, image_h);
    let center = bounds_doc.center();
    let local = doc - center;
    let unrotated = rotate_vec2(local, -layer.rotation.to_radians());
    let unrotated_doc = center + unrotated;
    let sx = (unrotated_doc.x - layer.x) / layer.scale.max(MIN_LAYER_SCALE);
    let sy = (unrotated_doc.y - layer.y) / layer.scale.max(MIN_LAYER_SCALE);
    if sx < 0.0 || sy < 0.0 || sx > (image_w - 1) as f32 || sy > (image_h - 1) as f32 {
        None
    } else {
        Some((sx, sy))
    }
}

fn rotated_layer_aabb_doc(layer: &PhotoLayer) -> egui::Rect {
    let rect = layer_bounds_doc(layer);
    let points = rotated_rect_points_doc(rect, layer.rotation.to_radians());
    let mut min = points[0];
    let mut max = points[0];
    for p in points.iter().skip(1) {
        min.x = min.x.min(p.x);
        min.y = min.y.min(p.y);
        max.x = max.x.max(p.x);
        max.y = max.y.max(p.y);
    }
    egui::Rect::from_min_max(min, max)
}

fn rotated_rect_points_doc(rect: egui::Rect, angle: f32) -> [egui::Pos2; 4] {
    let center = rect.center();
    let corners = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()];
    let mut out = [center; 4];
    for (i, point) in corners.iter().enumerate() {
        let local = *point - center;
        out[i] = center + rotate_vec2(local, angle);
    }
    out
}

fn blend_pixel(canvas: &mut image::RgbaImage, x: u32, y: u32, src: [u8; 4]) {
    let (w, h) = canvas.dimensions();
    if x >= w || y >= h { return; }
    let a = src[3] as f32 / 255.0;
    if a <= 0.0 { return; }
    let dst = canvas.get_pixel_mut(x, y);
    let dst_a = dst[3] as f32 / 255.0;
    let out_a = a + dst_a * (1.0 - a);
    for c in 0..3 {
        let src_c = src[c] as f32 / 255.0;
        let dst_c = dst[c] as f32 / 255.0;
        let out_c = if out_a > 0.0 { (src_c * a + dst_c * dst_a * (1.0 - a)) / out_a } else { 0.0 };
        dst[c] = (out_c * 255.0).round().clamp(0.0, 255.0) as u8;
    }
    dst[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
}

fn rgba_array_to_u8(rgba: [f32; 4], opacity: f32) -> [u8; 4] {
    [
        (rgba[0].clamp(0.0, 1.0) * 255.0).round() as u8,
        (rgba[1].clamp(0.0, 1.0) * 255.0).round() as u8,
        (rgba[2].clamp(0.0, 1.0) * 255.0).round() as u8,
        (rgba[3].clamp(0.0, 1.0) * opacity.clamp(0.0, 1.0) * 255.0).round() as u8,
    ]
}

fn render_text_layer_to_rgba(
    canvas: &mut image::RgbaImage,
    layer: &PhotoLayer,
    text: &str,
    size: f32,
    color: [f32; 4],
    _fx: &LayerRuntimeFx,
) {
    // Previous builds rendered export/crop text through a 5x7 block glyph table.
    // That made small text turn into squares and made crop/export look unlike the canvas.
    // Use real TTF/OTF glyph outlines via ab_glyph and only fall back to blocks when no
    // usable system font exists.
    if let Some(font) = raster_font() {
        let effective_px = (size * layer.scale).max(6.0);
        if let Some(text_bitmap) = rasterize_text_bitmap(&font, text, effective_px, color, layer.opacity) {
            // rasterize_text_bitmap adds padding for glyph overhang/antialiasing. Offset it
            // back so rendered text starts at the same top-left as the preview text layer.
            let pad = (effective_px * 0.35).ceil().max(3.0);
            composite_text_bitmap(canvas, &text_bitmap, layer.x - pad, layer.y - pad, layer.rotation.to_radians());
            return;
        }
    }

    // Last-resort fallback. It is intentionally clamped above 1px so it remains readable
    // instead of degenerating into isolated square pixels.
    render_text_layer_block_fallback(canvas, layer, text, size.max(6.0), color);
}

fn raster_font() -> Option<ab_glyph::FontArc> {
    RASTER_FONT_CACHE.with(|cache| {
        if cache.borrow().is_none() {
            let loaded = load_raster_font();
            *cache.borrow_mut() = Some(loaded);
        }
        cache.borrow().as_ref().cloned().flatten()
    })
}

fn push_font_candidates_from_dir(candidates: &mut Vec<std::path::PathBuf>, dir: &std::path::Path, limit: usize) {
    if limit == 0 || !dir.exists() { return; }
    let mut stack = vec![dir.to_path_buf()];
    let mut added = 0usize;
    while let Some(path) = stack.pop() {
        if added >= limit { break; }
        let Ok(read_dir) = std::fs::read_dir(&path) else { continue; };
        for entry in read_dir.flatten() {
            if added >= limit { break; }
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
            if matches!(ext.as_str(), "ttf" | "otf") {
                candidates.push(p);
                added += 1;
            }
        }
    }
}

fn try_load_font_candidates(candidates: &[std::path::PathBuf]) -> Option<ab_glyph::FontArc> {
    for path in candidates {
        let Ok(bytes) = std::fs::read(path) else { continue; };
        if let Ok(font) = ab_glyph::FontArc::try_from_vec(bytes) {
            return Some(font);
        }
    }
    None
}

fn load_raster_font() -> Option<ab_glyph::FontArc> {
    // Fast path first: never recursively scan font directories before trying the
    // known system fonts. The old order could make the first PNG export feel frozen.
    let mut common: Vec<std::path::PathBuf> = Vec::new();

    if let Ok(windir) = std::env::var("WINDIR") {
        let fonts = std::path::PathBuf::from(windir).join("Fonts");
        for name in [
            "segoeui.ttf", "arial.ttf", "calibri.ttf", "tahoma.ttf", "times.ttf",
            "seguisym.ttf", "arialuni.ttf",
        ] {
            common.push(fonts.join(name));
        }
    }

    for path in [
        "C:/Windows/Fonts/segoeui.ttf",
        "C:/Windows/Fonts/arial.ttf",
        "C:/Windows/Fonts/calibri.ttf",
        "C:/Windows/Fonts/tahoma.ttf",
        "C:/Windows/Fonts/arialuni.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSansCondensed.ttf",
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/liberation2/LiberationSans-Regular.ttf",
        "/usr/share/fonts/truetype/freefont/FreeSans.ttf",
        "/Library/Fonts/Arial.ttf",
        "/Library/Fonts/Helvetica.ttf",
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "/System/Library/Fonts/Supplemental/Helvetica.ttf",
    ] {
        common.push(std::path::PathBuf::from(path));
    }

    if let Some(font) = try_load_font_candidates(&common) {
        return Some(font);
    }

    // Slow fallback: limited recursive scan only after direct paths failed.
    let mut fallback: Vec<std::path::PathBuf> = Vec::new();
    push_font_candidates_from_dir(&mut fallback, std::path::Path::new("/usr/share/fonts"), 64);
    push_font_candidates_from_dir(&mut fallback, std::path::Path::new("/Library/Fonts"), 64);
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        push_font_candidates_from_dir(&mut fallback, &std::path::PathBuf::from(local_app_data).join("Microsoft/Windows/Fonts"), 64);
    }
    try_load_font_candidates(&fallback)
}

fn rasterize_text_bitmap(
    font: &ab_glyph::FontArc,
    text: &str,
    font_size: f32,
    color: [f32; 4],
    layer_opacity: f32,
) -> Option<image::RgbaImage> {
    use ab_glyph::{point, Font, PxScale, ScaleFont};

    let text = if text.is_empty() { " " } else { text };
    let lines: Vec<&str> = text.lines().collect::<Vec<_>>();
    let lines = if lines.is_empty() { vec![" "] } else { lines };
    let scale = PxScale::from(font_size.max(6.0));
    let scaled = font.as_scaled(scale);
    let ascent = scaled.ascent();
    let descent = scaled.descent();
    let line_gap = scaled.line_gap();
    let line_height = (ascent - descent + line_gap).max(font_size * 1.20).ceil();
    let pad = (font_size * 0.35).ceil().max(3.0);

    let mut max_width = 1.0f32;
    for line in &lines {
        let mut x = 0.0f32;
        let mut prev = None;
        for ch in line.chars() {
            let glyph_id = scaled.glyph_id(ch);
            if let Some(prev_id) = prev {
                x += scaled.kern(prev_id, glyph_id);
            }
            x += scaled.h_advance(glyph_id);
            prev = Some(glyph_id);
        }
        max_width = max_width.max(x);
    }

    let width = (max_width + pad * 2.0).ceil().clamp(1.0, 100_000.0) as u32;
    let height = (line_height * lines.len() as f32 + pad * 2.0).ceil().clamp(1.0, 100_000.0) as u32;
    if width == 0 || height == 0 || width > 100_000 || height > 100_000 {
        return None;
    }

    let mut bitmap = image::RgbaImage::from_pixel(width, height, image::Rgba([0, 0, 0, 0]));
    let rgb = [
        (color[0].clamp(0.0, 1.0) * 255.0).round() as u8,
        (color[1].clamp(0.0, 1.0) * 255.0).round() as u8,
        (color[2].clamp(0.0, 1.0) * 255.0).round() as u8,
    ];
    let alpha_base = color[3].clamp(0.0, 1.0) * layer_opacity.clamp(0.0, 1.0);

    for (line_idx, line) in lines.iter().enumerate() {
        let mut x = pad;
        let baseline = pad + ascent + line_idx as f32 * line_height;
        let mut prev = None;
        for ch in line.chars() {
            let glyph_id = scaled.glyph_id(ch);
            if let Some(prev_id) = prev {
                x += scaled.kern(prev_id, glyph_id);
            }
            let glyph = glyph_id.with_scale_and_position(scale, point(x, baseline));
            if let Some(outlined) = scaled.outline_glyph(glyph) {
                outlined.draw(|gx, gy, coverage| {
                    if gx >= width || gy >= height { return; }
                    let a = (alpha_base * coverage).clamp(0.0, 1.0);
                    if a <= 0.0 { return; }
                    blend_pixel(&mut bitmap, gx, gy, [rgb[0], rgb[1], rgb[2], (a * 255.0).round() as u8]);
                });
            }
            x += scaled.h_advance(glyph_id);
            prev = Some(glyph_id);
        }
    }

    Some(bitmap)
}

fn composite_text_bitmap(
    canvas: &mut image::RgbaImage,
    text_bitmap: &image::RgbaImage,
    doc_x: f32,
    doc_y: f32,
    angle: f32,
) {
    let (tw, th) = text_bitmap.dimensions();
    if tw == 0 || th == 0 { return; }
    let pivot = egui::pos2(doc_x + tw as f32 * 0.5, doc_y + th as f32 * 0.5);
    let rect = egui::Rect::from_min_size(egui::pos2(doc_x, doc_y), egui::vec2(tw as f32, th as f32));
    let points = rotated_rect_points_doc(rect, angle);
    let (cw, ch) = canvas.dimensions();
    let min_x = points.iter().map(|p| p.x).fold(f32::INFINITY, f32::min).floor().max(0.0) as i32;
    let max_x = points.iter().map(|p| p.x).fold(f32::NEG_INFINITY, f32::max).ceil().min(cw as f32) as i32;
    let min_y = points.iter().map(|p| p.y).fold(f32::INFINITY, f32::min).floor().max(0.0) as i32;
    let max_y = points.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max).ceil().min(ch as f32) as i32;

    for y in min_y..max_y {
        for x in min_x..max_x {
            let doc = egui::pos2(x as f32 + 0.5, y as f32 + 0.5);
            let local = pivot + rotate_vec2(doc - pivot, -angle);
            let sx = local.x - doc_x;
            let sy = local.y - doc_y;
            if sx < 0.0 || sy < 0.0 || sx >= tw as f32 || sy >= th as f32 { continue; }
            let px = sx.floor() as u32;
            let py = sy.floor() as u32;
            let p = text_bitmap.get_pixel(px, py);
            if p[3] == 0 { continue; }
            blend_pixel(canvas, x as u32, y as u32, [p[0], p[1], p[2], p[3]]);
        }
    }
}

fn render_text_layer_block_fallback(canvas: &mut image::RgbaImage, layer: &PhotoLayer, text: &str, size: f32, color: [f32; 4]) {
    let cell = (size * layer.scale / 7.0).round().max(2.0);
    let line_h = (size * layer.scale * 1.15).round().max(cell * 8.0);
    let angle = layer.rotation.to_radians();
    let base_color = rgba_array_to_u8(color, layer.opacity.clamp(0.0, 1.0));
    let bounds = layer_bounds_doc(layer);
    let pivot = bounds.center();

    for (line_idx, line) in text.lines().enumerate() {
        let mut cursor_x = 0.0;
        for ch in line.chars() {
            if ch == ' ' {
                cursor_x += cell * 4.0;
                continue;
            }
            let pattern = glyph5x7(ch);
            for (row, row_bits) in pattern.iter().enumerate() {
                for (col, bit) in row_bits.chars().enumerate() {
                    if bit != '1' { continue; }
                    let local_min = egui::vec2(cursor_x + col as f32 * cell, line_idx as f32 * line_h + row as f32 * cell);
                    let local_max = local_min + egui::vec2(cell.max(1.0), cell.max(1.0));
                    fill_rotated_doc_rect(canvas, layer, pivot, angle, local_min, local_max, base_color);
                }
            }
            cursor_x += cell * 6.0;
        }
    }
}

fn fill_rotated_doc_rect(
    canvas: &mut image::RgbaImage,
    layer: &PhotoLayer,
    pivot: egui::Pos2,
    angle: f32,
    local_min: egui::Vec2,
    local_max: egui::Vec2,
    color: [u8; 4],
) {
    let p0 = egui::pos2(layer.x + local_min.x, layer.y + local_min.y);
    let p1 = egui::pos2(layer.x + local_max.x, layer.y + local_min.y);
    let p2 = egui::pos2(layer.x + local_max.x, layer.y + local_max.y);
    let p3 = egui::pos2(layer.x + local_min.x, layer.y + local_max.y);
    let pts = [
        pivot + rotate_vec2(p0 - pivot, angle),
        pivot + rotate_vec2(p1 - pivot, angle),
        pivot + rotate_vec2(p2 - pivot, angle),
        pivot + rotate_vec2(p3 - pivot, angle),
    ];
    let min_x = pts.iter().map(|p| p.x).fold(f32::INFINITY, f32::min).floor().max(0.0) as i32;
    let max_x = pts.iter().map(|p| p.x).fold(f32::NEG_INFINITY, f32::max).ceil().min(canvas.width() as f32) as i32;
    let min_y = pts.iter().map(|p| p.y).fold(f32::INFINITY, f32::min).floor().max(0.0) as i32;
    let max_y = pts.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max).ceil().min(canvas.height() as f32) as i32;
    for y in min_y..max_y {
        for x in min_x..max_x {
            let doc = egui::pos2(x as f32 + 0.5, y as f32 + 0.5);
            let local = rotate_vec2(doc - pivot, -angle);
            let unrot = pivot + local;
            if unrot.x >= p0.x && unrot.x <= p2.x && unrot.y >= p0.y && unrot.y <= p2.y {
                blend_pixel(canvas, x as u32, y as u32, color);
            }
        }
    }
}

fn glyph5x7(ch: char) -> [&'static str; 7] {
    match ch.to_ascii_uppercase() {
        'A' => ["01110","10001","10001","11111","10001","10001","10001"],
        'B' => ["11110","10001","10001","11110","10001","10001","11110"],
        'C' => ["01111","10000","10000","10000","10000","10000","01111"],
        'D' => ["11110","10001","10001","10001","10001","10001","11110"],
        'E' => ["11111","10000","10000","11110","10000","10000","11111"],
        'F' => ["11111","10000","10000","11110","10000","10000","10000"],
        'G' => ["01111","10000","10000","10111","10001","10001","01111"],
        'H' => ["10001","10001","10001","11111","10001","10001","10001"],
        'I' => ["11111","00100","00100","00100","00100","00100","11111"],
        'J' => ["00111","00010","00010","00010","10010","10010","01100"],
        'K' => ["10001","10010","10100","11000","10100","10010","10001"],
        'L' => ["10000","10000","10000","10000","10000","10000","11111"],
        'M' => ["10001","11011","10101","10101","10001","10001","10001"],
        'N' => ["10001","11001","10101","10011","10001","10001","10001"],
        'O' => ["01110","10001","10001","10001","10001","10001","01110"],
        'P' => ["11110","10001","10001","11110","10000","10000","10000"],
        'Q' => ["01110","10001","10001","10001","10101","10010","01101"],
        'R' => ["11110","10001","10001","11110","10100","10010","10001"],
        'S' => ["01111","10000","10000","01110","00001","00001","11110"],
        'T' => ["11111","00100","00100","00100","00100","00100","00100"],
        'U' => ["10001","10001","10001","10001","10001","10001","01110"],
        'V' => ["10001","10001","10001","10001","10001","01010","00100"],
        'W' => ["10001","10001","10001","10101","10101","10101","01010"],
        'X' => ["10001","10001","01010","00100","01010","10001","10001"],
        'Y' => ["10001","10001","01010","00100","00100","00100","00100"],
        'Z' => ["11111","00001","00010","00100","01000","10000","11111"],
        '0' => ["01110","10001","10011","10101","11001","10001","01110"],
        '1' => ["00100","01100","00100","00100","00100","00100","01110"],
        '2' => ["01110","10001","00001","00010","00100","01000","11111"],
        '3' => ["11110","00001","00001","01110","00001","00001","11110"],
        '4' => ["00010","00110","01010","10010","11111","00010","00010"],
        '5' => ["11111","10000","10000","11110","00001","00001","11110"],
        '6' => ["01110","10000","10000","11110","10001","10001","01110"],
        '7' => ["11111","00001","00010","00100","01000","01000","01000"],
        '8' => ["01110","10001","10001","01110","10001","10001","01110"],
        '9' => ["01110","10001","10001","01111","00001","00001","01110"],
        '-' => ["00000","00000","00000","11111","00000","00000","00000"],
        '_' => ["00000","00000","00000","00000","00000","00000","11111"],
        '.' => ["00000","00000","00000","00000","00000","01100","01100"],
        ',' => ["00000","00000","00000","00000","00000","01100","01000"],
        ':' => ["00000","01100","01100","00000","01100","01100","00000"],
        '!' => ["00100","00100","00100","00100","00100","00000","00100"],
        '?' => ["01110","10001","00001","00010","00100","00000","00100"],
        '/' => ["00001","00010","00010","00100","01000","01000","10000"],
        '+' => ["00000","00100","00100","11111","00100","00100","00000"],
        _ => ["11111","10001","00110","00110","00110","10001","11111"],
    }
}

fn trim_document_to_visible_layers(photo_doc: &mut PhotoDocument) {
    push_undo_checkpoint(photo_doc, "Trim document");
    let mut bounds: Option<egui::Rect> = None;
    for layer in &photo_doc.layers {
        if !layer.visible {
            continue;
        }
        let rect = layer_bounds_doc(layer);
        bounds = Some(if let Some(acc) = bounds { acc.union(rect) } else { rect });
    }

    let Some(bounds) = bounds else { return; };
    if bounds.width() <= 1.0 || bounds.height() <= 1.0 {
        return;
    }

    let shift = egui::vec2(bounds.min.x, bounds.min.y);
    for layer in &mut photo_doc.layers {
        layer.x -= shift.x;
        layer.y -= shift.y;
    }
    photo_doc.width = bounds.width().ceil().max(1.0) as u32;
    photo_doc.height = bounds.height().ceil().max(1.0) as u32;
}

fn crop_document_to_selected_layer(photo_doc: &mut PhotoDocument) {
    push_undo_checkpoint(photo_doc, "Crop document to layer");
    let Some(layer) = selected_layer(photo_doc).cloned() else { return; };
    crop_document_to_rect(photo_doc, layer_bounds_doc(&layer));
}

fn rotate_canvas_90(photo_doc: &mut PhotoDocument, clockwise: bool) {
    push_undo_checkpoint(photo_doc, "Rotate canvas");
    let old_w = photo_doc.width.max(1) as f32;
    let old_h = photo_doc.height.max(1) as f32;

    for layer in &mut photo_doc.layers {
        let size = layer_size_doc(layer);
        if clockwise {
            let new_x = old_h - layer.y - size.y;
            let new_y = layer.x;
            layer.x = new_x;
            layer.y = new_y;
            layer.rotation += 90.0;
        } else {
            let new_x = layer.y;
            let new_y = old_w - layer.x - size.x;
            layer.x = new_x;
            layer.y = new_y;
            layer.rotation -= 90.0;
        }
        normalize_layer_transform(layer);
    }

    photo_doc.width = old_h.round().max(1.0) as u32;
    photo_doc.height = old_w.round().max(1.0) as u32;
}

fn flip_canvas(photo_doc: &mut PhotoDocument, axis: Axis) {
    push_undo_checkpoint(photo_doc, "Flip canvas");
    let doc_w = photo_doc.width.max(1) as f32;
    let doc_h = photo_doc.height.max(1) as f32;
    let mut ids_to_flip: Vec<String> = Vec::new();

    for layer in &mut photo_doc.layers {
        let size = layer_size_doc(layer);
        match axis {
            Axis::X => {
                layer.x = doc_w - layer.x - size.x;
                ids_to_flip.push(layer.id.clone());
            }
            Axis::Y => {
                layer.y = doc_h - layer.y - size.y;
                ids_to_flip.push(layer.id.clone());
            }
        }
    }

    for id in ids_to_flip {
        mutate_layer_runtime_fx(&id, |fx| match axis {
            Axis::X => fx.flip_x = !fx.flip_x,
            Axis::Y => fx.flip_y = !fx.flip_y,
        });
    }
}

fn clear_document_selection(photo_doc: &PhotoDocument) {
    DOC_SELECTIONS.with(|store| {
        store.borrow_mut().remove(&photo_doc.name);
    });
}


fn handle_keyboard_shortcuts(ctx: &egui::Context, photo_doc: &mut PhotoDocument) {
    // Photoshop-like keyboard behavior. Do not steal text while a text field is being edited.
    if ctx.wants_keyboard_input() {
        return;
    }

    let mut choose_tool: Option<PhotoTool> = None;
    let mut undo = false;
    let mut redo = false;
    let mut copy_layer = false;
    let mut cut_layer = false;
    let mut paste_layer = false;
    let mut select_all = false;
    let mut merge_down = false;
    let mut flatten_visible = false;
    let mut delete = false;
    let mut duplicate_layer = false;
    let mut deselect = false;
    let mut new_text = false;
    let mut new_image = false;
    let mut reset_transform = false;
    let mut scale_up = false;
    let mut scale_down = false;
    let mut rotate_left = false;
    let mut rotate_right = false;
    let mut apply_crop = false;
    let mut cancel_selection = false;
    let mut nudge = egui::Vec2::ZERO;
    let mut shift_down = false;
    let mut brush_size_factor = 1.0_f32;
    let mut brush_hardness_delta = 0.0_f32;
    let mut brush_opacity: Option<f32> = None;
    let mut reset_brush_colors = false;
    let mut invert_image = false;
    let mut desaturate_image = false;
    let mut solid_fill = false;
    let mut gradient_fill = false;

    ctx.input(|input| {
        let command = input.modifiers.command || input.modifiers.ctrl;
        shift_down = input.modifiers.shift;

        // Photoshop-like tool shortcuts.
        if !command && input.key_pressed(egui::Key::V) { choose_tool = Some(PhotoTool::Move); }
        if !command && input.key_pressed(egui::Key::H) { choose_tool = Some(PhotoTool::Hand); }
        if !command && input.key_pressed(egui::Key::M) { choose_tool = Some(PhotoTool::Marquee); }
        if !command && input.key_pressed(egui::Key::C) { choose_tool = Some(PhotoTool::Crop); }
        if !command && input.key_pressed(egui::Key::B) { choose_tool = Some(PhotoTool::Brush); }
        if !command && input.key_pressed(egui::Key::E) { choose_tool = Some(PhotoTool::Eraser); }
        if !command && input.key_pressed(egui::Key::I) { choose_tool = Some(PhotoTool::Eyedropper); }
        if !command && input.key_pressed(egui::Key::O) {
            choose_tool = Some(if shift_down {
                match active_photo_tool() {
                    PhotoTool::Dodge => PhotoTool::Burn,
                    PhotoTool::Burn => PhotoTool::Dodge,
                    _ => PhotoTool::Burn,
                }
            } else {
                PhotoTool::Dodge
            });
        }
        if !command && input.key_pressed(egui::Key::D) { reset_brush_colors = true; }

        if !command && input.key_pressed(egui::Key::Enter) { apply_crop = true; }
        if !command && input.key_pressed(egui::Key::Escape) { cancel_selection = true; }

        if !command {
            for event in &input.events {
                if let egui::Event::Text(text) = event {
                    match text.as_str() {
                        "[" if shift_down => brush_hardness_delta -= 0.10,
                        "]" if shift_down => brush_hardness_delta += 0.10,
                        "[" => brush_size_factor *= 1.0 / 1.1,
                        "]" => brush_size_factor *= 1.1,
                        "0" if is_paint_tool(active_photo_tool()) => brush_opacity = Some(1.0),
                        "1" if is_paint_tool(active_photo_tool()) => brush_opacity = Some(0.1),
                        "2" if is_paint_tool(active_photo_tool()) => brush_opacity = Some(0.2),
                        "3" if is_paint_tool(active_photo_tool()) => brush_opacity = Some(0.3),
                        "4" if is_paint_tool(active_photo_tool()) => brush_opacity = Some(0.4),
                        "5" if is_paint_tool(active_photo_tool()) => brush_opacity = Some(0.5),
                        "6" if is_paint_tool(active_photo_tool()) => brush_opacity = Some(0.6),
                        "7" if is_paint_tool(active_photo_tool()) => brush_opacity = Some(0.7),
                        "8" if is_paint_tool(active_photo_tool()) => brush_opacity = Some(0.8),
                        "9" if is_paint_tool(active_photo_tool()) => brush_opacity = Some(0.9),
                        _ => {}
                    }
                }
            }
        }

        delete = input.key_pressed(egui::Key::Delete) || input.key_pressed(egui::Key::Backspace);

        // Photoshop-like edit/layer and selection shortcuts.
        undo = command && !shift_down && input.key_pressed(egui::Key::Z);
        redo = command && shift_down && input.key_pressed(egui::Key::Z);
        copy_layer = command && input.key_pressed(egui::Key::C);
        cut_layer = command && input.key_pressed(egui::Key::X);
        paste_layer = command && input.key_pressed(egui::Key::V);
        select_all = command && input.key_pressed(egui::Key::A);
        deselect = command && input.key_pressed(egui::Key::D);
        duplicate_layer = command && input.key_pressed(egui::Key::J);
        merge_down = command && !shift_down && input.key_pressed(egui::Key::E);
        flatten_visible = command && shift_down && input.key_pressed(egui::Key::E);
        reset_transform = command && input.key_pressed(egui::Key::T);
        invert_image = command && input.key_pressed(egui::Key::I);
        desaturate_image = command && shift_down && input.key_pressed(egui::Key::U);
        gradient_fill = command && input.modifiers.alt && input.key_pressed(egui::Key::G);

        // App-specific creation shortcuts kept explicit.
        new_text = command && shift_down && input.key_pressed(egui::Key::T);
        new_image = command && input.modifiers.alt && input.key_pressed(egui::Key::I);

        if input.key_pressed(egui::Key::ArrowUp) { nudge.y -= 1.0; }
        if input.key_pressed(egui::Key::ArrowDown) { nudge.y += 1.0; }
        if input.key_pressed(egui::Key::ArrowLeft) { nudge.x -= 1.0; }
        if input.key_pressed(egui::Key::ArrowRight) { nudge.x += 1.0; }

        scale_up = command && input.key_pressed(egui::Key::ArrowUp);
        scale_down = command && input.key_pressed(egui::Key::ArrowDown);
        rotate_left = command && input.key_pressed(egui::Key::ArrowLeft);
        rotate_right = command && input.key_pressed(egui::Key::ArrowRight);
    });

    if let Some(tool) = choose_tool {
        set_active_photo_tool(tool);
    }
    if undo {
        undo_photo_edit(photo_doc);
        return;
    }
    if redo {
        redo_photo_edit(photo_doc);
        return;
    }
    if copy_layer {
        copy_selected_layer_to_clipboard(photo_doc);
    }
    if cut_layer {
        cut_selected_layer_to_clipboard(photo_doc);
    }
    if paste_layer {
        paste_layer_from_clipboard(photo_doc);
    }
    if select_all {
        select_all_document(photo_doc);
    }
    if invert_image {
        invert_selected_image(photo_doc);
    }
    if desaturate_image {
        desaturate_selected_image(photo_doc);
    }
    if solid_fill {
        add_solid_fill_layer(photo_doc);
    }
    if gradient_fill {
        add_foreground_to_transparent_gradient_layer(photo_doc);
    }
    if merge_down {
        merge_selected_layer_down(photo_doc);
    }
    if flatten_visible {
        flatten_visible_layers(photo_doc);
    }
    if reset_brush_colors || brush_opacity.is_some() || (brush_size_factor - 1.0).abs() > f32::EPSILON || brush_hardness_delta.abs() > f32::EPSILON {
        BRUSH_SETTINGS.with(|store| {
            let mut settings = store.borrow_mut();
            if reset_brush_colors {
                settings.color = [0.0, 0.0, 0.0, 1.0];
            }
            if let Some(opacity) = brush_opacity {
                settings.opacity = opacity.clamp(0.0, 1.0);
            }
            if (brush_size_factor - 1.0).abs() > f32::EPSILON {
                settings.size = (settings.size * brush_size_factor).clamp(1.0, 512.0);
            }
            if brush_hardness_delta.abs() > f32::EPSILON {
                settings.hardness = (settings.hardness + brush_hardness_delta).clamp(0.0, 1.0);
            }
        });
    }
    if cancel_selection || deselect {
        push_undo_checkpoint(photo_doc, "Deselect");
        clear_document_selection(photo_doc);
    }
    if apply_crop && active_photo_tool() == PhotoTool::Crop {
        crop_document_to_active_selection(photo_doc);
    }
    if delete {
        delete_selected_layer(photo_doc);
    }
    if duplicate_layer {
        duplicate_selected_layer(photo_doc);
    }
    if reset_transform {
        set_active_photo_tool(PhotoTool::Move);
    }
    if new_text {
        add_text_layer(photo_doc);
    }
    if new_image {
        add_image_layer(photo_doc);
    }

    if scale_up { scale_selected_layer(photo_doc, 1.05); }
    if scale_down { scale_selected_layer(photo_doc, 0.95); }
    if rotate_left { rotate_selected_layer(photo_doc, -5.0); }
    if rotate_right { rotate_selected_layer(photo_doc, 5.0); }

    if nudge != egui::Vec2::ZERO && !scale_up && !scale_down && !rotate_left && !rotate_right {
        let step = if shift_down { 10.0 } else { 1.0 };
        nudge_selected_layer(photo_doc, nudge.x * step, nudge.y * step);
    }
}

fn nudge_selected_layer(photo_doc: &mut PhotoDocument, dx: f32, dy: f32) {
    push_undo_checkpoint(photo_doc, "Nudge layer");
    if let Some(layer) = selected_layer_mut(photo_doc) {
        layer.x += dx;
        layer.y += dy;
    }
}

fn rotate_selected_layer(photo_doc: &mut PhotoDocument, degrees: f32) {
    push_undo_checkpoint(photo_doc, "Rotate layer");
    if let Some(layer) = selected_layer_mut(photo_doc) {
        layer.rotation += degrees;
        normalize_layer_transform(layer);
    }
}

fn scale_selected_layer(photo_doc: &mut PhotoDocument, factor: f32) {
    push_undo_checkpoint(photo_doc, "Scale layer");
    if let Some(layer) = selected_layer_mut(photo_doc) {
        layer.scale = (layer.scale * factor).clamp(MIN_LAYER_SCALE, MAX_LAYER_SCALE);
    }
}

fn reset_selected_transform(photo_doc: &mut PhotoDocument) {
    push_undo_checkpoint(photo_doc, "Reset transform");
    if let Some(layer) = selected_layer_mut(photo_doc) {
        layer.scale = 1.0;
        layer.rotation = 0.0;
    }
}

fn center_selected_layer(photo_doc: &mut PhotoDocument) {
    push_undo_checkpoint(photo_doc, "Center layer");
    let doc_w = photo_doc.width.max(1) as f32;
    let doc_h = photo_doc.height.max(1) as f32;

    if let Some(layer) = selected_layer_mut(photo_doc) {
        let size = layer_size_doc(layer);
        layer.x = (doc_w - size.x) * 0.5;
        layer.y = (doc_h - size.y) * 0.5;
    }
}

fn fit_selected_layer_to_canvas(photo_doc: &mut PhotoDocument) {
    push_undo_checkpoint(photo_doc, "Fit layer to canvas");
    let doc_w = photo_doc.width.max(1) as f32;
    let doc_h = photo_doc.height.max(1) as f32;

    if let Some(layer) = selected_layer_mut(photo_doc) {
        let current_size = layer_size_doc(layer);
        let base_size = (current_size / layer.scale.max(MIN_LAYER_SCALE)).max(egui::vec2(1.0, 1.0));
        let fit = (doc_w / base_size.x.max(1.0)).min(doc_h / base_size.y.max(1.0));
        layer.scale = fit.clamp(MIN_LAYER_SCALE, MAX_LAYER_SCALE);
        layer.x = 0.0;
        layer.y = 0.0;
    }
}

#[derive(Clone, Copy)]
enum AlignTarget {
    Left,
    CenterX,
    Right,
    Top,
    CenterY,
    Bottom,
}

fn align_selected_layer(photo_doc: &mut PhotoDocument, target: AlignTarget) {
    push_undo_checkpoint(photo_doc, "Align layer");
    let doc_w = photo_doc.width.max(1) as f32;
    let doc_h = photo_doc.height.max(1) as f32;

    if let Some(layer) = selected_layer_mut(photo_doc) {
        let size = layer_size_doc(layer);
        match target {
            AlignTarget::Left => layer.x = 0.0,
            AlignTarget::CenterX => layer.x = (doc_w - size.x) * 0.5,
            AlignTarget::Right => layer.x = doc_w - size.x,
            AlignTarget::Top => layer.y = 0.0,
            AlignTarget::CenterY => layer.y = (doc_h - size.y) * 0.5,
            AlignTarget::Bottom => layer.y = doc_h - size.y,
        }
    }
}

#[derive(Clone, Copy)]
enum Axis {
    X,
    Y,
}

fn distribute_layers(photo_doc: &mut PhotoDocument, axis: Axis) {
    push_undo_checkpoint(photo_doc, "Distribute layers");
    if photo_doc.layers.len() < 3 {
        return;
    }

    let mut indices: Vec<usize> = (0..photo_doc.layers.len()).collect();
    match axis {
        Axis::X => indices.sort_by(|a, b| photo_doc.layers[*a].x.total_cmp(&photo_doc.layers[*b].x)),
        Axis::Y => indices.sort_by(|a, b| photo_doc.layers[*a].y.total_cmp(&photo_doc.layers[*b].y)),
    }

    let first = *indices.first().unwrap();
    let last = *indices.last().unwrap();
    let start = match axis {
        Axis::X => photo_doc.layers[first].x,
        Axis::Y => photo_doc.layers[first].y,
    };
    let end = match axis {
        Axis::X => photo_doc.layers[last].x,
        Axis::Y => photo_doc.layers[last].y,
    };

    let count = indices.len();
    if count <= 2 {
        return;
    }
    let step = (end - start) / (count.saturating_sub(1) as f32);

    for (rank, index) in indices.into_iter().enumerate() {
        let value = start + step * rank as f32;
        match axis {
            Axis::X => photo_doc.layers[index].x = value,
            Axis::Y => photo_doc.layers[index].y = value,
        }
    }
}

fn set_all_layers_visibility(photo_doc: &mut PhotoDocument, visible: bool) {
    push_undo_checkpoint(photo_doc, "Set layer visibility");
    for layer in &mut photo_doc.layers {
        layer.visible = visible;
    }
}

fn normalize_layer_transform(layer: &mut PhotoLayer) {
    layer.opacity = layer.opacity.clamp(0.0, 1.0);
    layer.scale = layer.scale.clamp(MIN_LAYER_SCALE, MAX_LAYER_SCALE);

    while layer.rotation > 360.0 {
        layer.rotation -= 360.0;
    }
    while layer.rotation < -360.0 {
        layer.rotation += 360.0;
    }
}

fn make_unique_layer_id(photo_doc: &PhotoDocument, prefix: &str) -> String {
    let mut n = 1;

    loop {
        let id = format!("{}_{}", prefix, n);
        if !photo_doc.layers.iter().any(|layer| layer.id == id) {
            return id;
        }
        n += 1;
    }
}
