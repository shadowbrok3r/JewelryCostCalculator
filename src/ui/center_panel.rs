//! Center panel UI - 3D model viewer with galactic neon aesthetic
//!
//! Uses GPU-accelerated rendering via glow (OpenGL) for mesh display,
//! with egui overlays for bounding box, info text, and annotations.

use std::sync::Arc;

use egui::{Context, Ui, Color32, Rect, Pos2, Stroke, Vec2, RichText};

use crate::app_state::JewelryCalculatorApp;
use crate::ring_sizing::DetectedRingHole;

mod colors {
    use egui::Color32;

    pub const BACKGROUND: Color32 = Color32::from_rgb(8, 8, 12);
    pub const NEON_PINK: Color32 = Color32::from_rgb(255, 20, 147);
    pub const NEON_PURPLE: Color32 = Color32::from_rgb(148, 0, 211);
    pub const NEON_BLUE: Color32 = Color32::from_rgb(0, 191, 255);
    pub const NEON_CYAN: Color32 = Color32::from_rgb(0, 255, 255);
    pub const NEON_MAGENTA: Color32 = Color32::from_rgb(255, 0, 255);
    pub const NEON_VIOLET: Color32 = Color32::from_rgb(33, 51, 252);
    pub const NEON_MINT_GREEN: Color32 = Color32::from_rgb(33, 252, 190);
    pub const TEXT_DIM: Color32 = Color32::from_rgb(120, 100, 140);

    pub fn grid_secondary() -> Color32 {
        Color32::from_rgba_unmultiplied(75, 0, 130, 40)
    }

    pub fn bounding_box() -> Color32 {
        Color32::from_rgba_unmultiplied(0, 191, 255, 100)
    }
}

/// Render the center panel with 3D viewer
pub fn render(app: &mut JewelryCalculatorApp, ui: &mut Ui, ctx: &Context) {
    puffin::profile_function!();

    let available_size = ui.available_size();

    if !app.has_mesh() {
        render_empty_state(ui, available_size);
        return;
    }

    ui.vertical(|ui| {
        render_viewer_controls(app, ui);
        ui.separator();

        let preview_rect = ui.available_rect_before_wrap();
        render_3d_preview(app, ui, preview_rect, ctx);
    });
}

fn render_empty_state(ui: &mut Ui, size: Vec2) {
    ui.vertical_centered(|ui| {
        ui.add_space(size.y / 3.0);
        ui.label(RichText::new("No Model Loaded").size(24.0).color(colors::NEON_PURPLE));
        ui.add_space(20.0);
        ui.label(
            RichText::new("Open an STL or OBJ file to view it here").color(colors::TEXT_DIM),
        );
        ui.label(RichText::new("Drag and drop supported").small().color(colors::TEXT_DIM));
    });
}

fn render_viewer_controls(app: &mut JewelryCalculatorApp, ui: &mut Ui) {
    puffin::profile_function!();
    ui.horizontal(|ui| {
        ui.label(RichText::new("View Controls:").color(colors::NEON_CYAN));

        let wireframe_label = if app.viewer_state.show_wireframe {
            RichText::new("Wireframe").color(colors::NEON_PINK)
        } else {
            RichText::new("Wireframe").color(colors::TEXT_DIM)
        };
        if ui
            .selectable_label(app.viewer_state.show_wireframe, wireframe_label)
            .clicked()
        {
            app.viewer_state.show_wireframe = !app.viewer_state.show_wireframe;
        }

        let grid_label = if app.viewer_state.show_grid {
            RichText::new("Grid").color(colors::NEON_PINK)
        } else {
            RichText::new("Grid").color(colors::TEXT_DIM)
        };
        if ui
            .selectable_label(app.viewer_state.show_grid, grid_label)
            .clicked()
        {
            app.viewer_state.show_grid = !app.viewer_state.show_grid;
        }

        ui.separator();

        if ui
            .button(RichText::new("Reset View").color(colors::NEON_BLUE))
            .clicked()
        {
            app.viewer_state.camera_distance = 100.0;
            app.viewer_state.camera_yaw = 0.0;
            app.viewer_state.camera_pitch = 0.3;
        }

        ui.label(RichText::new("Zoom:").color(colors::TEXT_DIM));
        ui.add(
            egui::Slider::new(&mut app.viewer_state.camera_distance, 10.0..=500.0)
                .logarithmic(true)
                .show_value(false),
        );

        ui.separator();

        let max_triangles = app
            .mesh
            .as_ref()
            .map(|m| m.triangle_count())
            .unwrap_or(200000)
            .max(5000);

        ui.label(RichText::new("Detail:").color(colors::TEXT_DIM));

        if app.viewer_state.target_triangle_count > max_triangles {
            app.viewer_state.target_triangle_count = max_triangles;
        }

        ui.add(
            egui::Slider::new(
                &mut app.viewer_state.target_triangle_count,
                5000..=max_triangles,
            )
            .logarithmic(true)
            .suffix(" tris")
            .clamping(egui::SliderClamping::Always),
        );

        if app.has_repairable_non_manifold() {
            ui.separator();

            let repair_btn =
                ui.button(RichText::new("Fix Non-Manifold").color(colors::NEON_MINT_GREEN));

            if repair_btn
                .on_hover_text(
                    "Remove faces connected to non-manifold edges (edges shared by >2 faces)",
                )
                .clicked()
            {
                app.repair_non_manifold_edges();
            }
        }
    });
}

