//! Application state for the Jewelry Cost Calculator

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crossbeam_channel::{Receiver, Sender};
use tokio::runtime::Handle;

use crate::mesh::Mesh;
use crate::pricing::{MetalPrices, WaxPricing};
use crate::report::{CostReport, JewelryType};
use crate::ring_sizing::{DetectedRingHole, RingSize};
use crate::database::profiles::{WaxProfile, NewWaxProfile};
use crate::database::files::ExportFormat;
use crate::ui::gpu_mesh::GpuMeshRenderer;
use surrealdb_types::RecordId;

/// Messages sent from async tasks to the UI
#[derive(Debug)]
pub enum AsyncMessage {
    /// Metal prices fetched from API
    PricesLoaded(Result<MetalPrices, String>),
    /// Mesh loaded from file
    MeshLoaded {
        path: PathBuf,
        result: Result<LoadedMeshData, String>,
    },
    /// File save completed
    FileSaved(Result<PathBuf, String>),
    /// Database initialized
    DatabaseInitialized(Result<(), String>),
    /// Wax profiles loaded from database
    ProfilesLoaded(Result<Vec<WaxProfile>, String>),
    /// Profile saved (created or updated)
    ProfileSaved(Result<WaxProfile, String>),
    /// Profile deleted
    ProfileDeleted(Result<String, String>),
    /// Single ring export completed
    RingExported {
        size: f64,
        path: PathBuf,
        result: Result<(), String>,
    },
    /// Batch export completed
    BatchExportCompleted {
        count: usize,
        path: PathBuf,
        result: Result<(), String>,
    },
}

/// Data returned when a mesh is successfully loaded
#[derive(Debug)]
pub struct LoadedMeshData {
    pub mesh: Mesh,
    pub volume_mm3: f64,
    pub volume_cm3: f64,
    pub triangle_count: usize,
    pub detected_hole: Option<DetectedRingHole>,
}

/// Ring size selection mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingSizeMode {
    /// Single target size
    Single,
    /// Range of sizes
    Range,
}

impl Default for RingSizeMode {
    fn default() -> Self {
        RingSizeMode::Single
    }
}

/// Settings for ring sizing
#[derive(Debug, Clone)]
pub struct RingSizeSettings {
    pub mode: RingSizeMode,
    /// Current inner diameter of the ring (mm) - auto-detected or manual
    pub current_diameter_mm: f64,
    /// Whether the diameter was auto-detected
    pub diameter_auto_detected: bool,
    /// Single target size (for Single mode)
    pub target_size: f64,
    /// Range start (for Range mode)
    pub range_start: f64,
    /// Range end (for Range mode)
    pub range_end: f64,
}

impl Default for RingSizeSettings {
    fn default() -> Self {
        Self {
            mode: RingSizeMode::Single,
            current_diameter_mm: 17.3, // Size 7
            diameter_auto_detected: false,
            target_size: 7.0,
            range_start: 5.0,
            range_end: 10.0,
        }
    }
}

impl RingSizeSettings {
    /// Get the target sizes based on current mode
    pub fn target_sizes(&self) -> Vec<RingSize> {
        match self.mode {
            RingSizeMode::Single => vec![RingSize::new(self.target_size)],
            RingSizeMode::Range => RingSize::range(self.range_start, self.range_end),
        }
    }
}

/// Async communication bridge for tokio <-> egui
pub struct AsyncBridge {
    pub sender: Sender<AsyncMessage>,
    pub receiver: Receiver<AsyncMessage>,
    pub runtime: Handle,
}

impl AsyncBridge {
    pub fn new(runtime: Handle) -> Self {
        let (sender, receiver) = crossbeam_channel::unbounded();
        Self {
            sender,
            receiver,
            runtime,
        }
    }

    /// Spawn an async task
    pub fn spawn<F>(&self, future: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.runtime.spawn(future);
    }
}

