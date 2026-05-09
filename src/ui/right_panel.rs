//! Right panel UI - Cost report display

use egui::{Context, Ui, RichText, Color32, ScrollArea};
use rfd::AsyncFileDialog;

use crate::app_state::JewelryCalculatorApp;
use crate::report::JewelryType;
use crate::ui::format_currency;

/// Galactic color palette
mod colors {
    use egui::Color32;
    
    pub const NEON_PINK: Color32 = Color32::from_rgb(255, 20, 147);
    pub const NEON_PURPLE: Color32 = Color32::from_rgb(148, 0, 211);
    pub const NEON_BLUE: Color32 = Color32::from_rgb(0, 191, 255);
    pub const NEON_CYAN: Color32 = Color32::from_rgb(0, 255, 255);
    pub const NEON_GREEN: Color32 = Color32::from_rgb(57, 255, 20);
    pub const TEXT_DIM: Color32 = Color32::from_rgb(120, 100, 140);
}

/// Render the right panel with the cost report
pub fn render(app: &mut JewelryCalculatorApp, ui: &mut Ui, ctx: &Context) {
    ui.heading(RichText::new("Cost Report").color(colors::NEON_CYAN));

    if !app.has_mesh() {
        ui.label(RichText::new("Load a model to generate report").color(colors::TEXT_DIM));
        return;
    }

    if app.report.is_none() {
        ui.label(RichText::new("No report generated yet").color(colors::TEXT_DIM));
        if ui.button(RichText::new("Generate Report").color(colors::NEON_BLUE)).clicked() {
            app.regenerate_report();
        }
        return;
    }

    // Action buttons
    render_action_buttons(app, ui, ctx);

    ui.separator();

    // Report summary
    render_report_summary(app, ui);

    ui.separator();

    // Cost breakdown - takes remaining height
    ui.label(RichText::new("Cost Breakdown").strong().color(colors::NEON_PINK));
    
    // Use all available vertical space
    let available_height = ui.available_height() - 10.0;
    render_cost_breakdown(app, ui, available_height);
}

/// Render action buttons (save)
fn render_action_buttons(app: &mut JewelryCalculatorApp, ui: &mut Ui, ctx: &Context) {
    ui.horizontal(|ui| {
        // Copy to clipboard
        if ui.button(RichText::new("📋 Copy JSON").color(colors::NEON_BLUE)).clicked() {
            ctx.copy_text(app.report_json.clone());
            app.set_status("Report copied to clipboard");
        }

        // Save to file
        if ui.button(RichText::new("💾 Save JSON...").color(colors::NEON_PURPLE)).clicked() {
            save_report_dialog(app, ctx);
        }

        // Regenerate
        if ui.button(RichText::new("🔄 Refresh").color(colors::NEON_CYAN)).clicked() {
            app.regenerate_report();
            app.set_status("Report regenerated");
        }
    });
}

/// Open save dialog for report
fn save_report_dialog(app: &mut JewelryCalculatorApp, ctx: &Context) {
    if let Some(bridge) = &app.async_bridge {
        let json = app.report_json.clone();
        let sender = bridge.sender.clone();
        let ctx = ctx.clone();

        // Generate default filename
        let default_name = app.loaded_file
            .as_ref()
            .and_then(|p| p.file_stem())
            .and_then(|n| n.to_str())
            .map(|n| format!("{}_cost_report.json", n))
            .unwrap_or_else(|| "cost_report.json".to_string());

        bridge.spawn(async move {
            let file = AsyncFileDialog::new()
                .add_filter("JSON", &["json"])
                .set_file_name(&default_name)
                .save_file()
                .await;

            if let Some(file) = file {
                let path = file.path().to_path_buf();
                let result = tokio::fs::write(&path, json)
                    .await
                    .map(|_| path.clone())
                    .map_err(|e| e.to_string());

                let _ = sender.send(crate::app_state::AsyncMessage::FileSaved(result));
                ctx.request_repaint();
            }
        });
    }
}