// ---------------------------------------------------------------------------
// 3D preview with GPU mesh rendering
// ---------------------------------------------------------------------------

fn render_3d_preview(app: &mut JewelryCalculatorApp, ui: &mut Ui, rect: Rect, ctx: &Context) {
    puffin::profile_function!();
    let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());

    if response.dragged() {
        let delta = response.drag_delta();
        app.viewer_state.camera_yaw -= delta.x * 0.01;
        app.viewer_state.camera_pitch += delta.y * 0.01;
        app.viewer_state.camera_pitch = app.viewer_state.camera_pitch.clamp(-1.5, 1.5);
    }

    if response.hovered() {
        ctx.input(|i| {
            let scroll = i.smooth_scroll_delta.y;
            if scroll != 0.0 {
                app.viewer_state.camera_distance *= 1.0 - scroll * 0.001;
                app.viewer_state.camera_distance =
                    app.viewer_state.camera_distance.clamp(10.0, 500.0);
            }
        });
    }

    let painter = ui.painter_at(rect);

    // Background
    painter.rect_filled(rect, 0.0, colors::BACKGROUND);

    if app.viewer_state.show_grid {
        draw_grid(&painter, rect);
    }

    // GPU mesh rendering via PaintCallback
    if app.mesh.is_some() {
        draw_mesh_gpu(app, &painter, rect);
    }

    // Egui-based overlays (bounding box, hole, center point) drawn on top
    if let Some(mesh) = app.mesh.clone() {
        let detected_hole = app.detected_hole.clone();
        draw_overlays(&painter, rect, &mesh, app, &detected_hole);
    }

    draw_info_overlay(&painter, rect, app);

    painter.rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0, colors::NEON_PURPLE),
        egui::StrokeKind::Outside,
    );
}

