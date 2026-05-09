//! GPU-accelerated mesh renderer using glow (OpenGL).
//!
//! Uploads mesh vertex/index data to GPU buffers once, then renders
//! with a single glDrawElements call per frame. Uses flat shading
//! (per-face normals) with diffuse lighting.

use std::sync::{Arc, Mutex};

use egui_glow::glow;
use glow::HasContext;

use crate::mesh::Mesh;

const VERTEX_SHADER: &str = r#"#version 330 core

layout(location = 0) in vec3 a_position;
layout(location = 1) in vec3 a_normal;

uniform mat4 u_mvp;
uniform mat3 u_normal_matrix;

out vec3 v_normal;

void main() {
    gl_Position = u_mvp * vec4(a_position, 1.0);
    v_normal = u_normal_matrix * a_normal;
}
"#;

const FRAGMENT_SHADER: &str = r#"#version 330 core

in vec3 v_normal;

uniform vec3 u_light_dir;
uniform vec3 u_base_color;
uniform float u_ambient;

out vec4 frag_color;

void main() {
    vec3 n = normalize(v_normal);
    float diff = max(dot(n, u_light_dir), 0.0);
    vec3 color = u_base_color * (u_ambient + (1.0 - u_ambient) * diff);
    frag_color = vec4(color, 1.0);
}
"#;

const WIREFRAME_FRAGMENT_SHADER: &str = r#"#version 330 core

uniform vec3 u_wire_color;

out vec4 frag_color;

void main() {
    frag_color = vec4(u_wire_color, 0.86);
}
"#;

struct GpuResources {
    program: glow::NativeProgram,
    wire_program: glow::NativeProgram,
    vao: glow::NativeVertexArray,
    vbo: glow::NativeBuffer,
    ebo: glow::NativeBuffer,
}

pub struct GpuMeshRenderer {
    resources: Option<GpuResources>,
    index_count: i32,
    pending_vertices: Option<Vec<f32>>,
    pending_indices: Option<Vec<u32>>,
}

// glow handles are u32 integers on native, safe to send across threads
unsafe impl Send for GpuMeshRenderer {}
unsafe impl Sync for GpuMeshRenderer {}

impl Default for GpuMeshRenderer {
    fn default() -> Self {
        Self {
            resources: None,
            index_count: 0,
            pending_vertices: None,
            pending_indices: None,
        }
    }
}