/// Render report summary section
fn render_report_summary(app: &JewelryCalculatorApp, ui: &mut Ui) {
    let report = match &app.report {
        Some(r) => r,
        None => return,
    };

    egui::Grid::new("summary_grid")
        .num_columns(2)
        .spacing([20.0, 4.0])
        .show(ui, |ui| {
            ui.label(RichText::new("File:").color(colors::TEXT_DIM));
            ui.label(&report.file_name);
            ui.end_row();

            ui.label(RichText::new("Type:").color(colors::TEXT_DIM));
            ui.label(RichText::new(app.jewelry_type.display_name()).color(colors::NEON_PINK));
            ui.end_row();

            ui.label(RichText::new("Base Volume:").color(colors::TEXT_DIM));
            ui.label(format!("{:.3} cm³", report.base_volume_cm3));
            ui.end_row();

            if report.jewelry_type == JewelryType::Ring {
                ui.label(RichText::new("Sizes:").color(colors::TEXT_DIM));
                ui.label(format!("{}", report.sizes.len()));
                ui.end_row();
            }

            ui.label(RichText::new("Prices:").color(colors::TEXT_DIM));
            if report.prices_are_live {
                ui.label(RichText::new("Live ☑").color(colors::NEON_GREEN));
            } else {
                ui.label(RichText::new("Cached/Default").color(Color32::YELLOW));
            }
            ui.end_row();
        });
}

/// Render cost breakdown section with full available height
fn render_cost_breakdown(app: &JewelryCalculatorApp, ui: &mut Ui, available_height: f32) {
    let report = match &app.report {
        Some(r) => r,
        None => return,
    };

    ScrollArea::vertical()
        .max_height(available_height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (idx, size_entry) in report.sizes.iter().enumerate() {
                // Size header
                let header = if report.jewelry_type == JewelryType::Ring {
                    RichText::new(format!(
                        "{} (📏 {:.1}mm, scale: {:.2}x)",
                        size_entry.ring_size,
                        size_entry.inner_diameter_mm,
                        size_entry.scale_factor
                    )).color(colors::NEON_PURPLE)
                } else {
                    RichText::new("Base Size").color(colors::NEON_PURPLE)
                };

                egui::CollapsingHeader::new(header)
                    .default_open(idx == 0 || report.sizes.len() == 1)
                    .show(ui, |ui| {
                        ui.label(RichText::new(format!("Volume: {:.3} cm³", size_entry.volume_cm3)).color(colors::TEXT_DIM));
                        
                        ui.add_space(4.0);

                        egui::Grid::new(format!("costs_grid_{}", idx))
                            .num_columns(3)
                            .spacing([15.0, 4.0])
                            .striped(true)
                            .show(ui, |ui| {
                                ui.label(RichText::new("Material").strong().color(colors::NEON_CYAN));
                                ui.label(RichText::new("Weight").strong().color(colors::NEON_CYAN));
                                ui.label(RichText::new("Cost").strong().color(colors::NEON_CYAN));
                                ui.end_row();

                                // Sort materials for consistent display
                                let mut materials: Vec<_> = size_entry.materials.iter().collect();
                                materials.sort_by_key(|(name, _)| {
                                    match name.as_str() {
                                        "gold_24k" => 0,
                                        "gold_22k" => 1,
                                        "gold_18k" => 2,
                                        "gold_14k" => 3,
                                        "gold_10k" => 4,
                                        "silver" => 5,
                                        "bronze" => 6,
                                        "wax" => 7,
                                        _ => 8,
                                    }
                                });

                                for (name, entry) in materials {
                                    let (display_name, color) = match name.as_str() {
                                        "gold_24k" => ("Gold 24K", Color32::from_rgb(255, 215, 0)),
                                        "gold_22k" => ("Gold 22K", Color32::from_rgb(238, 201, 0)),
                                        "gold_18k" => ("Gold 18K", Color32::from_rgb(218, 165, 32)),
                                        "gold_14k" => ("Gold 14K", Color32::from_rgb(207, 181, 59)),
                                        "gold_10k" => ("Gold 10K", Color32::from_rgb(184, 134, 11)),
                                        "silver" => ("Silver", Color32::from_rgb(192, 192, 192)),
                                        "bronze" => ("Bronze", Color32::from_rgb(205, 127, 50)),
                                        "wax" => ("Wax", Color32::from_rgb(139, 90, 43)),
                                        other => (other, colors::TEXT_DIM),
                                    };

                                    ui.label(RichText::new(display_name).color(color));
                                    ui.label(format!("{:.2}g", entry.weight_g));
                                    ui.label(RichText::new(format_currency(entry.price_usd)).color(colors::NEON_GREEN));
                                    ui.end_row();
                                }
                            });
                        
                        ui.add_space(8.0);
                    });
            }
        });
}