/// 3D viewer state
pub struct ViewerState {
    /// Camera distance from model
    pub camera_distance: f32,
    /// Camera rotation around Y axis (yaw) in radians
    pub camera_yaw: f32,
    /// Camera rotation around X axis (pitch) in radians
    pub camera_pitch: f32,
    /// Whether the user is currently dragging to rotate
    pub is_rotating: bool,
    /// Show wireframe overlay
    pub show_wireframe: bool,
    /// Show grid floor
    pub show_grid: bool,
    /// Target triangle count for display (affects LOD)
    pub target_triangle_count: usize,
    /// GPU mesh renderer (shared with paint callback)
    pub gpu_renderer: Arc<Mutex<GpuMeshRenderer>>,
}

/// UI panel visibility state
#[derive(Debug, Clone)]
pub struct PanelState {
    /// Whether the log panel is visible
    pub show_log_panel: bool,
}

impl Default for ViewerState {
    fn default() -> Self {
        Self {
            camera_distance: 100.0,
            camera_yaw: 0.0,
            camera_pitch: 0.3,
            is_rotating: false,
            show_wireframe: false,
            show_grid: true,
            target_triangle_count: usize::MAX,
            gpu_renderer: GpuMeshRenderer::new(),
        }
    }
}

impl Default for PanelState {
    fn default() -> Self {
        Self {
            show_log_panel: false,
        }
    }
}

/// Main application state
pub struct JewelryCalculatorApp {
    pub first_run: bool,
    // === File & Mesh ===
    /// Path to the currently loaded file
    pub loaded_file: Option<PathBuf>,
    /// Loaded mesh data
    pub mesh: Option<Arc<Mesh>>,
    /// Mesh volume in mm³
    pub volume_mm3: f64,
    /// Mesh volume in cm³
    pub volume_cm3: f64,
    /// Triangle count
    pub triangle_count: usize,

    // === Jewelry Settings ===
    /// Type of jewelry (pendant or ring)
    pub jewelry_type: JewelryType,
    /// Ring sizing settings
    pub ring_settings: RingSizeSettings,
    /// Detected ring hole (if any)
    pub detected_hole: Option<DetectedRingHole>,

    // === Material Settings ===
    /// Custom wax density (g/cm³)
    pub wax_density: f64,
    /// Wax pricing
    pub wax_pricing: WaxPricing,
    /// Available wax/resin profiles from database
    pub wax_profiles: Vec<WaxProfile>,
    /// Currently selected profile index
    pub selected_profile_index: Option<usize>,
    /// Profile being edited (for add/edit dialog)
    pub editing_profile: Option<NewWaxProfile>,
    /// ID of profile being edited (None for new profiles)
    pub editing_profile_id: Option<RecordId>,
    /// Whether we're showing the profile editor
    pub show_profile_editor: bool,
    /// Whether database is initialized
    pub database_ready: bool,
    
    // === Export Settings ===
    /// Export format preference
    pub export_format: ExportFormat,
    /// Whether an export is in progress
    pub exporting: bool,

    // === Pricing ===
    /// Current metal prices
    pub metal_prices: MetalPrices,
    /// Whether prices are being fetched
    pub prices_loading: bool,
    /// Price fetch error message
    pub prices_error: Option<String>,

    // === Report ===
    /// Generated cost report
    pub report: Option<CostReport>,
    /// Report JSON string (cached for display)
    pub report_json: String,

    // === UI State ===
    /// 3D viewer state
    pub viewer_state: ViewerState,
    /// Panel visibility state
    pub panel_state: PanelState,
    /// Status message to display
    pub status_message: Option<(String, std::time::Instant)>,
    /// Whether mesh is currently loading
    pub mesh_loading: bool,

    // === Async ===
    /// Async communication bridge
    pub async_bridge: Option<AsyncBridge>,
}