/// Issue a GPU draw call for the mesh via egui PaintCallback.
fn draw_mesh_gpu(app: &JewelryCalculatorApp, painter: &egui::Painter, rect: Rect) {
    puffin::profile_function!();

    let mesh = match &app.mesh {
        Some(m) => m,
        None => return,
    };

    let (min_v, max_v) = match mesh.bounds() {
        Ok(b) => b,
        Err(_) => return,
    };

    let mesh_size = [
        (max_v.0 - min_v.0) as f64,
        (max_v.1 - min_v.1) as f64,
        (max_v.2 - min_v.2) as f64,
    ];
    let max_dim = mesh_size.iter().cloned().fold(0.0_f64, f64::max);
    if max_dim == 0.0 {
        return;
    }

    let viewport_size = rect.width().min(rect.height()) * 0.7;
    let scale = viewport_size as f64 / max_dim / app.viewer_state.camera_distance as f64 * 100.0;

    let yaw = app.viewer_state.camera_yaw as f64;
    let pitch = app.viewer_state.camera_pitch as f64;

    let cx = (min_v.0 as f64 + max_v.0 as f64) / 2.0;
    let cy = (min_v.1 as f64 + max_v.1 as f64) / 2.0;
    let cz = (min_v.2 as f64 + max_v.2 as f64) / 2.0;

    // Build MVP matrix matching the CPU overlay projection.
    // CPU Ry uses x1 = x*cos - z*sin, so negate sin_y to match.
    let (cos_y, sin_y) = (yaw.cos(), yaw.sin());
    let (cos_p, sin_p) = (pitch.cos(), pitch.sin());

    // V = Rx(pitch) * Ry(-yaw_sign):
    //   cos_y          0        -sin_y
    //  -sin_p*sin_y    cos_p    -sin_p*cos_y
    //   cos_p*sin_y    sin_p     cos_p*cos_y
    let v00 = cos_y;
    let v01 = 0.0;
    let v02 = -sin_y;
    let v10 = -sin_p * sin_y;
    let v11 = cos_p;
    let v12 = -sin_p * cos_y;
    let v20 = cos_p * sin_y;
    let v21 = sin_p;
    let v22 = cos_p * cos_y;

    // Normal matrix = upper-left 3x3 of view (rotation only, no scaling issues)
    let normal_matrix: [f32; 9] = [
        v00 as f32, v10 as f32, v20 as f32, // column 0
        v01 as f32, v11 as f32, v21 as f32, // column 1
        v02 as f32, v12 as f32, v22 as f32, // column 2
    ];

    // Orthographic half-extents in world units
    let half_w = rect.width() as f64 / (2.0 * scale);
    let half_h = rect.height() as f64 / (2.0 * scale);
    let far = max_dim * 2.0;

    // MVP = Projection * View * Model  (column-major for OpenGL)
    // Model = translate(-cx, -cy, -cz)
    // Projection = ortho(-half_w, half_w, -half_h, half_h, -far, far)
    //
    // Ortho:  1/half_w   0         0        0
    //         0          1/half_h  0        0
    //         0          0        -1/far    0
    //         0          0         0        1
    //
    // PV = P * V  (rotation columns scaled by projection)
    let sx = 1.0 / half_w;
    let sy = 1.0 / half_h;
    let sz = -1.0 / far;

    // Translation part: V * (-cx, -cy, -cz)
    let tx = v00 * (-cx) + v01 * (-cy) + v02 * (-cz);
    let ty = v10 * (-cx) + v11 * (-cy) + v12 * (-cz);
    let tz = v20 * (-cx) + v21 * (-cy) + v22 * (-cz);

    #[rustfmt::skip]
    let mvp: [f32; 16] = [
        (sx * v00) as f32, (sy * v10) as f32, (sz * v20) as f32, 0.0,
        (sx * v01) as f32, (sy * v11) as f32, (sz * v21) as f32, 0.0,
        (sx * v02) as f32, (sy * v12) as f32, (sz * v22) as f32, 0.0,
        (sx * tx)  as f32, (sy * ty)  as f32, (sz * tz)  as f32, 1.0,
    ];

    // Light direction in view space (upper-left-front)
    let light_dir: [f32; 3] = {
        let l = [0.3_f32, 0.6, 0.7];
        let len = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt();
        [l[0] / len, l[1] / len, l[2] / len]
    };

    // Neon purple base color (matches original: rgb(200, 50, 160) / 255)
    let base_color: [f32; 3] = [200.0 / 255.0, 50.0 / 255.0, 160.0 / 255.0];

    let max_triangles = app.viewer_state.target_triangle_count as i32;
    let wireframe = app.viewer_state.show_wireframe;

    let gpu = app.viewer_state.gpu_renderer.clone();

    let callback = egui_glow::CallbackFn::new(move |info, glow_painter| {
        let gl = glow_painter.gl();
        if let Ok(mut renderer) = gpu.lock() {
            renderer.paint(
                gl,
                info,
                &mvp,
                &normal_matrix,
                light_dir,
                base_color,
                max_triangles,
                wireframe,
            );
        }
    });

    painter.add(egui::PaintCallback {
        rect,
        callback: Arc::new(callback),
    });
}

// ---------------------------------------------------------------------------
// Egui-based overlays (bounding box, detected hole, center point)
// ---------------------------------------------------------------------------

