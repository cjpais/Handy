use crate::commands;
use crate::commands::audio::AudioDevice;
use crate::managers::history::{HistoryEntry, HistoryManager};
use crate::managers::model::{ModelInfo, ModelManager};
use crate::managers::transcription::TranscriptionManager;
use crate::settings::{
    self, AppSettings, AutoSubmitKey, ClipboardHandling, KeyboardImplementation, LogLevel,
    ModelUnloadTimeout, OverlayPosition, PasteMethod, SoundTheme, TypingTool,
};
use eframe::egui;
use egui::Color32;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{Listener, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

// ═══════════════════════════════════════════════════════════════
// TERMINAL HACKER THEME
// ═══════════════════════════════════════════════════════════════

const BG_DARK: Color32 = Color32::from_rgb(0x0d, 0x11, 0x17);
const BG_SIDEBAR: Color32 = Color32::from_rgb(0x08, 0x0c, 0x10);
const BG_CARD: Color32 = Color32::from_rgb(0x12, 0x1a, 0x2b);
const BG_CARD_HOVER: Color32 = Color32::from_rgb(0x18, 0x22, 0x38);
const BG_INPUT: Color32 = Color32::from_rgb(0x0a, 0x0f, 0x1a);
const ACCENT_GREEN: Color32 = Color32::from_rgb(0x00, 0xff, 0x88);
const ACCENT_CYAN: Color32 = Color32::from_rgb(0x00, 0xd2, 0xff);
const ACCENT_YELLOW: Color32 = Color32::from_rgb(0xff, 0xd9, 0x3d);
const ACCENT_RED: Color32 = Color32::from_rgb(0xff, 0x44, 0x44);
const TEXT_PRIMARY: Color32 = Color32::from_rgb(0xe0, 0xe0, 0xe0);
const TEXT_DIM: Color32 = Color32::from_rgb(0x55, 0x66, 0x77);
const BORDER_SUBTLE: Color32 = Color32::from_rgb(0x1a, 0x2a, 0x3a);
const NAV_ACTIVE_BG: Color32 = Color32::from_rgb(0x0f, 0x1e, 0x2e);

fn apply_hacker_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    let mut visuals = egui::Visuals::dark();

    style.override_font_id = Some(egui::FontId::new(13.0, egui::FontFamily::Monospace));

    visuals.panel_fill = BG_DARK;
    visuals.window_fill = BG_DARK;
    visuals.faint_bg_color = BG_CARD;
    visuals.extreme_bg_color = BG_INPUT;
    visuals.override_text_color = Some(TEXT_PRIMARY);

    visuals.widgets.noninteractive.bg_fill = BG_CARD;
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, TEXT_DIM);
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(0.5, BORDER_SUBTLE);

    visuals.widgets.inactive.bg_fill = BG_INPUT;
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, TEXT_PRIMARY);
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(0.5, BORDER_SUBTLE);

    visuals.widgets.hovered.bg_fill = BG_CARD_HOVER;
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, ACCENT_GREEN);
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, ACCENT_GREEN);

    visuals.widgets.active.bg_fill = Color32::from_rgb(0x0a, 0x2a, 0x2a);
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.5, ACCENT_GREEN);
    visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, ACCENT_GREEN);

    visuals.widgets.open.bg_fill = BG_CARD;
    visuals.widgets.open.fg_stroke = egui::Stroke::new(1.0, ACCENT_CYAN);

    visuals.selection.bg_fill = Color32::from_rgba_premultiplied(0x00, 0xff, 0x88, 0x20);
    visuals.selection.stroke = egui::Stroke::new(1.0, ACCENT_GREEN);

    visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(4);
    visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(4);
    visuals.widgets.active.corner_radius = egui::CornerRadius::same(4);

    style.visuals = visuals;
    style.spacing.item_spacing = egui::vec2(8.0, 4.0);
    style.spacing.button_padding = egui::vec2(10.0, 3.0);

    ctx.set_style(style);
}

// ═══════════════════════════════════════════════════════════════
// UI HELPERS
// ═══════════════════════════════════════════════════════════════

fn section(ui: &mut egui::Ui, title: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(4.0);
    let w = ui.available_width();
    egui::Frame::new()
        .fill(BG_CARD)
        .stroke(egui::Stroke::new(0.5, BORDER_SUBTLE))
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_min_width(w - 22.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("\u{2500}\u{2500}").color(BORDER_SUBTLE).monospace().size(10.0));
                ui.label(
                    egui::RichText::new(title)
                        .color(ACCENT_CYAN)
                        .strong()
                        .monospace()
                        .size(11.0),
                );
                let remaining = ui.available_width();
                let dashes = ((remaining / 7.0) as usize).min(40);
                ui.label(egui::RichText::new("\u{2500}".repeat(dashes)).color(BORDER_SUBTLE).monospace().size(10.0));
            });
            ui.add_space(4.0);
            add_contents(ui);
        });
}

fn terminal_bar(ui: &mut egui::Ui, label: &str, value: f32, width: usize) {
    let filled = (value * width as f32).round() as usize;
    let empty = width.saturating_sub(filled);
    let bar_color = if value > 0.7 {
        ACCENT_GREEN
    } else if value > 0.4 {
        ACCENT_YELLOW
    } else {
        ACCENT_RED
    };
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("{:<8}", label))
                .color(TEXT_DIM)
                .monospace()
                .size(10.0),
        );
        // Paint the bar as a colored rect + text overlay
        let bar_text = format!("{}{}", "\u{2588}".repeat(filled), "\u{2591}".repeat(empty));
        ui.label(
            egui::RichText::new(bar_text)
                .color(bar_color)
                .monospace()
                .size(10.0),
        );
        ui.label(
            egui::RichText::new(format!("{:.0}%", value * 100.0))
                .color(TEXT_DIM)
                .monospace()
                .size(10.0),
        );
    });
}

fn badge(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::new()
        .fill(Color32::from_rgba_premultiplied(color.r(), color.g(), color.b(), 0x20))
        .stroke(egui::Stroke::new(0.5, color))
        .corner_radius(egui::CornerRadius::same(3))
        .inner_margin(egui::Margin::symmetric(4, 1))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .color(color)
                    .monospace()
                    .size(9.0)
                    .strong(),
            );
        });
}

fn dim_label(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).color(TEXT_DIM).monospace().size(11.0));
}