impl Default for JewelryCalculatorApp {
    fn default() -> Self {
        Self {
            first_run: true,
            loaded_file: None,
            mesh: None,
            volume_mm3: 0.0,
            volume_cm3: 0.0,
            triangle_count: 0,

            jewelry_type: JewelryType::Pendant,
            ring_settings: RingSizeSettings::default(),
            detected_hole: None,

            wax_density: 1.08,
            wax_pricing: WaxPricing::default(),
            wax_profiles: Vec::new(),
            selected_profile_index: None,
            editing_profile: None,
            editing_profile_id: None,
            show_profile_editor: false,
            database_ready: false,
            
            export_format: ExportFormat::STL,
            exporting: false,

            metal_prices: MetalPrices::default(),
            prices_loading: false,
            prices_error: None,

            report: None,
            report_json: String::new(),

            viewer_state: ViewerState::default(),
            panel_state: PanelState::default(),
            status_message: None,
            mesh_loading: false,

            async_bridge: None,
        }
    }
}

impl JewelryCalculatorApp {
    /// Create a new app with async runtime
    pub fn new(runtime: Handle) -> Self {
        let mut app = Self::default();
        app.async_bridge = Some(AsyncBridge::new(runtime));
        app
    }

    /// Set a status message that will auto-clear after a few seconds
    pub fn set_status(&mut self, message: impl Into<String>) {
        self.status_message = Some((message.into(), std::time::Instant::now()));
    }

    /// Clear status message if it's old enough
    pub fn clear_old_status(&mut self) {
        if let Some((_, time)) = &self.status_message {
            if time.elapsed().as_secs() > 5 {
                self.status_message = None;
            }
        }
    }

    /// Process incoming async messages
    pub fn process_async_messages(&mut self) {
        // Collect all messages first to avoid borrow issues
        let messages: Vec<AsyncMessage> = self
            .async_bridge
            .as_ref()
            .map(|bridge| bridge.receiver.try_iter().collect())
            .unwrap_or_default();

        for msg in messages {
            match msg {
                AsyncMessage::PricesLoaded(result) => {
                    self.prices_loading = false;
                    match result {
                        Ok(prices) => {
                            self.metal_prices = prices;
                            self.prices_error = None;
                            self.set_status("Metal prices updated");
                            self.regenerate_report();
                        }
                        Err(e) => {
                            self.prices_error = Some(e.clone());
                            self.set_status(format!("Failed to fetch prices: {}", e));
                        }
                    }
                }
                AsyncMessage::MeshLoaded { path, result } => {
                    self.mesh_loading = false;
                    match result {
                        Ok(data) => {
                            self.loaded_file = Some(path.clone());
                            self.mesh = Some(Arc::new(data.mesh));
                            self.volume_mm3 = data.volume_mm3;
                            self.volume_cm3 = data.volume_cm3;
                            self.triangle_count = data.triangle_count;
                            self.detected_hole = data.detected_hole.clone();
                            
                            // Queue mesh data for GPU upload
                            if let Some(mesh) = &self.mesh {
                                self.viewer_state.gpu_renderer.lock().unwrap().prepare_upload(mesh);
                            }

                            // Update ring settings if hole was detected
                            if let Some(hole) = &data.detected_hole {
                                self.ring_settings.current_diameter_mm = hole.diameter_mm;
                                self.ring_settings.diameter_auto_detected = true;
                            }

                            let filename = path.file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("model");
                            self.set_status(format!("Loaded: {}", filename));
                            self.regenerate_report();
                        }
                        Err(e) => {
                            self.set_status(format!("Failed to load mesh: {}", e));
                        }
                    }
                }
                AsyncMessage::FileSaved(result) => {
                    match result {
                        Ok(path) => {
                            let filename = path.file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("file");
                            self.set_status(format!("Saved: {}", filename));
                        }
                        Err(e) => {
                            self.set_status(format!("Failed to save: {}", e));
                        }
                    }
                }
                AsyncMessage::DatabaseInitialized(result) => {
                    match result {
                        Ok(()) => {
                            self.database_ready = true;
                            self.set_status("Database initialized");
                            // Load profiles
                            self.load_profiles();
                        }
                        Err(e) => {
                            self.set_status(format!("Database error: {}", e));
                        }
                    }
                }
                AsyncMessage::ProfilesLoaded(result) => {
                    match result {
                        Ok(profiles) => {
                            self.wax_profiles = profiles;
                            // Auto-select first profile if available
                            if !self.wax_profiles.is_empty() && self.selected_profile_index.is_none() {
                                self.select_profile(0);
                            }
                        }
                        Err(e) => {
                            self.set_status(format!("Failed to load profiles: {}", e));
                        }
                    }
                }
                AsyncMessage::ProfileSaved(result) => {
                    match result {
                        Ok(profile) => {
                            self.set_status(format!("Profile saved: {}", profile.name));
                            self.show_profile_editor = false;
                            self.editing_profile = None;
                            self.editing_profile_id = None;
                            self.load_profiles();
                        }
                        Err(e) => {
                            self.set_status(format!("Failed to save profile: {}", e));
                        }
                    }
                }
                AsyncMessage::ProfileDeleted(result) => {
                    match result {
                        Ok(name) => {
                            self.set_status(format!("Profile deleted: {}", name));
                            self.selected_profile_index = None;
                            self.load_profiles();
                        }
                        Err(e) => {
                            self.set_status(format!("Failed to delete profile: {}", e));
                        }
                    }
                }
                AsyncMessage::RingExported { size, path, result } => {
                    self.exporting = false;
                    match result {
                        Ok(()) => {
                            let filename = path.file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("file");
                            self.set_status(format!("Exported size {}: {}", size, filename));
                        }
                        Err(e) => {
                            self.set_status(format!("Export failed: {}", e));
                        }
                    }
                }
                AsyncMessage::BatchExportCompleted { count, path, result } => {
                    self.exporting = false;
                    match result {
                        Ok(()) => {
                            self.set_status(format!("Exported {} sizes to {}", count, path.display()));
                        }
                        Err(e) => {
                            self.set_status(format!("Batch export failed: {}", e));
                        }
                    }
                }
            }
        }
    }

