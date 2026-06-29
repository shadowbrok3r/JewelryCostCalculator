//! Left panel UI - Options and settings

use egui::{Context, Ui, RichText, Color32};
use rfd::AsyncFileDialog;

use crate::app_state::{JewelryCalculatorApp, RingSizeMode};
use crate::report::JewelryType;
use crate::ring_sizing::RingSize;
use crate::database::profiles::NewWaxProfile;
use crate::database::files::ExportFormat;
use crate::METAL_API_KEY;

/// Render the left panel
pub fn render(app: &mut JewelryCalculatorApp, ui: &mut Ui, ctx: &Context) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;

        // File section
        render_file_section(app, ui, ctx);

        ui.add_space(8.0);
        ui.separator();

        // Mesh info section
        if app.has_mesh() {
            render_mesh_info_section(app, ui);
            ui.add_space(8.0);
            ui.separator();
        }

        // Jewelry type section
        render_jewelry_type_section(app, ui);

        ui.add_space(8.0);
        ui.separator();

        // Ring sizing section (only for rings)
        if app.jewelry_type == JewelryType::Ring {
            render_ring_section(app, ui, ctx);
            ui.add_space(8.0);
            ui.separator();
        }

        // Material settings section
        render_material_section(app, ui);

        ui.add_space(8.0);
        ui.separator();

        // Pricing section
        render_pricing_section(app, ui);
    });
}

/// File loading section
fn render_file_section(app: &mut JewelryCalculatorApp, ui: &mut Ui, ctx: &Context) {
    ui.heading("File");

    ui.horizontal(|ui| {
        if ui.button("Open STL/OBJ...").clicked() {
            open_file_dialog(app, ctx);
        }

        if app.mesh_loading {
            ui.spinner();
        }
    });

    // Show loaded file name
    if let Some(path) = &app.loaded_file {
        let filename = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        ui.label(format!("Loaded: {}", filename));
    }

    // Drag and drop hint
    ui.label(RichText::new("(or drag & drop a file)").small().weak());

    // Handle dropped files
    handle_dropped_files(app, ctx);
}

/// Open file dialog
fn open_file_dialog(app: &mut JewelryCalculatorApp, ctx: &Context) {
    if let Some(bridge) = &app.async_bridge {
        let sender = bridge.sender.clone();
        let ctx = ctx.clone();

        bridge.spawn(async move {
            let file = AsyncFileDialog::new()
                .add_filter("3D Models", &["stl", "obj", "STL", "OBJ"])
                .pick_file()
                .await;

            if let Some(file) = file {
                let path = file.path().to_path_buf();
                ctx.request_repaint();
                
                let _ = sender.send(crate::app_state::AsyncMessage::MeshLoaded {
                    path: path.clone(),
                    result: tokio::task::spawn_blocking(move || {
                        crate::mesh::load_mesh(&path)
                            .map(|mesh| {
                                use crate::mesh::volume::{calculate_volume, calculate_volume_cm3};
                                use crate::ring_sizing::measure_inner_diameter;

                                let volume_mm3 = calculate_volume(&mesh);
                                let volume_cm3 = calculate_volume_cm3(&mesh);
                                let triangle_count = mesh.triangle_count();
                                mesh.warm_cache();
                                let detected_hole = measure_inner_diameter(&mesh)
                                    .filter(|b| b.coverage >= 0.5)
                                    .map(|b| b.to_detected_hole());

                                crate::app_state::LoadedMeshData {
                                    mesh,
                                    volume_mm3,
                                    volume_cm3,
                                    triangle_count,
                                    detected_hole,
                                }
                            })
                            .map_err(|e| e.to_string())
                    })
                    .await
                    .map_err(|e| e.to_string())
                    .and_then(|r| r),
                });
            }
        });
    }
}

