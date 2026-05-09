//! Jewelry Cost Calculator
//!
//! A desktop application for calculating material costs for jewelry pieces.
//! Supports STL and OBJ file formats, with special features for rings including
//! size scaling and automatic inner diameter detection.

pub const METAL_API_KEY: &str = env!("METAL_API_KEY");

/// Whether profiling is enabled (toggle with checkbox in UI)
pub static PROFILING_ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
const STYLE: &str = r#"{"override_text_style":null,"override_font_id":null,"override_text_valign":"Center","text_styles":{"Small":{"size":10.0,"family":"Proportional"},"Body":{"size":14.0,"family":"Proportional"},"Monospace":{"size":12.0,"family":"Monospace"},"Button":{"size":14.0,"family":"Proportional"},"Heading":{"size":18.0,"family":"Proportional"}},"drag_value_text_style":"Button","wrap":null,"wrap_mode":null,"spacing":{"item_spacing":{"x":3.0,"y":3.0},"window_margin":{"left":12,"right":12,"top":12,"bottom":12},"button_padding":{"x":5.0,"y":3.0},"menu_margin":{"left":12,"right":12,"top":12,"bottom":12},"indent":18.0,"interact_size":{"x":40.0,"y":20.0},"slider_width":100.0,"slider_rail_height":8.0,"combo_width":100.0,"text_edit_width":280.0,"icon_width":14.0,"icon_width_inner":8.0,"icon_spacing":6.0,"default_area_size":{"x":600.0,"y":400.0},"tooltip_width":600.0,"menu_width":400.0,"menu_spacing":2.0,"indent_ends_with_horizontal_line":false,"combo_height":200.0,"scroll":{"floating":true,"bar_width":6.0,"handle_min_length":12.0,"bar_inner_margin":4.0,"bar_outer_margin":0.0,"floating_width":2.0,"floating_allocated_width":0.0,"foreground_color":true,"dormant_background_opacity":0.0,"active_background_opacity":0.4,"interact_background_opacity":0.7,"dormant_handle_opacity":0.0,"active_handle_opacity":0.6,"interact_handle_opacity":1.0}},"interaction":{"interact_radius":5.0,"resize_grab_radius_side":5.0,"resize_grab_radius_corner":10.0,"show_tooltips_only_when_still":true,"tooltip_delay":0.5,"tooltip_grace_time":0.2,"selectable_labels":true,"multi_widget_text_select":true},"visuals":{"dark_mode":true,"text_alpha_from_coverage":"TwoCoverageMinusCoverageSq","override_text_color":[207,216,220,255],"weak_text_alpha":0.6,"weak_text_color":null,"widgets":{"noninteractive":{"bg_fill":[0,0,0,0],"weak_bg_fill":[61,61,61,232],"bg_stroke":{"width":1.0,"color":[71,71,71,247]},"corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"fg_stroke":{"width":1.0,"color":[207,216,220,255]},"expansion":0.0},"inactive":{"bg_fill":[58,51,106,0],"weak_bg_fill":[8,8,8,231],"bg_stroke":{"width":1.5,"color":[48,51,73,255]},"corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"fg_stroke":{"width":1.0,"color":[207,216,220,255]},"expansion":0.0},"hovered":{"bg_fill":[37,29,61,97],"weak_bg_fill":[95,62,97,69],"bg_stroke":{"width":1.7,"color":[106,101,155,255]},"corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"fg_stroke":{"width":1.5,"color":[83,87,88,35]},"expansion":2.0},"active":{"bg_fill":[12,12,15,255],"weak_bg_fill":[39,37,54,214],"bg_stroke":{"width":1.0,"color":[12,12,16,255]},"corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"fg_stroke":{"width":2.0,"color":[207,216,220,255]},"expansion":1.0},"open":{"bg_fill":[20,22,28,255],"weak_bg_fill":[17,18,22,255],"bg_stroke":{"width":1.8,"color":[42,44,93,165]},"corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"fg_stroke":{"width":1.0,"color":[109,109,109,255]},"expansion":0.0}},"selection":{"bg_fill":[23,64,53,27],"stroke":{"width":1.0,"color":[12,12,15,255]}},"hyperlink_color":[135,85,129,255],"faint_bg_color":[17,18,22,255],"extreme_bg_color":[9,12,15,83],"text_edit_bg_color":null,"code_bg_color":[30,31,35,255],"warn_fg_color":[61,185,157,255],"error_fg_color":[255,55,102,255],"window_corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"window_shadow":{"offset":[0,0],"blur":7,"spread":5,"color":[17,17,41,118]},"window_fill":[11,11,15,255],"window_stroke":{"width":1.0,"color":[77,94,120,138]},"window_highlight_topmost":true,"menu_corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"panel_fill":[12,12,15,255],"popup_shadow":{"offset":[0,0],"blur":8,"spread":3,"color":[19,18,18,96]},"resize_corner_size":18.0,"text_cursor":{"stroke":{"width":2.0,"color":[197,192,255,255]},"preview":true,"blink":true,"on_duration":0.5,"off_duration":0.5},"clip_rect_margin":3.0,"button_frame":true,"collapsing_header_frame":true,"indent_has_left_vline":true,"striped":true,"slider_trailing_fill":true,"handle_shape":{"Rect":{"aspect_ratio":0.5}},"interact_cursor":"Crosshair","image_loading_spinners":true,"numeric_color_space":"GammaByte","disabled_alpha":0.5},"animation_time":0.083333336,"debug":{"debug_on_hover":false,"debug_on_hover_with_all_modifiers":false,"hover_shows_next":false,"show_expand_width":false,"show_expand_height":false,"show_resize":false,"show_interactive_widgets":false,"show_widget_hits":false,"show_unaligned":true},"explanation_tooltips":false,"url_in_tooltip":false,"always_scroll_the_only_direction":false,"scroll_animation":{"points_per_second":1000.0,"duration":{"min":0.1,"max":0.3}},"compact_menu_style":true}"#;