    /// Regenerate the cost report based on current settings
    pub fn regenerate_report(&mut self) {
        if self.volume_cm3 <= 0.0 {
            self.report = None;
            self.report_json = String::new();
            return;
        }

        let file_name = self
            .loaded_file
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        // Update wax pricing with current density
        self.wax_pricing.cost_per_gram = self.wax_density * 0.10; // Rough estimate

        let report = match self.jewelry_type {
            JewelryType::Pendant => {
                use crate::materials::calculate_all_weights;
                use crate::pricing::calculate_all_costs;

                let weights = calculate_all_weights(self.volume_cm3, Some(self.wax_density));
                let costs = calculate_all_costs(&weights, &self.metal_prices, &self.wax_pricing);

                CostReport::new_pendant(file_name, self.volume_cm3, &costs, &self.metal_prices)
            }
            JewelryType::Ring => {
                let sizes = self.ring_settings.target_sizes();
                CostReport::new_ring(
                    file_name,
                    self.volume_cm3,
                    self.ring_settings.current_diameter_mm,
                    &sizes,
                    &self.metal_prices,
                    &self.wax_pricing,
                )
            }
        };

        self.report_json = report.to_json();
        self.report = Some(report);
    }

    /// Fetch metal prices from API
    pub fn fetch_prices(&mut self, api_key: &str) {
        if self.prices_loading {
            return;
        }

        if let Some(bridge) = &self.async_bridge {
            self.prices_loading = true;
            let sender = bridge.sender.clone();
            let api_key = api_key.to_string();

            bridge.spawn(async move {
                let result = crate::pricing::api::fetch_metal_prices(&api_key).await;
                let msg = AsyncMessage::PricesLoaded(result.map_err(|e| e.to_string()));
                let _ = sender.send(msg);
            });
        }
    }