/// Shared projection helper for CPU overlays (bounding box, hole, center dot).
struct CpuProjection {
    center: Pos2,
    scale: f64,
    cos_yaw: f64,
    sin_yaw: f64,
    cos_pitch: f64,
    sin_pitch: f64,
    cx: f64,
    cy: f64,
    cz: f64,
}

impl CpuProjection {
    fn from_app(app: &JewelryCalculatorApp, rect: Rect) -> Option<Self> {
        let mesh = app.mesh.as_ref()?;
        let (min_v, max_v) = mesh.bounds().ok()?;

        let mesh_size = [
            (max_v.0 - min_v.0) as f64,
            (max_v.1 - min_v.1) as f64,
            (max_v.2 - min_v.2) as f64,
        ];
        let max_dim = mesh_size.iter().cloned().fold(0.0_f64, f64::max);
        if max_dim == 0.0 {
            return None;
        }

        let viewport_size = rect.width().min(rect.height()) * 0.7;
        let scale =
            viewport_size as f64 / max_dim / app.viewer_state.camera_distance as f64 * 100.0;

        let yaw = app.viewer_state.camera_yaw as f64;
        let pitch = app.viewer_state.camera_pitch as f64;

        Some(Self {
            center: rect.center(),
            scale,
            cos_yaw: yaw.cos(),
            sin_yaw: yaw.sin(),
            cos_pitch: pitch.cos(),
            sin_pitch: pitch.sin(),
            cx: (min_v.0 as f64 + max_v.0 as f64) / 2.0,
            cy: (min_v.1 as f64 + max_v.1 as f64) / 2.0,
            cz: (min_v.2 as f64 + max_v.2 as f64) / 2.0,
        })
    }

    fn project(&self, x: f64, y: f64, z: f64) -> Pos2 {
        let x = x - self.cx;
        let y = y - self.cy;
        let z = z - self.cz;
        let x1 = x * self.cos_yaw - z * self.sin_yaw;
        let z1 = x * self.sin_yaw + z * self.cos_yaw;
        let y1 = y * self.cos_pitch - z1 * self.sin_pitch;
        let px = self.center.x + (x1 * self.scale) as f32;
        let py = self.center.y - (y1 * self.scale) as f32;
        Pos2::new(px, py)
    }

    fn project_with_depth(&self, x: f64, y: f64, z: f64) -> (Pos2, f64) {
        let x = x - self.cx;
        let y = y - self.cy;
        let z = z - self.cz;
        let x1 = x * self.cos_yaw - z * self.sin_yaw;
        let z1 = x * self.sin_yaw + z * self.cos_yaw;
        let y1 = y * self.cos_pitch - z1 * self.sin_pitch;
        let z2 = y * self.sin_pitch + z1 * self.cos_pitch;
        let px = self.center.x + (x1 * self.scale) as f32;
        let py = self.center.y - (y1 * self.scale) as f32;
        (Pos2::new(px, py), z2)
    }
}

fn draw_overlays(
    painter: &egui::Painter,
    rect: Rect,
    mesh: &crate::mesh::Mesh,
    app: &JewelryCalculatorApp,
    detected_hole: &Option<DetectedRingHole>,
) {
    puffin::profile_function!();
    let proj = match CpuProjection::from_app(app, rect) {
        Some(p) => p,
        None => return,
    };

    // Bounding box
    if let Ok((min_v, max_v)) = mesh.bounds() {
        let corners = [
            (min_v.0 as f64, min_v.1 as f64, min_v.2 as f64),
            (max_v.0 as f64, min_v.1 as f64, min_v.2 as f64),
            (max_v.0 as f64, max_v.1 as f64, min_v.2 as f64),
            (min_v.0 as f64, max_v.1 as f64, min_v.2 as f64),
            (min_v.0 as f64, min_v.1 as f64, max_v.2 as f64),
            (max_v.0 as f64, min_v.1 as f64, max_v.2 as f64),
            (max_v.0 as f64, max_v.1 as f64, max_v.2 as f64),
            (min_v.0 as f64, max_v.1 as f64, max_v.2 as f64),
        ];
        let projected: Vec<Pos2> = corners
            .iter()
            .map(|(x, y, z)| proj.project(*x, *y, *z))
            .collect();
        let bbox_stroke = Stroke::new(1.0, colors::bounding_box());

        let edges = [
            (0, 1),
            (1, 2),
            (2, 3),
            (3, 0),
            (4, 5),
            (5, 6),
            (6, 7),
            (7, 4),
            (0, 4),
            (1, 5),
            (2, 6),
            (3, 7),
        ];
        for (a, b) in edges {
            painter.line_segment([projected[a], projected[b]], bbox_stroke);
        }
    }

    // Detected hole
    if let Some(hole) = detected_hole {
        draw_detected_hole_3d(painter, &proj, hole);
    }

    // Center point
    let mesh_center = proj.project(proj.cx, proj.cy, proj.cz);
    painter.circle_filled(
        mesh_center,
        6.0,
        Color32::from_rgba_unmultiplied(255, 0, 255, 60),
    );
    painter.circle_filled(mesh_center, 4.0, colors::NEON_MAGENTA);
}