/// Storage key for cached prices
const PRICES_STORAGE_KEY: &str = "cached_metal_prices";

// if i want to find orientation of the ring mesh, i can try and find the longest chain of the same angle of faces in a row. or an odd idea... 
// take an alpha from three different axis, then somewhere, theres got to be a perfect circle for where the finger fits through. 
pub mod app_state;
pub mod materials;
pub mod mesh;
pub mod pricing;
pub mod report;
pub mod ring_sizing;
pub mod ui;
pub mod database;

use app_state::JewelryCalculatorApp;
use tokio::runtime::Handle;

impl eframe::App for JewelryCalculatorApp {
    fn update(&mut self, ctx: &eframe::egui::Context, frame: &mut eframe::Frame) {
        puffin::profile_function!();
        
        // Start a new profiler frame
        puffin::GlobalProfiler::lock().new_frame();
        
        if self.first_run {
            match serde_json::from_str::<eframe::egui::Style>(STYLE) {
                Ok(mut theme) => {
                    theme.visuals.widgets.active.fg_stroke = egui::Stroke::new(1., egui::Color32::WHITE);
                    let style = std::sync::Arc::new(theme);
                    ctx.set_style(style);
                }
                Err(e) => eprintln!("Error setting theme: {e:?}")
            };

            // Try to load cached prices from storage
            if let Some(storage) = frame.storage() {
                if let Some(prices_json) = storage.get_string(PRICES_STORAGE_KEY) {
                    if let Ok(cached) = serde_json::from_str::<pricing::CachedPrices>(&prices_json) {
                        // Check if prices are less than 24 hours old
                        let age = chrono::Utc::now() - cached.fetched_at;
                        if age.num_hours() < 24 {
                            self.metal_prices = cached.prices;
                            self.set_status(format!("Loaded cached prices ({})", self.metal_prices.age_string()));
                        } else {
                            // Prices are stale, fetch new ones
                            self.fetch_prices(METAL_API_KEY);
                        }
                    }
                }
            }

            // Initialize database
            self.init_database();
            
            self.first_run = false;
        }

        {
            puffin::profile_scope!("render_ui");
            ui::render_ui(self, ctx);
        }

        // Request continuous repaints while loading or profiling
        if self.mesh_loading || self.prices_loading || puffin::are_scopes_on() {
            ctx.request_repaint();
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        // Save current prices to storage
        let cached = pricing::CachedPrices {
            prices: self.metal_prices.clone(),
            fetched_at: self.metal_prices.fetched_at,
        };
        if let Ok(json) = serde_json::to_string(&cached) {
            storage.set_string(PRICES_STORAGE_KEY, json);
        }
    }
}

#[tokio::main]
async fn main() -> eframe::Result<()> {
    // Get the tokio runtime handle
    let runtime = Handle::current();
    
    // Initialize puffin profiler (disabled by default)
    // Enable with the profiler button in the UI, then connect with puffin_viewer
    puffin::set_scopes_on(false);
    
    // Start puffin HTTP server for external viewer connection
    // Connect with: cargo install puffin_viewer && puffin_viewer
    let puffin_server = puffin_http::Server::new("127.0.0.1:8585").ok();
    if puffin_server.is_some() {
        log::info!("Puffin profiler server started on 127.0.0.1:8585");
    }

    // Initialize logger for the bottom panel
    egui_logger::builder()
        .max_level(log::LevelFilter::Info)
        .init()
        .unwrap();

    eframe::run_native(
        &format!("Jewelry Cost Calculator v{}", env!("CARGO_PKG_VERSION")),
        eframe::NativeOptions {
            viewport: eframe::egui::ViewportBuilder::default()
                .with_inner_size([1200.0, 800.0])
                .with_min_inner_size([900.0, 600.0])
                .with_drag_and_drop(true),
            ..Default::default()
        },
        Box::new(move |_cc| {
            let app = JewelryCalculatorApp::new(runtime);
            Ok(Box::new(app))
        }),
    )
}
