//! UI modules for the Jewelry Cost Calculator

pub mod left_panel;
pub mod center_panel;
pub mod right_panel;
pub mod gpu_mesh;

use egui::{Context, SidePanel, CentralPanel, TopBottomPanel, Color32, RichText};

use crate::app_state::JewelryCalculatorApp;

/// Galactic color palette for top bar
mod colors {
    use egui::Color32;
    
    pub const NEON_PINK: Color32 = Color32::from_rgb(255, 20, 147);
    pub const NEON_CYAN: Color32 = Color32::from_rgb(0, 255, 255);
    pub const NEON_GREEN: Color32 = Color32::from_rgb(57, 255, 20);
    pub const TEXT_DIM: Color32 = Color32::from_rgb(120, 100, 140);
}

/// Render the complete UI
pub fn render_ui(app: &mut JewelryCalculatorApp, ctx: &Context) {
    // Process any pending async messages
    app.process_async_messages();
    app.clear_old_status();

    // Ensure we repaint if the GPU renderer has pending data to upload
    if app
        .viewer_state
        .gpu_renderer
        .lock()
        .map_or(false, |r| r.has_pending_upload())
    {
        ctx.request_repaint();
    }

    // Top panel with status bar
    render_top_panel(app, ctx);

    // Bottom panel with log (collapsible)
    if app.panel_state.show_log_panel {
        render_bottom_panel(ctx);
    }

    // Left side panel with options
    SidePanel::left("left_panel")
        .default_width(280.0)
        .min_width(250.0)
        .max_width(400.0)
        .show(ctx, |ui| {
            left_panel::render(app, ui, ctx);
        });

    // Right side panel with report
    SidePanel::right("right_panel")
        .default_width(350.0)
        .min_width(300.0)
        .max_width(500.0)
        .show(ctx, |ui| {
            right_panel::render(app, ui, ctx);
        });

    // Central panel with 3D viewer
    CentralPanel::default().show(ctx, |ui| {
        center_panel::render(app, ui, ctx);
    });
}

/// Render the top status bar
fn render_top_panel(app: &mut JewelryCalculatorApp, ctx: &Context) {
    TopBottomPanel::top("top_panel").show(ctx, |ui| {
        ui.horizontal(|ui| {
            // App title
            ui.heading(RichText::new("Jewelry Cost Calculator").color(colors::NEON_CYAN));

            ui.separator();

            // Log panel toggle button
            let log_btn_text = if app.panel_state.show_log_panel {
                RichText::new("📋 Hide Log").color(colors::NEON_PINK)
            } else {
                RichText::new("📋 Show Log").color(colors::TEXT_DIM)
            };
            if ui.button(log_btn_text).clicked() {
                app.panel_state.show_log_panel = !app.panel_state.show_log_panel;
            }

            // Profiler toggle button
            let profiler_on = puffin::are_scopes_on();
            let profiler_btn_text = if profiler_on {
                RichText::new("🔬 Profiler ON").color(colors::NEON_GREEN)
            } else {
                RichText::new("🔬 Profiler").color(colors::TEXT_DIM)
            };
            if ui.button(profiler_btn_text)
                .on_hover_text("Toggle profiling. Connect with: puffin_viewer (127.0.0.1:8585)")
                .clicked() 
            {
                puffin::set_scopes_on(!profiler_on);
            }

            ui.separator();

            // Loading indicators
            if app.mesh_loading {
                ui.spinner();
                ui.label(RichText::new("Loading mesh...").color(colors::NEON_PINK));
            } else if app.prices_loading {
                ui.spinner();
                ui.label(RichText::new("Fetching prices...").color(colors::NEON_PINK));
            }

            // Status message
            if let Some((msg, _)) = &app.status_message {
                ui.separator();
                ui.label(RichText::new(msg).color(colors::NEON_GREEN));
            }

            // Price status (right-aligned)
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if app.metal_prices.is_live {
                    ui.label(RichText::new(format!("Prices: {}", app.metal_prices.age_string())).color(colors::NEON_GREEN));
                } else {
                    ui.label(RichText::new("Prices: Default (not live)").color(Color32::YELLOW));
                }
            });
        });
    });
}

/// Render the bottom log panel
fn render_bottom_panel(ctx: &Context) {
    TopBottomPanel::bottom("log_panel")
        .resizable(true)
        .default_height(200.0)
        .max_height(400.0)
        .show(ctx, |ui| 
            egui_logger::logger_ui().show(ui)
        );
}

/// Format a number with appropriate precision
#[allow(dead_code)]
pub fn format_number(value: f64, decimals: usize) -> String {
    format!("{:.1$}", value, decimals)
}

/// Format a currency value
pub fn format_currency(value: f64) -> String {
    if value >= 1000.0 {
        format!("${:.2}", value)
    } else if value >= 1.0 {
        format!("${:.2}", value)
    } else {
        format!("${:.3}", value)
    }
}