impl GpuMeshRenderer {
    pub fn new() -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self::default()))
    }

    /// Convert mesh data into flat vertex/index buffers for GPU upload.
    /// Flat shading: each triangle gets its own vertices with the face normal.
    pub fn prepare_upload(&mut self, mesh: &Mesh) {
        puffin::profile_function!();

        let face_count = mesh.faces.len();
        let mut vertices: Vec<f32> = Vec::with_capacity(face_count * 3 * 6);
        let mut indices: Vec<u32> = Vec::with_capacity(face_count * 3);
        let mut idx: u32 = 0;

        for face in &mesh.faces {
            if face.v.len() < 3 {
                continue;
            }

            let v0 = match mesh.vertices.get(face.v[0]) {
                Some(v) if v.is_finite() => v,
                _ => continue,
            };
            let v1 = match mesh.vertices.get(face.v[1]) {
                Some(v) if v.is_finite() => v,
                _ => continue,
            };
            let v2 = match mesh.vertices.get(face.v[2]) {
                Some(v) if v.is_finite() => v,
                _ => continue,
            };

            let e1 = [v1.0 - v0.0, v1.1 - v0.1, v1.2 - v0.2];
            let e2 = [v2.0 - v0.0, v2.1 - v0.1, v2.2 - v0.2];
            let nx = e1[1] * e2[2] - e1[2] * e2[1];
            let ny = e1[2] * e2[0] - e1[0] * e2[2];
            let nz = e1[0] * e2[1] - e1[1] * e2[0];
            let len = (nx * nx + ny * ny + nz * nz).sqrt();
            if len < 1e-10 {
                continue;
            }
            let (nx, ny, nz) = (nx / len, ny / len, nz / len);

            vertices.extend_from_slice(&[v0.0, v0.1, v0.2, nx, ny, nz]);
            vertices.extend_from_slice(&[v1.0, v1.1, v1.2, nx, ny, nz]);
            vertices.extend_from_slice(&[v2.0, v2.1, v2.2, nx, ny, nz]);

            indices.push(idx);
            indices.push(idx + 1);
            indices.push(idx + 2);
            idx += 3;
        }

        self.pending_vertices = Some(vertices);
        self.pending_indices = Some(indices);
    }

    /// Returns true if there is data waiting to be uploaded to the GPU.
    pub fn has_pending_upload(&self) -> bool {
        self.pending_vertices.is_some()
    }

    /// Render the mesh. Called from within the paint callback.
    ///
    /// `max_triangles` is the LOD limit from the detail slider.
    /// The actual draw count is computed internally after any pending upload.
    pub fn paint(
        &mut self,
        gl: &glow::Context,
        viewport: egui::PaintCallbackInfo,
        mvp: &[f32; 16],
        normal_matrix: &[f32; 9],
        light_dir: [f32; 3],
        base_color: [f32; 3],
        max_triangles: i32,
        wireframe: bool,
    ) {
        unsafe {
            self.ensure_resources(gl);
        }

        let res = match &self.resources {
            Some(r) => r,
            None => return,
        };

        // Upload pending data if any
        if let (Some(verts), Some(idxs)) =
            (self.pending_vertices.take(), self.pending_indices.take())
        {
            self.index_count = idxs.len() as i32;

            unsafe {
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(res.vbo));
                gl.buffer_data_u8_slice(
                    glow::ARRAY_BUFFER,
                    as_u8_slice(&verts),
                    glow::STATIC_DRAW,
                );

                gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(res.ebo));
                gl.buffer_data_u8_slice(
                    glow::ELEMENT_ARRAY_BUFFER,
                    as_u8_slice(&idxs),
                    glow::STATIC_DRAW,
                );

                gl.bind_buffer(glow::ARRAY_BUFFER, None);
                gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, None);
            }
        }

        if self.index_count == 0 {
            return;
        }

        // Compute draw count from the LOD slider AFTER upload so index_count is current
        let count = (max_triangles * 3).min(self.index_count);
        if count <= 0 {
            return;
        }

        unsafe {
            let vp = viewport.viewport_in_pixels();
            gl.viewport(vp.left_px, vp.from_bottom_px, vp.width_px, vp.height_px);
            gl.scissor(vp.left_px, vp.from_bottom_px, vp.width_px, vp.height_px);

            gl.enable(glow::DEPTH_TEST);
            gl.depth_func(glow::LESS);
            gl.enable(glow::CULL_FACE);
            gl.cull_face(glow::BACK);
            gl.enable(glow::SCISSOR_TEST);

            gl.clear(glow::DEPTH_BUFFER_BIT);

            // --- Solid pass ---
            gl.use_program(Some(res.program));
            gl.bind_vertex_array(Some(res.vao));

            let mvp_loc = gl.get_uniform_location(res.program, "u_mvp");
            gl.uniform_matrix_4_f32_slice(mvp_loc.as_ref(), false, mvp);

            let nm_loc = gl.get_uniform_location(res.program, "u_normal_matrix");
            gl.uniform_matrix_3_f32_slice(nm_loc.as_ref(), false, normal_matrix);

            let light_loc = gl.get_uniform_location(res.program, "u_light_dir");
            gl.uniform_3_f32(light_loc.as_ref(), light_dir[0], light_dir[1], light_dir[2]);

            let color_loc = gl.get_uniform_location(res.program, "u_base_color");
            gl.uniform_3_f32(
                color_loc.as_ref(),
                base_color[0],
                base_color[1],
                base_color[2],
            );

            let ambient_loc = gl.get_uniform_location(res.program, "u_ambient");
            gl.uniform_1_f32(ambient_loc.as_ref(), 0.3);

            gl.polygon_mode(glow::FRONT_AND_BACK, glow::FILL);
            gl.draw_elements(glow::TRIANGLES, count, glow::UNSIGNED_INT, 0);

            // --- Wireframe overlay pass ---
            if wireframe {
                gl.use_program(Some(res.wire_program));

                let wire_mvp_loc = gl.get_uniform_location(res.wire_program, "u_mvp");
                gl.uniform_matrix_4_f32_slice(wire_mvp_loc.as_ref(), false, mvp);

                let wire_color_loc =
                    gl.get_uniform_location(res.wire_program, "u_wire_color");
                gl.uniform_3_f32(wire_color_loc.as_ref(), 1.0, 0.39, 0.78);

                gl.enable(glow::BLEND);
                gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
                gl.enable(glow::POLYGON_OFFSET_LINE);
                gl.polygon_offset(-1.0, -1.0);
                gl.polygon_mode(glow::FRONT_AND_BACK, glow::LINE);
                gl.draw_elements(glow::TRIANGLES, count, glow::UNSIGNED_INT, 0);

                gl.disable(glow::POLYGON_OFFSET_LINE);
                gl.polygon_mode(glow::FRONT_AND_BACK, glow::FILL);
                gl.disable(glow::BLEND);
            }

            // Restore state for egui
            gl.bind_vertex_array(None);
            gl.use_program(None);
            gl.disable(glow::DEPTH_TEST);
            gl.disable(glow::CULL_FACE);
            gl.disable(glow::SCISSOR_TEST);
        }
    }

    /// Lazily create GL resources on first paint call.
    unsafe fn ensure_resources(&mut self, gl: &glow::Context) {
        if self.resources.is_some() {
            return;
        }

        let program = unsafe { compile_program(gl, VERTEX_SHADER, FRAGMENT_SHADER) };
        let wire_program =
            unsafe { compile_program(gl, VERTEX_SHADER, WIREFRAME_FRAGMENT_SHADER) };

        let vao = unsafe { gl.create_vertex_array() }.expect("create VAO");
        let vbo = unsafe { gl.create_buffer() }.expect("create VBO");
        let ebo = unsafe { gl.create_buffer() }.expect("create EBO");

        unsafe {
            gl.bind_vertex_array(Some(vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(ebo));

            let stride = 6 * std::mem::size_of::<f32>() as i32;

            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, stride, 0);

            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_f32(
                1,
                3,
                glow::FLOAT,
                false,
                stride,
                3 * std::mem::size_of::<f32>() as i32,
            );

            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, None);
        }

        self.resources = Some(GpuResources {
            program,
            wire_program,
            vao,
            vbo,
            ebo,
        });
    }

    /// Free GPU resources.
    pub fn destroy(&mut self, gl: &glow::Context) {
        if let Some(res) = self.resources.take() {
            unsafe {
                gl.delete_program(res.program);
                gl.delete_program(res.wire_program);
                gl.delete_vertex_array(res.vao);
                gl.delete_buffer(res.vbo);
                gl.delete_buffer(res.ebo);
            }
        }
        self.index_count = 0;
    }
}