/// Handle drag-and-drop files
fn handle_dropped_files(app: &mut JewelryCalculatorApp, ctx: &Context) {
    ctx.input(|i| {
        for file in &i.raw.dropped_files {
            if let Some(path) = &file.path {
                let ext = path.extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_lowercase());
                
                if matches!(ext.as_deref(), Some("stl") | Some("obj")) {
                    app.load_mesh_file(path.clone());
                }
            }
        }
    });
}

/// Mesh information section
fn render_mesh_info_section(app: &JewelryCalculatorApp, ui: &mut Ui) {
    ui.heading("Mesh Info");

    egui::Grid::new("mesh_info_grid")
        .num_columns(2)
        .spacing([20.0, 4.0])
        .show(ui, |ui| {
            ui.label("Volume:");
            ui.label(format!("{:.3} cm³", app.volume_cm3));
            ui.end_row();

            ui.label("Volume (mm³):");
            ui.label(format!("{:.1} mm³", app.volume_mm3));
            ui.end_row();

            ui.label("Triangles:");
            ui.label(format!("{}", app.triangle_count));
            ui.end_row();
        });

    // Ring hole detection info
    if let Some(hole) = &app.detected_hole {
        ui.add_space(4.0);
        ui.label(RichText::new("Ring Hole Detected:").strong());
        ui.label(format!("  Diameter: {:.2} mm", hole.diameter_mm));
        ui.label(format!("  Confidence: {:.0}%", hole.confidence * 100.0));
    }
}

/// Jewelry type selection
fn render_jewelry_type_section(app: &mut JewelryCalculatorApp, ui: &mut Ui) {
    ui.heading("Jewelry Type");

    ui.horizontal(|ui| {
        if ui.selectable_label(app.jewelry_type == JewelryType::Pendant, "Pendant").clicked() {
            app.jewelry_type = JewelryType::Pendant;
            app.regenerate_report();
        }
        if ui.selectable_label(app.jewelry_type == JewelryType::Ring, "Ring").clicked() {
            app.jewelry_type = JewelryType::Ring;
            app.regenerate_report();
        }
    });
}