// ---------------------------------------------------------------------------
// Grid
// ---------------------------------------------------------------------------

fn draw_grid(painter: &egui::Painter, rect: Rect) {
    puffin::profile_function!();
    let grid_spacing = 40.0;
    let center = rect.center();

    let mut x = center.x;
    while x < rect.right() {
        let alpha = ((x - center.x) / rect.width() * 2.0).abs();
        let color = Color32::from_rgba_unmultiplied(138, 43, 226, (25.0 * (1.0 - alpha)) as u8);
        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            Stroke::new(1.0, color),
        );
        x += grid_spacing;
    }
    x = center.x - grid_spacing;
    while x > rect.left() {
        let alpha = ((x - center.x) / rect.width() * 2.0).abs();
        let color = Color32::from_rgba_unmultiplied(138, 43, 226, (25.0 * (1.0 - alpha)) as u8);
        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            Stroke::new(1.0, color),
        );
        x -= grid_spacing;
    }

    let mut y = center.y;
    while y < rect.bottom() {
        let alpha = ((y - center.y) / rect.height() * 2.0).abs();
        let color = Color32::from_rgba_unmultiplied(138, 43, 226, (25.0 * (1.0 - alpha)) as u8);
        painter.line_segment(
            [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
            Stroke::new(1.0, color),
        );
        y += grid_spacing;
    }
    y = center.y - grid_spacing;
    while y > rect.top() {
        let alpha = ((y - center.y) / rect.height() * 2.0).abs();
        let color = Color32::from_rgba_unmultiplied(138, 43, 226, (25.0 * (1.0 - alpha)) as u8);
        painter.line_segment(
            [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
            Stroke::new(1.0, color),
        );
        y -= grid_spacing;
    }

    painter.line_segment(
        [
            Pos2::new(rect.left(), center.y),
            Pos2::new(rect.right(), center.y),
        ],
        Stroke::new(1.5, colors::grid_secondary()),
    );
    painter.line_segment(
        [
            Pos2::new(center.x, rect.top()),
            Pos2::new(center.x, rect.bottom()),
        ],
        Stroke::new(1.5, colors::grid_secondary()),
    );
}

// ---------------------------------------------------------------------------
// Detected hole overlay
// ---------------------------------------------------------------------------

fn draw_detected_hole_3d(
    painter: &egui::Painter,
    proj: &CpuProjection,
    hole: &DetectedRingHole,
) {
    let radius = hole.diameter_mm / 2.0;
    let hc = hole.center;

    let (hole_center_2d, _) = proj.project_with_depth(hc[0], hc[1], hc[2]);

    painter.circle_filled(
        hole_center_2d,
        8.0,
        Color32::from_rgba_unmultiplied(57, 255, 20, 80),
    );
    painter.circle_filled(hole_center_2d, 5.0, colors::NEON_VIOLET);

    let num_segments = 32;
    let circle_color = Color32::from_rgba_unmultiplied(33, 252, 190, 200);
    let circle_stroke = Stroke::new(2.5, circle_color);

    // Orient the circle in the plane perpendicular to the detected axis direction
    // (a non-cardinal axis would render wrong if we only keyed off hole.axis).
    let n = {
        let d = hole.axis_direction;
        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if l > 1e-9 {
            [d[0] / l, d[1] / l, d[2] / l]
        } else {
            match hole.axis {
                0 => [1.0, 0.0, 0.0],
                1 => [0.0, 1.0, 0.0],
                _ => [0.0, 0.0, 1.0],
            }
        }
    };
    let e = {
        let (ax, ay, az) = (n[0].abs(), n[1].abs(), n[2].abs());
        if ax <= ay && ax <= az {
            [1.0, 0.0, 0.0]
        } else if ay <= az {
            [0.0, 1.0, 0.0]
        } else {
            [0.0, 0.0, 1.0]
        }
    };
    let perp1 = {
        let c = [
            n[1] * e[2] - n[2] * e[1],
            n[2] * e[0] - n[0] * e[2],
            n[0] * e[1] - n[1] * e[0],
        ];
        let l = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
        [c[0] / l, c[1] / l, c[2] / l]
    };
    let perp2: [f64; 3] = [
        n[1] * perp1[2] - n[2] * perp1[1],
        n[2] * perp1[0] - n[0] * perp1[2],
        n[0] * perp1[1] - n[1] * perp1[0],
    ];

    let mut circle_points: Vec<Pos2> = Vec::with_capacity(num_segments + 1);
    for i in 0..=num_segments {
        let angle = (i as f64) * 2.0 * std::f64::consts::PI / (num_segments as f64);
        let cos_a = angle.cos();
        let sin_a = angle.sin();

        let point_3d = [
            hc[0] + radius * (perp1[0] * cos_a + perp2[0] * sin_a),
            hc[1] + radius * (perp1[1] * cos_a + perp2[1] * sin_a),
            hc[2] + radius * (perp1[2] * cos_a + perp2[2] * sin_a),
        ];

        let (point_2d, _) = proj.project_with_depth(point_3d[0], point_3d[1], point_3d[2]);
        circle_points.push(point_2d);
    }

    for i in 0..num_segments {
        painter.line_segment([circle_points[i], circle_points[i + 1]], circle_stroke);
    }

    let p1_3d = [
        hc[0] + radius * perp1[0],
        hc[1] + radius * perp1[1],
        hc[2] + radius * perp1[2],
    ];
    let p2_3d = [
        hc[0] - radius * perp1[0],
        hc[1] - radius * perp1[1],
        hc[2] - radius * perp1[2],
    ];

    let (p1_2d, _) = proj.project_with_depth(p1_3d[0], p1_3d[1], p1_3d[2]);
    let (p2_2d, _) = proj.project_with_depth(p2_3d[0], p2_3d[1], p2_3d[2]);

    painter.line_segment([p1_2d, p2_2d], Stroke::new(1.5, colors::NEON_CYAN));

    let cap_size = 15.0;
    let dx = (p2_2d.x - p1_2d.x) / (p1_2d.distance(p2_2d)).max(1.0);
    let dy = (p2_2d.y - p1_2d.y) / (p1_2d.distance(p2_2d)).max(1.0);

    painter.line_segment(
        [
            Pos2::new(p1_2d.x - dy * cap_size, p1_2d.y + dx * cap_size),
            Pos2::new(p1_2d.x + dy * cap_size, p1_2d.y - dx * cap_size),
        ],
        Stroke::new(1.5, colors::NEON_VIOLET),
    );
    painter.line_segment(
        [
            Pos2::new(p2_2d.x - dy * cap_size, p2_2d.y + dx * cap_size),
            Pos2::new(p2_2d.x + dy * cap_size, p2_2d.y - dx * cap_size),
        ],
        Stroke::new(1.5, colors::NEON_VIOLET),
    );

    let label_offset = (radius * proj.scale) as f32 + 20.0;
    let label_pos = Pos2::new(
        hole_center_2d.x,
        hole_center_2d.y - label_offset.min(60.0),
    );

    let label = format!("\u{1f4d0} {:.2}mm", hole.diameter_mm);
    painter.text(
        label_pos,
        egui::Align2::CENTER_BOTTOM,
        label,
        egui::FontId::proportional(16.0),
        colors::NEON_CYAN,
    );

    let axis_name = match hole.axis {
        0 => "X-axis",
        1 => "Y-axis",
        _ => "Z-axis",
    };
    let confidence_label = format!("{} \u{2022} {:.0}% conf", axis_name, hole.confidence * 100.0);
    painter.text(
        Pos2::new(
            hole_center_2d.x,
            hole_center_2d.y + label_offset.min(60.0),
        ),
        egui::Align2::CENTER_TOP,
        confidence_label,
        egui::FontId::proportional(11.0),
        colors::TEXT_DIM,
    );
}

// ---------------------------------------------------------------------------
// Info overlay
// ---------------------------------------------------------------------------

fn draw_info_overlay(painter: &egui::Painter, rect: Rect, app: &JewelryCalculatorApp) {
    puffin::profile_function!();
    let font = egui::FontId::monospace(12.0);
    let mut y_offset = 10.0;

    if let Some(mesh) = &app.mesh {
        if let Ok((min_v, max_v)) = mesh.bounds() {
            let dims_text = format!(
                "Dimensions: {:.1} \u{00d7} {:.1} \u{00d7} {:.1} mm",
                max_v.0 - min_v.0,
                max_v.1 - min_v.1,
                max_v.2 - min_v.2
            );
            painter.text(
                Pos2::new(rect.left() + 10.0, rect.top() + y_offset),
                egui::Align2::LEFT_TOP,
                dims_text,
                font.clone(),
                colors::NEON_CYAN,
            );
            y_offset += 18.0;

            let total_tris = mesh.triangle_count();
            let target_tris = app.viewer_state.target_triangle_count;
            let rendered = total_tris.min(target_tris);
            let tri_text = if rendered < total_tris {
                format!("Triangles: {} (showing ~{})", total_tris, rendered)
            } else {
                format!("Triangles: {}", total_tris)
            };
            painter.text(
                Pos2::new(rect.left() + 10.0, rect.top() + y_offset),
                egui::Align2::LEFT_TOP,
                tri_text,
                font.clone(),
                colors::TEXT_DIM,
            );
            y_offset += 18.0;

            if mesh.is_watertight() {
                painter.text(
                    Pos2::new(rect.left() + 10.0, rect.top() + y_offset),
                    egui::Align2::LEFT_TOP,
                    "Watertight \u{2611}",
                    font.clone(),
                    colors::NEON_VIOLET,
                );
            } else {
                let non_manifold = mesh.non_manifold_edge_count();
                painter.text(
                    Pos2::new(rect.left() + 10.0, rect.top() + y_offset),
                    egui::Align2::LEFT_TOP,
                    format!("Non-manifold edges: {}", non_manifold),
                    font.clone(),
                    colors::NEON_MINT_GREEN,
                );
            }
            y_offset += 18.0;
        }
    }

    if let Some(hole) = &app.detected_hole {
        let axis_name = match hole.axis {
            0 => "X",
            1 => "Y",
            _ => "Z",
        };
        let hole_text = format!(
            "Detected hole: \u{1f4d0} {:.2}mm ({}-axis)",
            hole.diameter_mm, axis_name
        );
        painter.text(
            Pos2::new(rect.left() + 10.0, rect.top() + y_offset),
            egui::Align2::LEFT_TOP,
            hole_text,
            font.clone(),
            colors::NEON_VIOLET,
        );
    }

    let camera_text = format!(
        "Yaw: {:.0}\u{00b0} Pitch: {:.0}\u{00b0}",
        app.viewer_state.camera_yaw.to_degrees(),
        app.viewer_state.camera_pitch.to_degrees()
    );
    painter.text(
        Pos2::new(rect.right() - 10.0, rect.top() + 10.0),
        egui::Align2::RIGHT_TOP,
        camera_text,
        font.clone(),
        colors::TEXT_DIM,
    );

    let hint_text = "Drag to rotate \u{2022} Scroll to zoom";
    painter.text(
        Pos2::new(rect.center().x, rect.bottom() - 10.0),
        egui::Align2::CENTER_BOTTOM,
        hint_text,
        font,
        colors::NEON_PURPLE,
    );
}