unsafe fn compile_program(
    gl: &glow::Context,
    vert_src: &str,
    frag_src: &str,
) -> glow::NativeProgram {
    let program = unsafe { gl.create_program() }.expect("create program");

    let vert = unsafe { gl.create_shader(glow::VERTEX_SHADER) }.expect("create vert shader");
    unsafe {
        gl.shader_source(vert, vert_src);
        gl.compile_shader(vert);
    }
    if !unsafe { gl.get_shader_compile_status(vert) } {
        panic!(
            "Vertex shader error: {}",
            unsafe { gl.get_shader_info_log(vert) }
        );
    }

    let frag = unsafe { gl.create_shader(glow::FRAGMENT_SHADER) }.expect("create frag shader");
    unsafe {
        gl.shader_source(frag, frag_src);
        gl.compile_shader(frag);
    }
    if !unsafe { gl.get_shader_compile_status(frag) } {
        panic!(
            "Fragment shader error: {}",
            unsafe { gl.get_shader_info_log(frag) }
        );
    }

    unsafe {
        gl.attach_shader(program, vert);
        gl.attach_shader(program, frag);
        gl.link_program(program);
    }
    if !unsafe { gl.get_program_link_status(program) } {
        panic!(
            "Program link error: {}",
            unsafe { gl.get_program_info_log(program) }
        );
    }

    unsafe {
        gl.detach_shader(program, vert);
        gl.detach_shader(program, frag);
        gl.delete_shader(vert);
        gl.delete_shader(frag);
    }

    program
}

fn as_u8_slice<T: Copy>(data: &[T]) -> &[u8] {
    unsafe {
        std::slice::from_raw_parts(
            data.as_ptr() as *const u8,
            data.len() * std::mem::size_of::<T>(),
        )
    }
}