/// Ring sizing section with export buttons
fn render_ring_section(app: &mut JewelryCalculatorApp, ui: &mut Ui, ctx: &Context) {
    ui.heading("Ring Sizing");

    // Current diameter
    ui.horizontal(|ui| {
        ui.label("Current diameter:");
        let mut diameter = app.ring_settings.current_diameter_mm;
        if ui.add(egui::DragValue::new(&mut diameter)
            .speed(0.1)
            .range(10.0..=30.0)
            .suffix(" mm"))
            .changed()
        {
            app.ring_settings.current_diameter_mm = diameter;
            app.ring_settings.diameter_auto_detected = false;
            app.regenerate_report();
        }
    });

    if app.ring_settings.diameter_auto_detected {
        ui.label(RichText::new("(auto-detected)").small().weak());
    }

    // Approximate current size
    let current_size = RingSize::from_diameter_mm(app.ring_settings.current_diameter_mm);
    ui.label(format!("≈ {}", current_size.display()));

    ui.add_space(8.0);

    // Size mode
    ui.label("Target size mode:");
    ui.horizontal(|ui| {
        if ui.selectable_label(app.ring_settings.mode == RingSizeMode::Single, "Single").clicked() {
            app.ring_settings.mode = RingSizeMode::Single;
            app.regenerate_report();
        }
        if ui.selectable_label(app.ring_settings.mode == RingSizeMode::Range, "Range").clicked() {
            app.ring_settings.mode = RingSizeMode::Range;
            app.regenerate_report();
        }
    });

    ui.add_space(4.0);

    match app.ring_settings.mode {
        RingSizeMode::Single => {
            ui.horizontal(|ui| {
                ui.label("Target size:");
                let mut size = app.ring_settings.target_size;
                if ui.add(egui::DragValue::new(&mut size)
                    .speed(0.5)
                    .range(3.0..=15.0))
                    .changed()
                {
                    // Round to nearest 0.5
                    app.ring_settings.target_size = (size * 2.0).round() / 2.0;
                    app.regenerate_report();
                }
            });
        }
        RingSizeMode::Range => {
            ui.horizontal(|ui| {
                ui.label("From:");
                let mut start = app.ring_settings.range_start;
                if ui.add(egui::DragValue::new(&mut start)
                    .speed(0.5)
                    .range(3.0..=15.0))
                    .changed()
                {
                    app.ring_settings.range_start = (start * 2.0).round() / 2.0;
                    app.regenerate_report();
                }

                ui.label("To:");
                let mut end = app.ring_settings.range_end;
                if ui.add(egui::DragValue::new(&mut end)
                    .speed(0.5)
                    .range(3.0..=15.0))
                    .changed()
                {
                    app.ring_settings.range_end = (end * 2.0).round() / 2.0;
                    app.regenerate_report();
                }
            });

            let count = ((app.ring_settings.range_end - app.ring_settings.range_start) * 2.0) as i32 + 1;
            ui.label(RichText::new(format!("({} sizes)", count.max(0))).small().weak());
        }
    }

    // Export section (only if mesh is loaded)
    if app.has_mesh() {
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);
        
        ui.label(RichText::new("Export").strong());
        
        // Format selection
        ui.horizontal(|ui| {
            ui.label("Format:");
            if ui.selectable_label(app.export_format == ExportFormat::STL, "STL").clicked() {
                app.export_format = ExportFormat::STL;
            }
            if ui.selectable_label(app.export_format == ExportFormat::OBJ, "OBJ").clicked() {
                app.export_format = ExportFormat::OBJ;
            }
        });
        
        ui.add_space(4.0);
        
        // Export buttons
        ui.horizontal(|ui| {
            let sizes = app.ring_settings.target_sizes();
            
            // Single size export (use first/only size)
            let single_enabled = !app.exporting && sizes.len() == 1;
            if ui.add_enabled(single_enabled, egui::Button::new("Export Size")).clicked() {
                export_single_ring(app, ctx, sizes[0]);
            }
            
            // Export all button
            let batch_enabled = !app.exporting && sizes.len() > 1;
            if ui.add_enabled(batch_enabled, egui::Button::new(format!("Export All ({})", sizes.len()))).clicked() {
                export_all_rings(app, ctx);
            }
            
            if app.exporting {
                ui.spinner();
            }
        });
    }
}

/// Export a single ring size
fn export_single_ring(app: &mut JewelryCalculatorApp, ctx: &Context, size: RingSize) {
    if let Some(bridge) = &app.async_bridge {
        let ctx = ctx.clone();
        let sender = bridge.sender.clone();
        let mesh = app.mesh.clone();
        let current_diameter = app.ring_settings.current_diameter_mm;
        let format = app.export_format;
        let base_name = app.loaded_file
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("ring")
            .to_string();
        
        bridge.spawn(async move {
            let folder = AsyncFileDialog::new()
                .set_title("Select Export Folder")
                .pick_folder()
                .await;
            
            if let Some(folder) = folder {
                let dest_dir = folder.path().to_path_buf();
                ctx.request_repaint();
                
                if let Some(mesh) = mesh {
                    let result = crate::app_state::export_ring_async(
                        &mesh,
                        current_diameter,
                        size,
                        format,
                        &base_name,
                        &dest_dir,
                    ).await;
                    
                    let path = dest_dir.join(crate::database::files::generate_export_filename(
                        &base_name, size.0, format
                    ));
                    
                    let _ = sender.send(crate::app_state::AsyncMessage::RingExported {
                        size: size.0,
                        path,
                        result: result.map_err(|e| e.to_string()),
                    });
                }
            }
        });
    }
}