    /// Load a mesh file asynchronously
    pub fn load_mesh_file(&mut self, path: PathBuf) {
        if self.mesh_loading {
            return;
        }

        if let Some(bridge) = &self.async_bridge {
            self.mesh_loading = true;
            let sender = bridge.sender.clone();

            bridge.spawn(async move {
                let result = tokio::task::spawn_blocking(move || {
                    load_mesh_blocking(&path).map(|data| (path, data))
                })
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r);

                let msg = match result {
                    Ok((path, data)) => AsyncMessage::MeshLoaded {
                        path,
                        result: Ok(data),
                    },
                    Err(e) => AsyncMessage::MeshLoaded {
                        path: PathBuf::new(),
                        result: Err(e),
                    },
                };
                let _ = sender.send(msg);
            });
        }
    }

    /// Save report to file
    pub fn save_report(&mut self, path: PathBuf) {
        if let Some(bridge) = &self.async_bridge {
            let json = self.report_json.clone();
            let sender = bridge.sender.clone();

            bridge.spawn(async move {
                let result = tokio::fs::write(&path, json)
                    .await
                    .map(|_| path.clone())
                    .map_err(|e| e.to_string());

                let _ = sender.send(AsyncMessage::FileSaved(result));
            });
        }
    }

    /// Check if the app has a valid mesh loaded
    pub fn has_mesh(&self) -> bool {
        self.mesh.is_some() && self.volume_cm3 > 0.0
    }

    /// Attempt to repair non-manifold edges in the mesh
    /// Returns the number of faces removed, or None if no mesh is loaded
    pub fn repair_non_manifold_edges(&mut self) -> Option<usize> {
        let mesh = self.mesh.take()?;
        
        // Clone the mesh so we can mutate it
        let mut mesh_clone = (*mesh).clone();
        
        // Perform the repair
        let removed_count = mesh_clone.repair_non_manifold_edges();
        
        if removed_count > 0 {
            // Recalculate volume after repair
            let volume_mm3 = crate::mesh::volume::calculate_volume(&mesh_clone);
            let volume_cm3 = crate::mesh::volume::calculate_volume_cm3(&mesh_clone);
            let triangle_count = mesh_clone.triangle_count();
            
            // Re-initialize cache
            mesh_clone.init_cache();
            
            // Update app state
            self.volume_mm3 = volume_mm3;
            self.volume_cm3 = volume_cm3;
            self.triangle_count = triangle_count;
            self.mesh = Some(Arc::new(mesh_clone));
            
            // Re-upload repaired mesh to GPU
            if let Some(mesh) = &self.mesh {
                self.viewer_state.gpu_renderer.lock().unwrap().prepare_upload(mesh);
            }
            
            // Regenerate report with new volume
            self.regenerate_report();
            
            self.set_status(format!("Repaired mesh: removed {} faces", removed_count));
        } else {
            // No changes, put the original mesh back
            self.mesh = Some(mesh);
            self.set_status("No non-manifold edges found to repair");
        }
        
        Some(removed_count)
    }

    /// Check if the mesh has repairable non-manifold edges
    pub fn has_repairable_non_manifold(&self) -> bool {
        self.mesh.as_ref()
            .map(|m| m.has_non_manifold_edges())
            .unwrap_or(false)
    }
    
    /// Initialize the database
    pub fn init_database(&mut self) {
        if let Some(bridge) = &self.async_bridge {
            let sender = bridge.sender.clone();
            
            bridge.spawn(async move {
                let result = crate::database::init().await.map_err(|e| e.to_string());
                let _ = sender.send(AsyncMessage::DatabaseInitialized(result));
            });
        }
    }
    
    /// Load wax profiles from database
    pub fn load_profiles(&mut self) {
        if let Some(bridge) = &self.async_bridge {
            let sender = bridge.sender.clone();
            
            bridge.spawn(async move {
                let result = crate::database::profiles::get_all_profiles()
                    .await
                    .map_err(|e| e.to_string());
                let _ = sender.send(AsyncMessage::ProfilesLoaded(result));
            });
        }
    }
    
    /// Save a profile (create or update)
    pub fn save_profile(&mut self, profile: NewWaxProfile, id: Option<RecordId>) {
        if let Some(bridge) = &self.async_bridge {
            let sender = bridge.sender.clone();
            
            bridge.spawn(async move {
                let result = if let Some(id) = id {
                    crate::database::profiles::update_profile(&id, &profile).await
                } else {
                    crate::database::profiles::create_profile(&profile).await
                };
                let _ = sender.send(AsyncMessage::ProfileSaved(result.map_err(|e| e.to_string())));
            });
        }
    }
    
    /// Delete a profile
    pub fn delete_profile(&mut self, profile: &WaxProfile) {
        if let Some(bridge) = &self.async_bridge {
            let sender = bridge.sender.clone();
            let id = profile.id.clone();
            let name = profile.name.clone();
            
            bridge.spawn(async move {
                let result = crate::database::profiles::delete_profile(&id)
                    .await
                    .map(|_| name.clone())
                    .map_err(|e| e.to_string());
                let _ = sender.send(AsyncMessage::ProfileDeleted(result));
            });
        }
    }
    
    /// Select a profile by index
    pub fn select_profile(&mut self, index: usize) {
        if index < self.wax_profiles.len() {
            self.selected_profile_index = Some(index);
            let profile = &self.wax_profiles[index];
            self.wax_density = profile.density;
            self.wax_pricing.cost_per_gram = profile.price_per_gram;
            self.regenerate_report();
        }
    }
    
    /// Export a single ring size
    pub fn export_ring(&mut self, size: RingSize, dest_dir: PathBuf) {
        if self.exporting || self.mesh.is_none() {
            return;
        }
        
        if let Some(bridge) = &self.async_bridge {
            self.exporting = true;
            let sender = bridge.sender.clone();
            let mesh = self.mesh.clone().unwrap();
            let current_diameter = self.ring_settings.current_diameter_mm;
            let format = self.export_format;
            let base_name = self.loaded_file
                .as_ref()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("ring")
                .to_string();
            
            bridge.spawn(async move {
                let result = export_ring_async(
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
                
                let _ = sender.send(AsyncMessage::RingExported {
                    size: size.0,
                    path,
                    result: result.map_err(|e| e.to_string()),
                });
            });
        }
    }
    
    /// Export all ring sizes in the current range
    pub fn export_all_rings(&mut self, dest_dir: PathBuf) {
        if self.exporting || self.mesh.is_none() {
            return;
        }
        
        if let Some(bridge) = &self.async_bridge {
            self.exporting = true;
            let sender = bridge.sender.clone();
            let mesh = self.mesh.clone().unwrap();
            let current_diameter = self.ring_settings.current_diameter_mm;
            let sizes = self.ring_settings.target_sizes();
            let format = self.export_format;
            let base_name = self.loaded_file
                .as_ref()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("ring")
                .to_string();
            let count = sizes.len();
            let dest = dest_dir.clone();
            
            bridge.spawn(async move {
                let result = export_all_rings_async(
                    &mesh,
                    current_diameter,
                    &sizes,
                    format,
                    &base_name,
                    &dest_dir,
                ).await;
                
                let _ = sender.send(AsyncMessage::BatchExportCompleted {
                    count,
                    path: dest,
                    result: result.map_err(|e| e.to_string()),
                });
            });
        }
    }
}