fn hint_label(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .color(TEXT_DIM)
            .monospace()
            .size(9.0)
            .italics(),
    );
}

// ═══════════════════════════════════════════════════════════════
// APP STATE
// ═══════════════════════════════════════════════════════════════

#[derive(PartialEq)]
enum Tab {
    General,
    Models,
    Words,
    PostProcess,
    Advanced,
    History,
}

struct SettingsApp {
    app_handle: tauri::AppHandle,
    current_tab: Tab,
    settings: AppSettings,
    models: Vec<ModelInfo>,
    microphones: Vec<AudioDevice>,
    history_entries: Vec<HistoryEntry>,
    custom_words_text: String,
    new_word_input: String,
    status_message: Option<(String, std::time::Instant)>,
    history_dirty: Arc<AtomicBool>,
    new_prompt_name: String,
    new_prompt_text: String,
    api_key_visible: bool,
    test_input: String,
    test_result: Arc<std::sync::Mutex<Option<String>>>,
    test_running: Arc<AtomicBool>,
    logo_texture: Option<egui::TextureHandle>,
}

impl SettingsApp {
    fn new(app_handle: tauri::AppHandle) -> Self {
        let settings = settings::get_settings(&app_handle);
        let custom_words_text = settings.custom_words.join(", ");

        let model_manager = app_handle.state::<Arc<ModelManager>>();
        let models = model_manager.get_available_models();

        let microphones = commands::audio::get_available_microphones().unwrap_or_default();

        let history_manager = app_handle.state::<Arc<HistoryManager>>();
        let history_entries = {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(history_manager.get_history_entries())
            }.unwrap_or_default();

        let history_dirty = Arc::new(AtomicBool::new(false));
        let dirty_flag = history_dirty.clone();
        app_handle.listen("history-updated", move |_| {
            dirty_flag.store(true, Ordering::Relaxed);
        });

        Self {
            app_handle,
            current_tab: Tab::General,
            settings,
            models,
            microphones,
            history_entries,
            custom_words_text,
            new_word_input: String::new(),
            status_message: None,
            history_dirty,
            new_prompt_name: String::new(),
            new_prompt_text: String::new(),
            api_key_visible: false,
            test_input: "bonjour, cest un test de post processing.".to_string(),
            test_result: Arc::new(std::sync::Mutex::new(None)),
            test_running: Arc::new(AtomicBool::new(false)),
            logo_texture: None,
        }
    }

    fn save_settings(&self) {
        settings::write_settings(&self.app_handle, self.settings.clone());
    }

    fn reload_models(&mut self) {
        let model_manager = self.app_handle.state::<Arc<ModelManager>>();
        self.models = model_manager.get_available_models();
    }

    fn reload_history(&mut self) {
        let history_manager = self.app_handle.state::<Arc<HistoryManager>>();
        if let Ok(entries) = {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(history_manager.get_history_entries())
            } {
            self.history_entries = entries;
        }
    }

    fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some((msg.into(), std::time::Instant::now()));
    }

    fn get_or_load_logo(&mut self, ctx: &egui::Context) -> Option<&egui::TextureHandle> {
        if self.logo_texture.is_none() {
            let png_data = include_bytes!("../../linux-logo.png");
            if let Ok(img) = image::load_from_memory(png_data) {
                let rgba = img.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                let pixels = rgba.into_raw();
                let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
                self.logo_texture = Some(ctx.load_texture(
                    "logo",
                    color_image,
                    egui::TextureOptions::LINEAR,
                ));
            }
        }
        self.logo_texture.as_ref()
    }

    // ═══════════════════════════════════════════════════════════
    // SIDEBAR
    // ═══════════════════════════════════════════════════════════

    fn render_sidebar(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        ui.add_space(12.0);

        // Logo with blinking cursor
        let time = ui.input(|i| i.time);
        let cursor = if (time * 2.0) as i32 % 2 == 0 { "\u{2588}" } else { " " };
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new(format!("> Handy{}", cursor))
                    .size(18.0)
                    .color(ACCENT_GREEN)
                    .monospace()
                    .strong(),
            );
            ui.label(
                egui::RichText::new(format!("MadWhisp v{}", env!("CARGO_PKG_VERSION")))
                    .size(9.0)
                    .color(TEXT_DIM)
                    .monospace(),
            );
        });

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}")
                    .color(BORDER_SUBTLE)
                    .monospace()
                    .size(9.0),
            );
        });
        ui.add_space(6.0);

        // Nav items
        let tabs = [
            (Tab::General, "\u{25A0}", "General"),
            (Tab::Models, "\u{25A0}", "Models"),
            (Tab::Words, "\u{25A1}", "Words"),
            (Tab::PostProcess, "\u{25C6}", "Process"),
            (Tab::Advanced, "\u{2606}", "Advanced"),
            (Tab::History, "\u{25A0}", "History"),
        ];

        for (tab, icon, label) in tabs {
            let is_active = self.current_tab == tab;

            let nav_text = if is_active {
                format!("\u{2503} {} {}", icon, label)
            } else {
                format!("  {} {}", icon, label)
            };

            let text_color = if is_active { ACCENT_GREEN } else { TEXT_PRIMARY };
            let bg = if is_active { NAV_ACTIVE_BG } else { Color32::TRANSPARENT };

            let btn = egui::Button::new(
                egui::RichText::new(nav_text)
                    .color(text_color)
                    .monospace()
                    .size(12.0)
                    .strong(),
            )
            .fill(bg)
            .stroke(egui::Stroke::NONE)
            .corner_radius(egui::CornerRadius::same(4))
            .min_size(egui::vec2(ui.available_width(), 28.0));

            if ui.add(btn).clicked() {
                self.current_tab = tab;
            }
        }

        // Spacer to push logo + footer to bottom
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
            ui.add_space(8.0);

            // Footer credit
            ui.label(
                egui::RichText::new("madera.tools")
                    .color(TEXT_DIM)
                    .monospace()
                    .size(9.0),
            );

            ui.add_space(4.0);

            // Logo
            if let Some(texture) = self.get_or_load_logo(ctx) {
                let size = egui::vec2(40.0, 40.0);
                ui.image(egui::load::SizedTexture::new(texture.id(), size));
            }

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}")
                        .color(BORDER_SUBTLE)
                        .monospace()
                        .size(9.0),
                );
            });
        });
    }

    // ═══════════════════════════════════════════════════════════
    // GENERAL TAB
    // ═══════════════════════════════════════════════════════════

    fn render_general_tab(&mut self, ui: &mut egui::Ui) {
        ui.label(
            egui::RichText::new("$ config --general")
                .color(ACCENT_GREEN)
                .monospace()
                .size(14.0)
                .strong(),
        );
        ui.add_space(2.0);

        ui.columns(2, |cols| {
            // ── LEFT COLUMN ──
            section(&mut cols[0], "Language & Input", |ui| {
                let languages = [
                    ("auto", "Auto-detect"),
                    ("fr", "French"),
                    ("en", "English"),
                    ("es", "Spanish"),
                    ("de", "German"),
                    ("it", "Italian"),
                    ("pt", "Portuguese"),
                    ("nl", "Dutch"),
                    ("ja", "Japanese"),
                    ("ko", "Korean"),
                    ("zh", "Chinese"),
                    ("ru", "Russian"),
                    ("ar", "Arabic"),
                    ("hi", "Hindi"),
                ];
                let current_label = languages
                    .iter()
                    .find(|(code, _)| *code == self.settings.selected_language)
                    .map(|(_, label)| *label)
                    .unwrap_or(&self.settings.selected_language);
                let current_label = current_label.to_string();

                ui.horizontal(|ui| {
                    dim_label(ui, "Language");
                    egui::ComboBox::from_id_salt("language")
                        .selected_text(current_label)
                        .show_ui(ui, |ui| {
                            for (code, label) in &languages {
                                if ui
                                    .selectable_value(
                                        &mut self.settings.selected_language,
                                        code.to_string(),
                                        *label,
                                    )
                                    .changed()
                                {
                                    self.save_settings();
                                }
                            }
                        });
                });

                let current_mic = self
                    .settings
                    .selected_microphone
                    .as_deref()
                    .unwrap_or("default");
                let current_mic_label = self
                    .microphones
                    .iter()
                    .find(|m| m.index == current_mic)
                    .map(|m| m.name.clone())
                    .unwrap_or_else(|| "Default".to_string());

                ui.horizontal(|ui| {
                    dim_label(ui, "Mic");
                    egui::ComboBox::from_id_salt("microphone")
                        .selected_text(current_mic_label)
                        .show_ui(ui, |ui| {
                            for mic in &self.microphones {
                                let selected = self
                                    .settings
                                    .selected_microphone
                                    .as_deref()
                                    .unwrap_or("default")
                                    == mic.index;
                                if ui.selectable_label(selected, &mic.name).clicked() {
                                    self.settings.selected_microphone =
                                        if mic.index == "default" { None } else { Some(mic.index.clone()) };
                                    self.save_settings();
                                }
                            }
                        });
                });

                if ui.checkbox(&mut self.settings.translate_to_english, "Translate to English").changed() {
                    self.save_settings();
                }
                if ui.checkbox(&mut self.settings.append_trailing_space, "Trailing space").changed() {
                    self.save_settings();
                }
            });

            section(&mut cols[0], "Startup", |ui| {
                if ui.checkbox(&mut self.settings.autostart_enabled, "Launch at startup").changed() {
                    self.save_settings();
                }
            });

            // ── RIGHT COLUMN ──
            section(&mut cols[1], "Recording", |ui| {
                if ui.checkbox(&mut self.settings.push_to_talk, "Push-to-talk").changed() {
                    self.save_settings();
                }
                if ui.checkbox(&mut self.settings.audio_feedback, "Audio feedback").changed() {
                    self.save_settings();
                }

                if self.settings.audio_feedback {
                    ui.horizontal(|ui| {
                        dim_label(ui, "Volume");
                        if ui
                            .add(egui::Slider::new(&mut self.settings.audio_feedback_volume, 0.0..=1.0))
                            .changed()
                        {
                            self.save_settings();
                        }
                    });
                }

                if ui.checkbox(&mut self.settings.mute_while_recording, "Mute while recording").changed() {
                    self.save_settings();
                }

                ui.horizontal(|ui| {
                    dim_label(ui, "Theme");
                    egui::ComboBox::from_id_salt("sound_theme")
                        .selected_text(match self.settings.sound_theme {
                            SoundTheme::Marimba => "Marimba",
                            SoundTheme::Pop => "Pop",
                            SoundTheme::Custom => "Custom",
                        })
                        .show_ui(ui, |ui| {
                            for (theme, label) in [
                                (SoundTheme::Marimba, "Marimba"),
                                (SoundTheme::Pop, "Pop"),
                                (SoundTheme::Custom, "Custom"),
                            ] {
                                if ui.selectable_value(&mut self.settings.sound_theme, theme, label).changed() {
                                    self.save_settings();
                                }
                            }
                        });
                });
            });
        });
    }

    // ═══════════════════════════════════════════════════════════
    // MODELS TAB
    // ═══════════════════════════════════════════════════════════

    fn render_models_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("$ models --list")
                    .color(ACCENT_GREEN)
                    .monospace()
                    .size(14.0)
                    .strong(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Refresh").clicked() {
                    self.reload_models();
                }
            });
        });
        ui.add_space(2.0);

        let mut downloaded: Vec<ModelInfo> =
            self.models.iter().filter(|m| m.is_downloaded).cloned().collect();
        let mut available: Vec<ModelInfo> =
            self.models.iter().filter(|m| !m.is_downloaded).cloned().collect();
        downloaded.sort_by(|a, b| a.name.cmp(&b.name));
        available.sort_by(|a, b| a.name.cmp(&b.name));

        let mut model_to_select: Option<String> = None;
        let mut model_to_delete: Option<String> = None;
        let mut model_to_download: Option<String> = None;

        if !downloaded.is_empty() {
            section(ui, "Downloaded", |ui| {
                for model in &downloaded {
                    let is_active = model.id == self.settings.selected_model;
                    Self::render_model_card(ui, model, is_active, true, &mut model_to_select, &mut model_to_delete, &mut model_to_download);
                    ui.add_space(2.0);
                }
            });
        }

        if !available.is_empty() {
            section(ui, "Available for Download", |ui| {
                for model in &available {
                    Self::render_model_card(ui, model, false, false, &mut model_to_select, &mut model_to_delete, &mut model_to_download);
                    ui.add_space(2.0);
                }
            });
        }

        // Process deferred actions
        if let Some(model_id) = model_to_select {
            let app = self.app_handle.clone();
            let id = model_id.clone();
            std::thread::spawn(move || {
                if let Err(e) = commands::models::switch_active_model(&app, &id) {
                    log::error!("Failed to switch model: {}", e);
                }
            });
            self.settings.selected_model = model_id;
            self.save_settings();
            self.set_status("Model selected");
        }

        if let Some(model_id) = model_to_delete {
            let model_manager = self.app_handle.state::<Arc<ModelManager>>();
            let transcription_manager = self.app_handle.state::<Arc<TranscriptionManager>>();

            if self.settings.selected_model == model_id {
                let _ = transcription_manager.unload_model();
                self.settings.selected_model = String::new();
                self.save_settings();
            }

            match model_manager.delete_model(&model_id) {
                Ok(()) => {
                    self.set_status(format!("Deleted {}", model_id));
                    self.reload_models();
                }
                Err(e) => self.set_status(format!("Delete failed: {}", e)),
            }
        }

        if let Some(model_id) = model_to_download {
            let model_manager = self.app_handle.state::<Arc<ModelManager>>();
            let mm = model_manager.inner().clone();
            let id = model_id.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                if let Err(e) = rt.block_on(mm.download_model(&id)) {
                    log::error!("Download failed: {}", e);
                }
            });
            self.set_status(format!("Downloading {}...", model_id));
        }
    }

    fn render_model_card(
        ui: &mut egui::Ui,
        model: &ModelInfo,
        is_active: bool,
        is_downloaded: bool,
        select_action: &mut Option<String>,
        delete_action: &mut Option<String>,
        download_action: &mut Option<String>,
    ) {
        let border_color = if is_active { ACCENT_GREEN } else { BORDER_SUBTLE };
        egui::Frame::new()
            .fill(if is_active { Color32::from_rgb(0x0a, 0x1a, 0x15) } else { BG_INPUT })
            .stroke(egui::Stroke::new(if is_active { 1.0 } else { 0.5 }, border_color))
            .corner_radius(egui::CornerRadius::same(6))
            .inner_margin(egui::Margin::same(10))
            .show(ui, |ui| {
                // Row 1: Name + badges + buttons
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(&model.name)
                            .color(TEXT_PRIMARY)
                            .monospace()
                            .size(13.0)
                            .strong(),
                    );
                    if is_active {
                        badge(ui, "ACTIVE", ACCENT_GREEN);
                    }
                    if model.is_recommended {
                        badge(ui, "REC", ACCENT_YELLOW);
                    }
                    ui.label(
                        egui::RichText::new(format!("{}MB | {:?}", model.size_mb, model.engine_type))
                            .color(TEXT_DIM)
                            .monospace()
                            .size(10.0),
                    );
                    if model.supports_translation {
                        badge(ui, "TR", ACCENT_CYAN);
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if is_downloaded {
                            if ui.button(egui::RichText::new("Del").color(ACCENT_RED).monospace().size(10.0)).clicked() {
                                *delete_action = Some(model.id.clone());
                            }
                            if !is_active && ui.button(egui::RichText::new("Use").monospace().size(10.0)).clicked() {
                                *select_action = Some(model.id.clone());
                            }
                        } else if model.is_downloading {
                            ui.spinner();
                        } else if ui.button(egui::RichText::new("Get").color(ACCENT_GREEN).monospace().size(10.0)).clicked() {
                            *download_action = Some(model.id.clone());
                        }
                    });
                });

                // Row 2: Description
                if !model.description.is_empty() {
                    ui.label(
                        egui::RichText::new(&model.description)
                            .color(TEXT_DIM)
                            .monospace()
                            .size(10.0),
                    );
                }

                // Row 3: Stats bars (compact, side by side)
                ui.horizontal(|ui| {
                    terminal_bar(ui, "Acc", model.accuracy_score, 8);
                    ui.add_space(8.0);
                    terminal_bar(ui, "Spd", model.speed_score, 8);
                });

                // Row 4: Languages
                if !model.supported_languages.is_empty() {
                    let langs = model.supported_languages.join(" ");
                    ui.label(
                        egui::RichText::new(format!("langs: {}", langs))
                            .color(TEXT_DIM)
                            .monospace()
                            .size(9.0),
                    );
                }
            });
    }

    // ═══════════════════════════════════════════════════════════
    // WORDS TAB
    // ═══════════════════════════════════════════════════════════

    fn render_words_tab(&mut self, ui: &mut egui::Ui) {
        ui.label(
            egui::RichText::new("$ words --manage")
                .color(ACCENT_GREEN)
                .monospace()
                .size(14.0)
                .strong(),
        );
        ui.add_space(2.0);
        hint_label(ui, "Words Whisper often misspells. Fuzzy matching corrects them.");
        ui.add_space(4.0);

        // Add new word
        section(ui, "Add Word", |ui| {
            ui.horizontal(|ui| {
                dim_label(ui, ">");
                let response = ui.text_edit_singleline(&mut self.new_word_input);
                let enter_pressed = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (ui.button("Add").clicked() || enter_pressed) && !self.new_word_input.trim().is_empty() {
                    let word = self.new_word_input.trim().to_string();
                    if !self.settings.custom_words.contains(&word) {
                        self.settings.custom_words.push(word);
                        self.custom_words_text = self.settings.custom_words.join(", ");
                        self.save_settings();
                    }
                    self.new_word_input.clear();
                }
            });
        });

        let mut word_to_remove: Option<usize> = None;

        if self.settings.custom_words.is_empty() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new("No custom words yet.").italics().color(TEXT_DIM).monospace());
            });
        } else {
            section(ui, "Word List", |ui| {
                egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                    for (i, word) in self.settings.custom_words.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(word).strong().monospace().color(ACCENT_CYAN));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button(egui::RichText::new("x").color(ACCENT_RED)).clicked() {
                                    word_to_remove = Some(i);
                                }
                            });
                        });

                        if i < self.settings.custom_words.len() - 1 {
                            ui.label(egui::RichText::new("\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}").color(BORDER_SUBTLE).monospace().size(8.0));
                        }
                    }
                });
            });
        }

        // Apply deferred mutations
        if let Some(idx) = word_to_remove {
            self.settings.custom_words.remove(idx);
            self.custom_words_text = self.settings.custom_words.join(", ");
            self.save_settings();
        }

        // Sensitivity slider
        section(ui, "Sensitivity", |ui| {
            ui.horizontal(|ui| {
                dim_label(ui, "Threshold");
                let mut sensitivity_pct = (self.settings.word_correction_threshold * 200.0).round() as i32;
                if ui.add(egui::Slider::new(&mut sensitivity_pct, 0..=100).suffix("%")).changed() {
                    self.settings.word_correction_threshold = sensitivity_pct as f64 / 200.0;
                    self.save_settings();
                }
            });
            hint_label(ui, "Low=strict. High=loose.");
        });
    }

    // ═══════════════════════════════════════════════════════════
    // POST-PROCESS TAB
    // ═══════════════════════════════════════════════════════════

    fn render_post_process_tab(&mut self, ui: &mut egui::Ui) {
        ui.label(
            egui::RichText::new("$ post-process --config")
                .color(ACCENT_GREEN)
                .monospace()
                .size(14.0)
                .strong(),
        );
        ui.add_space(2.0);
        hint_label(ui, "Send transcriptions to an LLM for cleanup.");

        if ui.checkbox(&mut self.settings.post_process_enabled, egui::RichText::new("Enable post-processing").monospace()).changed() {
            self.save_settings();
        }

        let enabled = self.settings.post_process_enabled;

        // Provider config - full width, compact
        section(ui, "Provider", |ui| {
            if !enabled { ui.disable(); }

            let providers: Vec<(String, String)> = self.settings.post_process_providers.iter().map(|p| (p.id.clone(), p.label.clone())).collect();
            let current_provider_label = providers.iter().find(|(id, _)| *id == self.settings.post_process_provider_id).map(|(_, label)| label.as_str()).unwrap_or("Select...");

            // All on one row or compact rows
            ui.horizontal(|ui| {
                dim_label(ui, "Provider");
                egui::ComboBox::from_id_salt("pp_provider")
                    .selected_text(current_provider_label)
                    .show_ui(ui, |ui| {
                        for (id, label) in &providers {
                            if ui.selectable_value(&mut self.settings.post_process_provider_id, id.clone(), label).changed() {
                                self.save_settings();
                            }
                        }
                    });

                dim_label(ui, "Model");
                let provider_id = self.settings.post_process_provider_id.clone();
                let model = self.settings.post_process_models.entry(provider_id.clone()).or_default();
                if ui.add(egui::TextEdit::singleline(model).desired_width(120.0)).changed() {
                    self.save_settings();
                }
            });

            let provider_id = self.settings.post_process_provider_id.clone();

            ui.horizontal(|ui| {
                dim_label(ui, "API Key");
                let key = self.settings.post_process_api_keys.entry(provider_id.clone()).or_default();
                let response = if self.api_key_visible {
                    ui.add(egui::TextEdit::singleline(key).desired_width(200.0))
                } else {
                    ui.add(egui::TextEdit::singleline(key).password(true).desired_width(200.0))
                };
                if response.changed() { self.save_settings(); }
                if ui.button(if self.api_key_visible { "Hide" } else { "Show" }).clicked() {
                    self.api_key_visible = !self.api_key_visible;
                }
            });

            let allow_base_url_edit = self.settings.post_process_providers.iter().find(|p| p.id == provider_id).map(|p| p.allow_base_url_edit).unwrap_or(false);
            if allow_base_url_edit {
                if let Some(idx) = self.settings.post_process_providers.iter().position(|p| p.id == provider_id) {
                    let mut base_url = self.settings.post_process_providers[idx].base_url.clone();
                    ui.horizontal(|ui| {
                        dim_label(ui, "Base URL");
                        ui.text_edit_singleline(&mut base_url);
                    });
                    if base_url != self.settings.post_process_providers[idx].base_url {
                        self.settings.post_process_providers[idx].base_url = base_url;
                        self.save_settings();
                    }
                }
            }
        });

        // Prompts section
        section(ui, "Prompts", |ui| {
            if !enabled { ui.disable(); }

            let prompts: Vec<(String, String)> = self.settings.post_process_prompts.iter().map(|p| (p.id.clone(), p.name.clone())).collect();
            let selected_id = self.settings.post_process_selected_prompt_id.clone();
            let current_prompt_label = selected_id.as_ref()
                .and_then(|id| prompts.iter().find(|(pid, _)| pid == id))
                .map(|(_, name)| name.as_str())
                .unwrap_or_else(|| prompts.first().map(|(_, name)| name.as_str()).unwrap_or("None"));

            ui.horizontal(|ui| {
                dim_label(ui, "Active");
                egui::ComboBox::from_id_salt("pp_prompt")
                    .selected_text(current_prompt_label)
                    .show_ui(ui, |ui| {
                        for (id, name) in &prompts {
                            let is_selected = selected_id.as_ref() == Some(id);
                            if ui.selectable_label(is_selected, name).clicked() {
                                self.settings.post_process_selected_prompt_id = Some(id.clone());
                                self.save_settings();
                            }
                        }
                    });

                // Delete button inline
                if self.settings.post_process_prompts.len() > 1 {
                    if let Some(ref del_id) = selected_id {
                        if ui.button(egui::RichText::new("Del").color(ACCENT_RED).monospace().size(10.0)).clicked() {
                            let del = del_id.clone();
                            self.settings.post_process_prompts.retain(|p| p.id != del);
                            self.settings.post_process_selected_prompt_id = self.settings.post_process_prompts.first().map(|p| p.id.clone());
                            self.save_settings();
                        }
                    }
                }
            });

            // Edit selected prompt
            let editing_id = self.settings.post_process_selected_prompt_id.clone()
                .or_else(|| self.settings.post_process_prompts.first().map(|p| p.id.clone()));

            if let Some(ref edit_id) = editing_id {
                if let Some(idx) = self.settings.post_process_prompts.iter().position(|p| p.id == *edit_id) {
                    let mut name = self.settings.post_process_prompts[idx].name.clone();
                    let mut prompt_text = self.settings.post_process_prompts[idx].prompt.clone();

                    ui.horizontal(|ui| {
                        dim_label(ui, "Name");
                        ui.text_edit_singleline(&mut name);
                    });
                    ui.add(
                        egui::TextEdit::multiline(&mut prompt_text)
                            .desired_rows(6)
                            .desired_width(f32::INFINITY)
                            .font(egui::FontId::new(11.0, egui::FontFamily::Monospace)),
                    );
                    hint_label(ui, "${output} = transcription placeholder");

                    let changed = name != self.settings.post_process_prompts[idx].name
                        || prompt_text != self.settings.post_process_prompts[idx].prompt;
                    if changed {
                        self.settings.post_process_prompts[idx].name = name;
                        self.settings.post_process_prompts[idx].prompt = prompt_text;
                        self.save_settings();
                    }
                }
            }

            // Add new prompt (collapsed)
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                dim_label(ui, "New prompt");
                ui.text_edit_singleline(&mut self.new_prompt_name);
            });

            if !self.new_prompt_name.trim().is_empty() {
                ui.add(egui::TextEdit::multiline(&mut self.new_prompt_text).desired_rows(3).desired_width(f32::INFINITY));
                if ui.button("Add prompt").clicked() && !self.new_prompt_text.trim().is_empty() {
                    let id = format!("custom_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis());
                    self.settings.post_process_prompts.push(crate::settings::LLMPrompt {
                        id: id.clone(),
                        name: self.new_prompt_name.trim().to_string(),
                        prompt: self.new_prompt_text.trim().to_string(),
                    });
                    self.settings.post_process_selected_prompt_id = Some(id);
                    self.save_settings();
                    self.new_prompt_name.clear();
                    self.new_prompt_text.clear();
                }
            }
        });

        // Test section
        section(ui, "Test", |ui| {
            if !enabled { ui.disable(); }

            ui.columns(2, |cols| {
                cols[0].label(egui::RichText::new("Input:").color(TEXT_DIM).monospace().size(10.0));
                cols[0].add(egui::TextEdit::multiline(&mut self.test_input).desired_rows(2).desired_width(f32::INFINITY).font(egui::FontId::new(11.0, egui::FontFamily::Monospace)));

                cols[1].label(egui::RichText::new("Output:").color(TEXT_DIM).monospace().size(10.0));
                if let Some(result) = self.test_result.lock().unwrap().as_ref() {
                    let mut result_display = result.clone();
                    cols[1].add(egui::TextEdit::multiline(&mut result_display).desired_rows(2).desired_width(f32::INFINITY).interactive(false).font(egui::FontId::new(11.0, egui::FontFamily::Monospace)));
                } else {
                    let mut empty = String::from("...");
                    cols[1].add(egui::TextEdit::multiline(&mut empty).desired_rows(2).desired_width(f32::INFINITY).interactive(false));
                }
            });

            let is_running = self.test_running.load(Ordering::Relaxed);
            ui.horizontal(|ui| {
                if is_running {
                    ui.spinner();
                    ui.label(egui::RichText::new("Processing...").color(ACCENT_YELLOW).monospace());
                } else if ui.button("Run test").clicked() && !self.test_input.trim().is_empty() {
                    let settings = self.settings.clone();
                    let input = self.test_input.clone();
                    let result_ref = self.test_result.clone();
                    let running_ref = self.test_running.clone();
                    running_ref.store(true, Ordering::Relaxed);
                    *result_ref.lock().unwrap() = None;
                    std::thread::spawn(move || {
                        let rt = tokio::runtime::Runtime::new().unwrap();
                        let output = rt.block_on(crate::actions::post_process_transcription(&settings, &input));
                        *result_ref.lock().unwrap() = Some(output.unwrap_or_else(|| "ERROR: returned nothing.".to_string()));
                        running_ref.store(false, Ordering::Relaxed);
                    });
                }
            });
        });
    }

    // ═══════════════════════════════════════════════════════════
    // ADVANCED TAB
    // ═══════════════════════════════════════════════════════════

    fn render_advanced_tab(&mut self, ui: &mut egui::Ui) {
        ui.label(
            egui::RichText::new("$ config --advanced")
                .color(ACCENT_GREEN)
                .monospace()
                .size(14.0)
                .strong(),
        );
        ui.add_space(2.0);

        ui.columns(2, |cols| {
            // ── LEFT COLUMN ──
            section(&mut cols[0], "Paste & Input", |ui| {
                ui.horizontal(|ui| {
                    dim_label(ui, "Method");
                    egui::ComboBox::from_id_salt("paste_method")
                        .selected_text(format!("{:?}", self.settings.paste_method))
                        .show_ui(ui, |ui| {
                            for (method, label) in [
                                (PasteMethod::CtrlV, "Ctrl+V"),
                                (PasteMethod::Direct, "Direct"),
                                (PasteMethod::ShiftInsert, "Shift+Ins"),
                                (PasteMethod::CtrlShiftV, "Ctrl+Shift+V"),
                                (PasteMethod::ExternalScript, "Script"),
                                (PasteMethod::None, "None"),
                            ] {
                                if ui.selectable_value(&mut self.settings.paste_method, method, label).changed() {
                                    self.save_settings();
                                }
                            }
                        });
                });

                if self.settings.paste_method == PasteMethod::ExternalScript {
                    ui.horizontal(|ui| {
                        dim_label(ui, "Script");
                        let mut path = self.settings.external_script_path.clone().unwrap_or_default();
                        if ui.text_edit_singleline(&mut path).changed() {
                            self.settings.external_script_path = if path.is_empty() { None } else { Some(path) };
                            self.save_settings();
                        }
                    });
                }

                ui.horizontal(|ui| {
                    dim_label(ui, "Delay");
                    let mut delay = self.settings.paste_delay_ms as i32;
                    if ui.add(egui::Slider::new(&mut delay, 0..=500).suffix("ms")).changed() {
                        self.settings.paste_delay_ms = delay as u64;
                        self.save_settings();
                    }
                });

                ui.horizontal(|ui| {
                    dim_label(ui, "Typing");
                    egui::ComboBox::from_id_salt("typing_tool")
                        .selected_text(format!("{:?}", self.settings.typing_tool))
                        .show_ui(ui, |ui| {
                            for (tool, label) in [
                                (TypingTool::Auto, "Auto"),
                                (TypingTool::Wtype, "wtype"),
                                (TypingTool::Kwtype, "kwtype"),
                                (TypingTool::Dotool, "dotool"),
                                (TypingTool::Ydotool, "ydotool"),
                                (TypingTool::Xdotool, "xdotool"),
                            ] {
                                if ui.selectable_value(&mut self.settings.typing_tool, tool, label).changed() {
                                    self.save_settings();
                                }
                            }
                        });
                });

                ui.horizontal(|ui| {
                    dim_label(ui, "Clipboard");
                    egui::ComboBox::from_id_salt("clipboard_handling")
                        .selected_text(match self.settings.clipboard_handling {
                            ClipboardHandling::DontModify => "Don't modify",
                            ClipboardHandling::CopyToClipboard => "Copy to clip",
                        })
                        .show_ui(ui, |ui| {
                            if ui.selectable_value(&mut self.settings.clipboard_handling, ClipboardHandling::DontModify, "Don't modify").changed() { self.save_settings(); }
                            if ui.selectable_value(&mut self.settings.clipboard_handling, ClipboardHandling::CopyToClipboard, "Copy to clipboard").changed() { self.save_settings(); }
                        });
                });

                if ui.checkbox(&mut self.settings.auto_submit, "Auto-submit").changed() {
                    self.save_settings();
                }

                if self.settings.auto_submit {
                    ui.horizontal(|ui| {
                        dim_label(ui, "Key");
                        egui::ComboBox::from_id_salt("auto_submit_key")
                            .selected_text(format!("{:?}", self.settings.auto_submit_key))
                            .show_ui(ui, |ui| {
                                for key in [AutoSubmitKey::Enter, AutoSubmitKey::CtrlEnter, AutoSubmitKey::CmdEnter] {
                                    if ui.selectable_value(&mut self.settings.auto_submit_key, key, format!("{:?}", key)).changed() { self.save_settings(); }
                                }
                            });
                    });
                }
            });

            // ── RIGHT COLUMN ──
            section(&mut cols[1], "Display & Model", |ui| {
                ui.horizontal(|ui| {
                    dim_label(ui, "Overlay");
                    egui::ComboBox::from_id_salt("overlay_position")
                        .selected_text(match self.settings.overlay_position {
                            OverlayPosition::None => "None",
                            OverlayPosition::Top => "Top",
                            OverlayPosition::Bottom => "Bottom",
                        })
                        .show_ui(ui, |ui| {
                            for (pos, label) in [(OverlayPosition::None, "None"), (OverlayPosition::Top, "Top"), (OverlayPosition::Bottom, "Bottom")] {
                                if ui.selectable_value(&mut self.settings.overlay_position, pos, label).changed() { self.save_settings(); }
                            }
                        });
                });

                ui.horizontal(|ui| {
                    dim_label(ui, "Unload");
                    egui::ComboBox::from_id_salt("model_unload_timeout")
                        .selected_text(match self.settings.model_unload_timeout {
                            ModelUnloadTimeout::Never => "Never",
                            ModelUnloadTimeout::Immediately => "Now",
                            ModelUnloadTimeout::Min2 => "2m",
                            ModelUnloadTimeout::Min5 => "5m",
                            ModelUnloadTimeout::Min10 => "10m",
                            ModelUnloadTimeout::Min15 => "15m",
                            ModelUnloadTimeout::Hour1 => "1h",
                            ModelUnloadTimeout::Sec5 => "5s",
                        })
                        .show_ui(ui, |ui| {
                            for (timeout, label) in [
                                (ModelUnloadTimeout::Never, "Never"),
                                (ModelUnloadTimeout::Immediately, "Immediately"),
                                (ModelUnloadTimeout::Min2, "2 min"),
                                (ModelUnloadTimeout::Min5, "5 min"),
                                (ModelUnloadTimeout::Min10, "10 min"),
                                (ModelUnloadTimeout::Min15, "15 min"),
                                (ModelUnloadTimeout::Hour1, "1 hour"),
                            ] {
                                if ui.selectable_value(&mut self.settings.model_unload_timeout, timeout, label).changed() { self.save_settings(); }
                            }
                        });
                });
            });

            section(&mut cols[1], "Data", |ui| {
                ui.horizontal(|ui| {
                    dim_label(ui, "History");
                    egui::ComboBox::from_id_salt("history_limit")
                        .selected_text(format!("{}", self.settings.history_limit))
                        .show_ui(ui, |ui| {
                            for limit in [5, 10, 25, 50, 100, 250] {
                                if ui.selectable_value(&mut self.settings.history_limit, limit, format!("{}", limit)).changed() { self.save_settings(); }
                            }
                        });
                });

                ui.horizontal(|ui| {
                    dim_label(ui, "Recordings");
                    use crate::settings::RecordingRetentionPeriod;
                    let options = [
                        (RecordingRetentionPeriod::Never, "Never"),
                        (RecordingRetentionPeriod::PreserveLimit, "History limit"),
                        (RecordingRetentionPeriod::Days3, "3 days"),
                        (RecordingRetentionPeriod::Weeks2, "2 weeks"),
                        (RecordingRetentionPeriod::Months3, "3 months"),
                    ];
                    let current_label = options.iter().find(|(v, _)| *v == self.settings.recording_retention_period).map(|(_, l)| *l).unwrap_or("?");
                    egui::ComboBox::from_id_salt("retention_period")
                        .selected_text(current_label)
                        .show_ui(ui, |ui| {
                            for (value, label) in options {
                                if ui.selectable_value(&mut self.settings.recording_retention_period, value, label).changed() { self.save_settings(); }
                            }
                        });
                });
            });

            section(&mut cols[1], "System", |ui| {
                ui.horizontal(|ui| {
                    dim_label(ui, "Keyboard");
                    egui::ComboBox::from_id_salt("keyboard_impl")
                        .selected_text(format!("{:?}", self.settings.keyboard_implementation))
                        .show_ui(ui, |ui| {
                            for (impl_, label) in [(KeyboardImplementation::Tauri, "Tauri"), (KeyboardImplementation::HandyKeys, "HandyKeys")] {
                                if ui.selectable_value(&mut self.settings.keyboard_implementation, impl_, label).changed() { self.save_settings(); }
                            }
                        });
                });

                if ui.checkbox(&mut self.settings.debug_mode, "Debug mode").changed() { self.save_settings(); }

                if self.settings.debug_mode {
                    ui.horizontal(|ui| {
                        dim_label(ui, "Log");
                        egui::ComboBox::from_id_salt("log_level")
                            .selected_text(format!("{:?}", self.settings.log_level))
                            .show_ui(ui, |ui| {
                                for (level, label) in [(LogLevel::Error, "Error"), (LogLevel::Warn, "Warn"), (LogLevel::Info, "Info"), (LogLevel::Debug, "Debug"), (LogLevel::Trace, "Trace")] {
                                    if ui.selectable_value(&mut self.settings.log_level, level, label).changed() { self.save_settings(); }
                                }
                            });
                    });
                }

                if ui.checkbox(&mut self.settings.experimental_enabled, "Experimental").changed() { self.save_settings(); }
            });
        });
    }

    // ═══════════════════════════════════════════════════════════
    // HISTORY TAB
    // ═══════════════════════════════════════════════════════════

    fn render_history_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("$ history --list")
                    .color(ACCENT_GREEN)
                    .monospace()
                    .size(14.0)
                    .strong(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Refresh").clicked() { self.reload_history(); }
                ui.label(egui::RichText::new(format!("{} entries", self.history_entries.len())).color(TEXT_DIM).monospace().size(10.0));
            });
        });
        ui.add_space(2.0);

        if self.history_entries.is_empty() {
            ui.add_space(32.0);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new("No transcription history.").italics().color(TEXT_DIM).monospace());
            });
            return;
        }

        let mut entry_to_delete: Option<i64> = None;
        let mut text_to_copy: Option<String> = None;

        // Header
        egui::Frame::new()
            .fill(BG_CARD)
            .inner_margin(egui::Margin::symmetric(10, 4))
            .corner_radius(egui::CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 })
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("TIMESTAMP").color(ACCENT_CYAN).monospace().size(10.0).strong());
                    ui.add_space(60.0);
                    ui.label(egui::RichText::new("TRANSCRIPTION").color(ACCENT_CYAN).monospace().size(10.0).strong());
                });
            });

        egui::ScrollArea::vertical().show(ui, |ui| {
            for (i, entry) in self.history_entries.iter().enumerate() {
                let bg = if i % 2 == 0 { BG_INPUT } else { BG_DARK };

                egui::Frame::new()
                    .fill(bg)
                    .stroke(egui::Stroke::new(0.5, BORDER_SUBTLE))
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        let text = entry.post_processed_text.as_deref().unwrap_or(&entry.transcription_text);
                        let preview = if text.len() > 100 { format!("{}...", &text[..100]) } else { text.to_string() };

                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(&entry.title).color(TEXT_DIM).monospace().size(10.0));
                            if entry.post_processed_text.is_some() {
                                badge(ui, "PP", ACCENT_CYAN);
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button(egui::RichText::new("x").color(ACCENT_RED).monospace()).clicked() {
                                    entry_to_delete = Some(entry.id);
                                }
                                if ui.small_button(egui::RichText::new("cp").color(ACCENT_GREEN).monospace()).clicked() {
                                    text_to_copy = Some(text.to_string());
                                }
                            });
                        });

                        ui.label(egui::RichText::new(&preview).color(TEXT_PRIMARY).monospace().size(11.0));
                    });
            }
        });

        if let Some(text) = text_to_copy {
            let _ = self.app_handle.clipboard().write_text(&text);
            self.set_status("Copied to clipboard");
        }

        if let Some(id) = entry_to_delete {
            let history_manager = self.app_handle.state::<Arc<HistoryManager>>();
            let hm = history_manager.inner().clone();
            if let Ok(rt) = tokio::runtime::Handle::try_current() {
                let _ = rt.block_on(hm.delete_entry(id));
            }
            self.reload_history();
            self.set_status("Entry deleted");
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// EFRAME APP
// ═══════════════════════════════════════════════════════════════

impl eframe::App for SettingsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        apply_hacker_theme(ctx);
        ctx.request_repaint_after(std::time::Duration::from_millis(500));



        if self.history_dirty.swap(false, Ordering::Relaxed) {
            self.reload_history();
        }

        if let Some((_, instant)) = &self.status_message {
            if instant.elapsed() > std::time::Duration::from_secs(3) {
                self.status_message = None;
            }
        }

        // ── SIDEBAR ──
        egui::SidePanel::left("nav_sidebar")
            .exact_width(170.0)
            .frame(egui::Frame::new().fill(BG_SIDEBAR).inner_margin(egui::Margin::same(0)))
            .show(ctx, |ui| {
                self.render_sidebar(ctx, ui);
            });

        // ── STATUS BAR ──
        egui::TopBottomPanel::bottom("status_bar")
            .frame(egui::Frame::new().fill(BG_SIDEBAR).inner_margin(egui::Margin::symmetric(12, 4)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if let Some((msg, _)) = &self.status_message {
                        ui.label(egui::RichText::new(format!("> {}", msg)).color(ACCENT_GREEN).monospace().size(10.0));
                    } else {
                        let model_name = if self.settings.selected_model.is_empty() { "none".to_string() } else { self.settings.selected_model.clone() };
                        ui.label(egui::RichText::new(format!("[READY] model: {} | lang: {}", model_name, self.settings.selected_language)).color(TEXT_DIM).monospace().size(9.0));
                    }
                });
            });

        // ── MAIN CONTENT ──
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG_DARK).inner_margin(egui::Margin::same(14)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    match self.current_tab {
                        Tab::General => self.render_general_tab(ui),
                        Tab::Models => self.render_models_tab(ui),
                        Tab::Words => self.render_words_tab(ui),
                        Tab::PostProcess => self.render_post_process_tab(ui),
                        Tab::Advanced => self.render_advanced_tab(ui),
                        Tab::History => self.render_history_tab(ui),
                    }
                });
            });
    }
}

// ═══════════════════════════════════════════════════════════════
// ENTRY POINT
// ═══════════════════════════════════════════════════════════════

pub fn run_settings_window(app_handle: tauri::AppHandle) -> eframe::Result<()> {
    use winit::platform::wayland::EventLoopBuilderExtWayland;

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 750.0])
            .with_min_inner_size([900.0, 650.0])
            .with_title("MadWhisp")
            .with_app_id("madwhisp"),
        event_loop_builder: Some(Box::new(|builder| {
            builder.with_any_thread(true);
        })),
        ..Default::default()
    };

    eframe::run_native(
        "MadWhisp",
        options,
        Box::new(move |_cc| Ok(Box::new(SettingsApp::new(app_handle)))),
    )
}