/// Export all ring sizes
fn export_all_rings(app: &mut JewelryCalculatorApp, ctx: &Context) {
    if let Some(bridge) = &app.async_bridge {
        let ctx = ctx.clone();
        let sender = bridge.sender.clone();
        let mesh = app.mesh.clone();
        let current_diameter = app.ring_settings.current_diameter_mm;
        let sizes = app.ring_settings.target_sizes();
        let format = app.export_format;
        let base_name = app.loaded_file
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("ring")
            .to_string();
        let count = sizes.len();
        
        bridge.spawn(async move {
            let folder = AsyncFileDialog::new()
                .set_title("Select Export Folder")
                .pick_folder()
                .await;
            
            if let Some(folder) = folder {
                let dest_dir = folder.path().to_path_buf();
                ctx.request_repaint();
                
                if let Some(mesh) = mesh {
                    let result = crate::app_state::export_all_rings_async(
                        &mesh,
                        current_diameter,
                        &sizes,
                        format,
                        &base_name,
                        &dest_dir,
                    ).await;
                    
                    let _ = sender.send(crate::app_state::AsyncMessage::BatchExportCompleted {
                        count,
                        path: dest_dir,
                        result: result.map_err(|e| e.to_string()),
                    });
                }
            }
        });
    }
}

/// Material settings section with profile management
fn render_material_section(app: &mut JewelryCalculatorApp, ui: &mut Ui) {
    ui.heading("Wax/Resin Profiles");
    
    // Profile dropdown
    let mut selected_index: Option<usize> = None;
    
    if !app.wax_profiles.is_empty() {
        let selected_name = app.selected_profile_index
            .and_then(|i| app.wax_profiles.get(i))
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "Select profile...".to_string());
        
        // Collect profile info before dropdown to avoid borrow issues
        let profiles: Vec<(usize, String, bool)> = app.wax_profiles
            .iter()
            .enumerate()
            .map(|(i, p)| (i, p.name.clone(), app.selected_profile_index == Some(i)))
            .collect();
        
        egui::ComboBox::from_id_salt("wax_profile_select")
            .selected_text(selected_name)
            .show_ui(ui, |ui| {
                for (i, name, is_selected) in &profiles {
                    if ui.selectable_label(*is_selected, name).clicked() {
                        selected_index = Some(*i);
                    }
                }
            });
    } else if app.database_ready {
        ui.label(RichText::new("No profiles found").weak());
    } else {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("Loading...");
        });
    }
    
    // Apply selection after dropdown
    if let Some(i) = selected_index {
        app.select_profile(i);
    }
    
    // Profile action buttons
    let edit_enabled = app.selected_profile_index.is_some();
    let profile_to_edit = app.selected_profile_index
        .and_then(|i| app.wax_profiles.get(i))
        .cloned();
    let profile_to_delete = profile_to_edit.clone();
    
    ui.horizontal(|ui| {
        // New profile button
        if ui.button("➕ New").clicked() {
            app.editing_profile = Some(NewWaxProfile::default());
            app.editing_profile_id = None;
            app.show_profile_editor = true;
        }
        
        // Edit button (only if a profile is selected)
        if ui.add_enabled(edit_enabled, egui::Button::new("✏ Edit")).clicked() {
            if let Some(profile) = profile_to_edit {
                // Convert WaxProfile to NewWaxProfile for editing
                app.editing_profile = Some(NewWaxProfile::from(&profile));
                app.editing_profile_id = Some(profile.id.clone());
                app.show_profile_editor = true;
            }
        }
        
        // Delete button
        if ui.add_enabled(edit_enabled, egui::Button::new("🗑 Delete")).clicked() {
            if let Some(profile) = profile_to_delete {
                app.delete_profile(&profile);
            }
        }
    });
    
    ui.add_space(4.0);
    
    // Current values (editable directly)
    ui.horizontal(|ui| {
        ui.label("Density:");
        let mut density = app.wax_density;
        if ui.add(egui::DragValue::new(&mut density)
            .speed(0.01)
            .range(0.5..=2.0)
            .suffix(" g/cm³"))
            .changed()
        {
            app.wax_density = density;
            app.regenerate_report();
        }
    });

    ui.horizontal(|ui| {
        ui.label("Cost:");
        let mut cost = app.wax_pricing.cost_per_gram;
        if ui.add(egui::DragValue::new(&mut cost)
            .speed(0.01)
            .range(0.01..=10.0)
            .prefix("$")
            .suffix("/g"))
            .changed()
        {
            app.wax_pricing.cost_per_gram = cost;
            app.regenerate_report();
        }
    });
    
    // Profile editor window
    render_profile_editor(app, ui.ctx());
}