/// Load mesh from file (blocking)
fn load_mesh_blocking(path: &PathBuf) -> Result<LoadedMeshData, String> {
    use crate::mesh::{load_mesh, volume::calculate_volume_cm3};
    use crate::ring_sizing::detect_ring_hole;
    use log::{info, warn};

    let filename = path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");
    
    info!("Loading mesh: {}", filename);

    let mesh = load_mesh(path).map_err(|e| e.to_string())?;

    info!("Mesh parsed: {} vertices, {} faces", mesh.vertices.len(), mesh.faces.len());

    // Check for potential issues
    let mut invalid_vertices = 0;
    for v in &mesh.vertices {
        if !v.is_finite() {
            invalid_vertices += 1;
        }
    }
    if invalid_vertices > 0 {
        warn!("Found {} vertices with non-finite coordinates (NaN/Inf)", invalid_vertices);
    }

    // Check face validity
    let mut invalid_faces = 0;
    let mut degenerate_faces = 0;
    for face in &mesh.faces {
        if face.v.len() < 3 {
            invalid_faces += 1;
            continue;
        }
        // Check for out-of-bounds indices
        for &idx in &face.v {
            if idx >= mesh.vertices.len() {
                invalid_faces += 1;
                break;
            }
        }
        // Check for degenerate triangles (duplicate vertices)
        if face.v.len() >= 3 {
            let has_dupe = face.v[0] == face.v[1] || face.v[1] == face.v[2] || face.v[0] == face.v[2];
            if has_dupe {
                degenerate_faces += 1;
            }
        }
    }
    if invalid_faces > 0 {
        warn!("Found {} faces with invalid structure (too few vertices or out-of-bounds indices)", invalid_faces);
    }
    if degenerate_faces > 0 {
        warn!("Found {} degenerate faces (duplicate vertex indices)", degenerate_faces);
    }

    // Pre-compute edge topology metrics (warm the cache off the UI thread)
    mesh.warm_cache();
    if mesh.is_watertight() {
        info!("Mesh is watertight (no boundary edges detected)");
    } else {
        warn!("Mesh has {} non-manifold edges", mesh.non_manifold_edge_count());
    }

    let volume_mm3 = crate::mesh::volume::calculate_volume(&mesh);
    let volume_cm3 = calculate_volume_cm3(&mesh);
    let triangle_count = mesh.triangle_count();
    
    info!("Volume: {:.3} cm³ ({:.1} mm³)", volume_cm3, volume_mm3);
    info!("Triangle count: {}", triangle_count);

    let detected_hole = detect_ring_hole(&mesh);
    if let Some(ref hole) = detected_hole {
        info!("Detected ring hole: 📏 {:.2}mm at ({:.1}, {:.1}, {:.1}), confidence: {:.0}%",
            hole.diameter_mm, hole.center[0], hole.center[1], hole.center[2], hole.confidence * 100.0);
    } else {
        info!("No ring hole detected");
    }

    Ok(LoadedMeshData {
        mesh,
        volume_mm3,
        volume_cm3,
        triangle_count,
        detected_hole,
    })
}