/// Render the profile editor window
fn render_profile_editor(app: &mut JewelryCalculatorApp, ctx: &Context) {
    if !app.show_profile_editor {
        return;
    }
    
    let is_new = app.editing_profile_id.is_none();
    let title = if is_new { "New Profile" } else { "Edit Profile" };
    
    // Track what action to take
    let mut should_save = false;
    let mut should_cancel = false;
    
    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .default_width(300.0)
        .show(ctx, |ui| {
            if let Some(ref mut profile) = app.editing_profile {
                ui.horizontal(|ui| {
                    ui.label("Name:");
                    ui.text_edit_singleline(&mut profile.name);
                });
                
                ui.horizontal(|ui| {
                    ui.label("Density:");
                    ui.add(egui::DragValue::new(&mut profile.density)
                        .speed(0.01)
                        .range(0.5..=2.0)
                        .suffix(" g/cm³"));
                });
                
                ui.horizontal(|ui| {
                    ui.label("Price:");
                    ui.add(egui::DragValue::new(&mut profile.price_per_gram)
                        .speed(0.01)
                        .range(0.01..=10.0)
                        .prefix("$")
                        .suffix("/g"));
                });
                
                // Description (optional)
                ui.horizontal(|ui| {
                    ui.label("Description:");
                });
                let desc = profile.description.get_or_insert_with(String::new);
                ui.text_edit_multiline(desc);
                if desc.is_empty() {
                    profile.description = None;
                }
                
                ui.add_space(8.0);
                
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        should_save = true;
                    }
                    
                    if ui.button("Cancel").clicked() {
                        should_cancel = true;
                    }
                });
            }
        });
    
    // Apply actions outside the window closure
    if should_save {
        if let Some(profile) = app.editing_profile.clone() {
            let id = app.editing_profile_id.clone();
            app.save_profile(profile, id);
        }
    }
    
    if should_cancel {
        app.show_profile_editor = false;
        app.editing_profile = None;
        app.editing_profile_id = None;
    }
}

/// Pricing section
fn render_pricing_section(app: &mut JewelryCalculatorApp, ui: &mut Ui) {
    ui.heading("Metal Prices");

    // Fetch button
    ui.horizontal(|ui| {
        let button_text = if app.prices_loading { "Fetching..." } else { "Refresh Prices" };
        if ui.button(button_text).clicked() && !app.prices_loading {
            app.fetch_prices(METAL_API_KEY);
        }

        if app.metal_prices.is_live {
            ui.label(RichText::new("Live").color(Color32::GREEN));
        }
    });

    // Error display
    if let Some(error) = &app.prices_error {
        ui.label(RichText::new(error).color(ui.style().visuals.error_fg_color).small());
    }

    ui.add_space(4.0);

    // Price display
    egui::Grid::new("prices_grid")
        .num_columns(2)
        .spacing([20.0, 4.0])
        .show(ui, |ui| {
            ui.label("Gold (24K):");
            ui.label(format!("${:.2}/oz", app.metal_prices.gold_per_troy_oz));
            ui.end_row();

            ui.label("Silver:");
            ui.label(format!("${:.2}/oz", app.metal_prices.silver_per_troy_oz));
            ui.end_row();

            ui.label("Bronze:");
            ui.label(format!("${:.2}/kg", app.metal_prices.bronze_per_kg));
            ui.end_row();
        });

    ui.label(RichText::new(format!("Updated: {}", app.metal_prices.age_string())).small().weak());
}