/// Export a single ring size asynchronously
pub async fn export_ring_async(
    mesh: &Arc<Mesh>,
    current_diameter: f64,
    size: RingSize,
    format: ExportFormat,
    base_name: &str,
    dest_dir: &PathBuf,
) -> anyhow::Result<()> {
    use crate::mesh::export::export_scaled_mesh;
    use crate::database::files::{generate_export_filename, cache_export};
    
    let target_diameter = size.inner_diameter_mm();
    let scale_factor = target_diameter / current_diameter;
    
    let mesh_ref = mesh.as_ref();
    let data = tokio::task::spawn_blocking({
        let mesh = mesh_ref.clone();
        move || export_scaled_mesh(&mesh, scale_factor, format)
    })
    .await??;
    
    let filename = generate_export_filename(base_name, size.0, format);
    let file_path = dest_dir.join(&filename);
    
    // Write to destination
    tokio::fs::write(&file_path, &data).await?;
    
    // Also cache in database
    cache_export(base_name, size.0, scale_factor, format, &data).await?;
    
    Ok(())
}

/// Export all ring sizes asynchronously
pub async fn export_all_rings_async(
    mesh: &Arc<Mesh>,
    current_diameter: f64,
    sizes: &[RingSize],
    format: ExportFormat,
    base_name: &str,
    dest_dir: &PathBuf,
) -> anyhow::Result<()> {
    use crate::mesh::export::export_scaled_mesh;
    use crate::database::files::{generate_export_filename, cache_export};
    
    // Ensure destination directory exists
    tokio::fs::create_dir_all(dest_dir).await?;
    
    for size in sizes {
        let target_diameter = size.inner_diameter_mm();
        let scale_factor = target_diameter / current_diameter;
        
        let mesh_ref = mesh.as_ref();
        let data = tokio::task::spawn_blocking({
            let mesh = mesh_ref.clone();
            move || export_scaled_mesh(&mesh, scale_factor, format)
        })
        .await??;
        
        let filename = generate_export_filename(base_name, size.0, format);
        let file_path = dest_dir.join(&filename);
        
        // Write to destination
        tokio::fs::write(&file_path, &data).await?;
        
        // Also cache in database
        cache_export(base_name, size.0, scale_factor, format, &data).await?;
    }
    
    Ok(())
}
