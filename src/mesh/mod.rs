pub mod stl;
pub mod obj;
pub mod volume;
pub mod export;

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use nalgebra::Vector3;
use parking_lot::RwLock;
use rayon::prelude::*;
use smallvec::SmallVec;

/// Maximum number of triangles allowed in a mesh
pub const MAX_TRIANGLES: u32 = 10_000_000;

/// Maximum triangles to render at full detail
pub const MAX_FULL_DETAIL_TRIANGLES: usize = 15000;

/// Cached mesh metadata to avoid recalculating every frame
#[derive(Debug, Clone)]
pub struct MeshCache {
    /// Cached bounding box (min, max)
    pub bounds: Option<(Vec3, Vec3)>,
    /// Cached triangle count
    pub triangle_count: usize,
    /// Cached watertight status
    pub is_watertight: Option<bool>,
    /// Cached non-manifold edge count
    pub non_manifold_edges: Option<usize>,
    /// Pre-computed LOD face indices for different LOD levels
    /// Key is LOD step (1 = full, 2 = half, etc.)
    pub lod_face_indices: HashMap<usize, Vec<usize>>,
}

/// Supported 3D file formats
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    STL,
    OBJ,
}

impl Format {
    /// Detect format from file extension
    pub fn from_extension(path: &Path) -> Option<Self> {
        path.extension()
            .and_then(|ext| ext.to_str())
            .and_then(|ext| match ext.to_lowercase().as_str() {
                "stl" => Some(Format::STL),
                "obj" => Some(Format::OBJ),
                _ => None,
            })
    }

    /// Detect format from file content (magic bytes)
    pub fn from_magic_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.is_empty() {
            return None;
        }

        // Binary STL detection
        if bytes.len() >= 84 {
            let triangle_count = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]);
            if triangle_count > 0
                && triangle_count <= MAX_TRIANGLES
                && bytes.len() >= 84 + (triangle_count as usize * 50)
            {
                return Some(Format::STL);
            }
        }

        // ASCII STL detection
        if bytes.len() >= 5 && &bytes[..5] == b"solid" {
            let preview = &bytes[..bytes.len().min(4096)];
            if let Ok(content) = std::str::from_utf8(preview) {
                if content.contains("facet") && content.contains("vertex") {
                    return Some(Format::STL);
                }
            }
        }

        // OBJ detection
        let preview = &bytes[..bytes.len().min(4096)];
        if let Ok(content) = std::str::from_utf8(preview) {
            let has_obj_markers = content
                .lines()
                .filter(|line| !line.trim().is_empty())
                .take(50)
                .any(|line| {
                    let line = line.trim_start();
                    line.starts_with("v ")
                        || line.starts_with("vt ")
                        || line.starts_with("vn ")
                        || line.starts_with("f ")
                        || line.starts_with("o ")
                        || line.starts_with("g ")
                });
            if has_obj_markers {
                return Some(Format::OBJ);
            }
        }

        None
    }
}

/// 3D vector type
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3(pub f32, pub f32, pub f32);

impl Vec3 {
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Self(x, y, z)
    }

    pub fn subtract(self, other: Vec3) -> Vec3 {
        Vec3(self.0 - other.0, self.1 - other.1, self.2 - other.2)
    }

    pub fn cross(self, other: Vec3) -> Vec3 {
        Vec3(
            self.1 * other.2 - self.2 * other.1,
            self.2 * other.0 - self.0 * other.2,
            self.0 * other.1 - self.1 * other.0,
        )
    }

    pub fn dot(self, other: Vec3) -> f32 {
        self.0 * other.0 + self.1 * other.1 + self.2 * other.2
    }

    pub fn length(self) -> f32 {
        (self.0 * self.0 + self.1 * self.1 + self.2 * self.2).sqrt()
    }

    pub fn normalize(self) -> Vec3 {
        let len = self.length();
        if len > 0.0 {
            Vec3(self.0 / len, self.1 / len, self.2 / len)
        } else {
            Vec3(0.0, 0.0, 0.0)
        }
    }

    pub fn is_finite(self) -> bool {
        self.0.is_finite() && self.1.is_finite() && self.2.is_finite()
    }
}

impl From<[f32; 3]> for Vec3 {
    fn from(arr: [f32; 3]) -> Self {
        Vec3(arr[0], arr[1], arr[2])
    }
}

impl From<Vec3> for Vector3<f64> {
    fn from(v: Vec3) -> Self {
        Vector3::new(v.0 as f64, v.1 as f64, v.2 as f64)
    }
}

impl From<Vec3> for [f32; 3] {
    fn from(v: Vec3) -> Self {
        [v.0, v.1, v.2]
    }
}

/// 2D vector type for texture coordinates
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec2(pub f32, pub f32);

/// A face (polygon) in the mesh
#[derive(Debug, Default, Clone)]
pub struct Face {
    /// Vertex indices
    pub v: SmallVec<[usize; 4]>,
    /// Vertex normal indices
    pub vn: SmallVec<[usize; 4]>,
    /// Vertex texture indices
    pub vt: SmallVec<[usize; 4]>,
}

/// A group of faces with optional material
#[derive(Debug, Clone)]
pub struct Group {
    pub name: String,
    pub material: Option<String>,
    pub face_range: Range<usize>,
}

/// A triangle with its vertices
#[derive(Debug, Clone, Copy)]
pub struct Triangle {
    pub vertices: [Vec3; 3],
}

impl Triangle {
    /// Calculate signed volume of tetrahedron formed by triangle and origin
    #[inline]
    pub fn signed_volume(&self) -> f64 {
        let a: Vector3<f64> = self.vertices[0].into();
        let b: Vector3<f64> = self.vertices[1].into();
        let c: Vector3<f64> = self.vertices[2].into();
        a.dot(&b.cross(&c)) / 6.0
    }

    /// Calculate the normal of the triangle
    pub fn normal(&self) -> Vec3 {
        let a = self.vertices[1].subtract(self.vertices[0]);
        let b = self.vertices[2].subtract(self.vertices[0]);
        a.cross(b).normalize()
    }
}

/// The main mesh data structure
#[derive(Debug, Clone, Default)]
pub struct Mesh {
    /// All vertices in the mesh
    pub vertices: Vec<Vec3>,
    /// All vertex normals
    pub normals: Vec<Vec3>,
    /// All texture coordinates
    pub textures: Vec<Vec2>,
    /// All faces (triangles or polygons)
    pub faces: Vec<Face>,
    /// Groups of faces
    pub groups: Vec<Group>,
    /// Material library names
    pub matlibs: Vec<String>,
    /// Cached computed values (lazily initialized)
    cache: Arc<RwLock<MeshCache>>,
}

impl Default for MeshCache {
    fn default() -> Self {
        Self {
            bounds: None,
            triangle_count: 0,
            is_watertight: None,
            non_manifold_edges: None,
            lod_face_indices: HashMap::new(),
        }
    }
}

impl Mesh {
    pub fn new() -> Self {
        Self {
            vertices: Vec::new(),
            normals: Vec::new(),
            textures: Vec::new(),
            faces: Vec::new(),
            groups: Vec::new(),
            matlibs: Vec::new(),
            cache: Arc::new(RwLock::new(MeshCache::default())),
        }
    }
    
    /// Initialize cache after mesh is fully loaded
    /// Call this once after parsing to pre-compute expensive values
    pub fn init_cache(&mut self) {
        puffin::profile_function!();
        
        let mut cache = self.cache.write();
        
        // Pre-compute bounds
        if !self.vertices.is_empty() {
            let (min_vertex, max_vertex) = self
                .vertices
                .par_iter()
                .fold(
                    || {
                        (
                            Vec3(f32::MAX, f32::MAX, f32::MAX),
                            Vec3(f32::MIN, f32::MIN, f32::MIN),
                        )
                    },
                    |acc, vertex| {
                        (
                            Vec3(
                                acc.0 .0.min(vertex.0),
                                acc.0 .1.min(vertex.1),
                                acc.0 .2.min(vertex.2),
                            ),
                            Vec3(
                                acc.1 .0.max(vertex.0),
                                acc.1 .1.max(vertex.1),
                                acc.1 .2.max(vertex.2),
                            ),
                        )
                    },
                )
                .reduce(
                    || {
                        (
                            Vec3(f32::MAX, f32::MAX, f32::MAX),
                            Vec3(f32::MIN, f32::MIN, f32::MIN),
                        )
                    },
                    |a, b| {
                        (
                            Vec3(a.0 .0.min(b.0 .0), a.0 .1.min(b.0 .1), a.0 .2.min(b.0 .2)),
                            Vec3(a.1 .0.max(b.1 .0), a.1 .1.max(b.1 .1), a.1 .2.max(b.1 .2)),
                        )
                    },
                );
            cache.bounds = Some((min_vertex, max_vertex));
        }
        
        // Pre-compute triangle count
        cache.triangle_count = self.faces
            .par_iter()
            .map(|face| face.v.len().saturating_sub(2))
            .sum();
        
        // Pre-compute LOD face indices for common LOD steps
        let total_triangles = cache.triangle_count;
        let lod_step = Self::calculate_lod_step_static(total_triangles);
        
        // Always have full detail indices
        cache.lod_face_indices.insert(1, (0..self.faces.len()).collect());
        
        // Pre-compute LOD indices if needed
        if lod_step > 1 {
            let lod_indices: Vec<usize> = (0..self.faces.len())
                .filter(|i| i % lod_step == 0)
                .collect();
            cache.lod_face_indices.insert(lod_step, lod_indices);
        }
    }
    
    /// Calculate LOD step based on triangle count (static version)
    fn calculate_lod_step_static(total_triangles: usize) -> usize {
        if total_triangles <= MAX_FULL_DETAIL_TRIANGLES {
            1
        } else {
            ((total_triangles as f32 / MAX_FULL_DETAIL_TRIANGLES as f32).ceil() as usize).max(1)
        }
    }
    
    /// Get pre-computed LOD face indices
    pub fn get_lod_face_indices(&self) -> Vec<usize> {
        let cache = self.cache.read();
        let lod_step = Self::calculate_lod_step_static(cache.triangle_count);
        
        if let Some(indices) = cache.lod_face_indices.get(&lod_step) {
            indices.clone()
        } else if let Some(indices) = cache.lod_face_indices.get(&1) {
            // Fall back to full detail if LOD not pre-computed
            if lod_step == 1 {
                indices.clone()
            } else {
                indices.iter()
                    .enumerate()
                    .filter(|(i, _)| i % lod_step == 0)
                    .map(|(_, &idx)| idx)
                    .collect()
            }
        } else {
            // No cache, compute on the fly
            if lod_step == 1 {
                (0..self.faces.len()).collect()
            } else {
                (0..self.faces.len())
                    .filter(|i| i % lod_step == 0)
                    .collect()
            }
        }
    }

    /// Calculate the bounding box of the mesh (cached)
    pub fn bounds(&self) -> Result<(Vec3, Vec3)> {
        // Try to get from cache first
        {
            let cache = self.cache.read();
            if let Some(bounds) = cache.bounds {
                return Ok(bounds);
            }
        }
        
        // Not cached, calculate
        if self.vertices.is_empty() {
            return Err(anyhow::anyhow!("mesh has no vertices"));
        }

        let (min_vertex, max_vertex) = self
            .vertices
            .par_iter()
            .fold(
                || {
                    (
                        Vec3(f32::MAX, f32::MAX, f32::MAX),
                        Vec3(f32::MIN, f32::MIN, f32::MIN),
                    )
                },
                |acc, vertex| {
                    (
                        Vec3(
                            acc.0 .0.min(vertex.0),
                            acc.0 .1.min(vertex.1),
                            acc.0 .2.min(vertex.2),
                        ),
                        Vec3(
                            acc.1 .0.max(vertex.0),
                            acc.1 .1.max(vertex.1),
                            acc.1 .2.max(vertex.2),
                        ),
                    )
                },
            )
            .reduce(
                || {
                    (
                        Vec3(f32::MAX, f32::MAX, f32::MAX),
                        Vec3(f32::MIN, f32::MIN, f32::MIN),
                    )
                },
                |a, b| {
                    (
                        Vec3(a.0 .0.min(b.0 .0), a.0 .1.min(b.0 .1), a.0 .2.min(b.0 .2)),
                        Vec3(a.1 .0.max(b.1 .0), a.1 .1.max(b.1 .1), a.1 .2.max(b.1 .2)),
                    )
                },
            );
        
        // Cache the result
        {
            let mut cache = self.cache.write();
            cache.bounds = Some((min_vertex, max_vertex));
        }

        Ok((min_vertex, max_vertex))
    }

    /// Calculate the diagonal of the bounding box
    pub fn diagonal(&self) -> Result<f32> {
        let (min_vertex, max_vertex) = self.bounds()?;

        let dx = max_vertex.0 - min_vertex.0;
        let dy = max_vertex.1 - min_vertex.1;
        let dz = max_vertex.2 - min_vertex.2;

        let diagonal = (dx * dx + dy * dy + dz * dz).sqrt();
        if diagonal == 0.0 {
            return Err(anyhow::anyhow!("mesh has 0 dimensions"));
        }

        Ok(diagonal)
    }

    /// Count the number of triangles in the mesh (cached)
    pub fn triangle_count(&self) -> usize {
        // Try cache first
        {
            let cache = self.cache.read();
            if cache.triangle_count > 0 {
                return cache.triangle_count;
            }
        }
        
        // Calculate and cache
        let count: usize = self.faces
            .par_iter()
            .map(|face| face.v.len().saturating_sub(2))
            .sum();
        
        {
            let mut cache = self.cache.write();
            cache.triangle_count = count;
        }
        
        count
    }

    /// Scale the mesh uniformly by a factor
    pub fn scale(&mut self, factor: f32) {
        self.vertices.par_iter_mut().for_each(|v| {
            v.0 *= factor;
            v.1 *= factor;
            v.2 *= factor;
        });
    }

    /// Scale the mesh to have a specific diagonal
    pub fn scale_to_diagonal(&mut self, target_diagonal: f32) -> Result<f32> {
        let current_diagonal = self.diagonal()?;
        let scale_factor = target_diagonal / current_diagonal;

        let (min_v, max_v) = self.bounds()?;
        let center_x = (min_v.0 + max_v.0) / 2.0;
        let center_y = (min_v.1 + max_v.1) / 2.0;
        let center_z = (min_v.2 + max_v.2) / 2.0;

        self.vertices.par_iter_mut().for_each(|v| {
            v.0 = (v.0 - center_x) * scale_factor + center_x;
            v.1 = (v.1 - center_y) * scale_factor + center_y;
            v.2 = (v.2 - center_z) * scale_factor + center_z;
        });

        Ok(scale_factor)
    }

    /// Get the center of the mesh
    pub fn center(&self) -> Result<Vec3> {
        let (min_v, max_v) = self.bounds()?;
        Ok(Vec3(
            (min_v.0 + max_v.0) / 2.0,
            (min_v.1 + max_v.1) / 2.0,
            (min_v.2 + max_v.2) / 2.0,
        ))
    }

    /// Quantize a vertex position for hashing (0.0001mm precision)
    fn quantize_vertex(v: &Vec3) -> (i64, i64, i64) {
        // 0.0001mm precision for welding
        const PRECISION: f32 = 10000.0;
        (
            (v.0 * PRECISION).round() as i64,
            (v.1 * PRECISION).round() as i64,
            (v.2 * PRECISION).round() as i64,
        )
    }

    /// Build edge topology map using vertex POSITIONS (not indices)
    /// This properly handles STL files where vertices are not shared between triangles
    pub fn edge_topology(&self) -> HashMap<((i64, i64, i64), (i64, i64, i64)), usize> {
        let mut map = HashMap::new();

        for face in &self.faces {
            if face.v.len() < 3 {
                continue;
            }

            for i in 0..face.v.len() {
                let v0_idx = face.v[i];
                let v1_idx = face.v[(i + 1) % face.v.len()];

                if v0_idx >= self.vertices.len() || v1_idx >= self.vertices.len() {
                    continue;
                }

                let v0 = Self::quantize_vertex(&self.vertices[v0_idx]);
                let v1 = Self::quantize_vertex(&self.vertices[v1_idx]);

                if v0 == v1 {
                    continue;
                }

                // Order the edge consistently regardless of face winding
                let edge = if v0 < v1 { (v0, v1) } else { (v1, v0) };
                *map.entry(edge).or_insert(0) += 1;
            }
        }

        map
    }

    /// Pre-compute all edge-topology-derived metrics in a single pass.
    /// Call this off the UI thread (e.g. during async mesh loading) so that
    /// `is_watertight()`, `non_manifold_edge_count()`, and `boundary_edges()`
    /// hit the warm cache instead of recomputing.
    pub fn warm_cache(&self) {
        {
            let cache = self.cache.read();
            if cache.is_watertight.is_some() && cache.non_manifold_edges.is_some() {
                return;
            }
        }

        let topology = self.edge_topology();
        let mut non_manifold = 0usize;
        for &count in topology.values() {
            if count != 2 {
                non_manifold += 1;
            }
        }
        let is_watertight = non_manifold == 0;

        let mut cache = self.cache.write();
        cache.is_watertight = Some(is_watertight);
        cache.non_manifold_edges = Some(non_manifold);
    }

    /// Find boundary edges (edges with only one adjacent face)
    /// These typically form holes in the mesh
    /// Returns position-based edges for STL compatibility
    pub fn boundary_edges(&self) -> Vec<((i64, i64, i64), (i64, i64, i64))> {
        self.edge_topology()
            .into_iter()
            .filter(|(_, count)| *count == 1)
            .map(|(edge, _)| edge)
            .collect()
    }

    /// Check if the mesh is watertight (manifold) - cached
    /// Returns true if all edges are shared by exactly 2 faces
    pub fn is_watertight(&self) -> bool {
        {
            let cache = self.cache.read();
            if let Some(is_watertight) = cache.is_watertight {
                return is_watertight;
            }
        }

        self.warm_cache();
        self.cache.read().is_watertight.unwrap_or(false)
    }

    /// Count non-manifold edges (edges shared by != 2 faces) - cached
    pub fn non_manifold_edge_count(&self) -> usize {
        {
            let cache = self.cache.read();
            if let Some(count) = cache.non_manifold_edges {
                return count;
            }
        }

        self.warm_cache();
        self.cache.read().non_manifold_edges.unwrap_or(0)
    }

    /// Weld duplicate vertices
    pub fn weld_vertices(&mut self) {
        let mut map: HashMap<(u32, u32, u32), usize> = HashMap::new();
        let mut new_vertices: Vec<Vec3> = Vec::with_capacity(self.vertices.len());
        let mut remap: Vec<usize> = vec![0; self.vertices.len()];

        for (old_index, vertex) in self.vertices.iter().enumerate() {
            let key = (vertex.0.to_bits(), vertex.1.to_bits(), vertex.2.to_bits());

            let idx = *map.entry(key).or_insert_with(|| {
                let idx = new_vertices.len();
                new_vertices.push(*vertex);
                idx
            });

            remap[old_index] = idx;
        }

        self.vertices = new_vertices;
        for face in &mut self.faces {
            for i in 0..face.v.len() {
                face.v[i] = remap[face.v[i]];
            }
        }
    }

    /// Build face adjacency map using vertex POSITIONS (not indices)
    /// This properly handles STL files where vertices are not shared between triangles
    /// Returns a map of Edge -> Vec<FaceIndex>
    pub fn build_face_adjacency_map(&self) -> HashMap<((i64, i64, i64), (i64, i64, i64)), SmallVec<[usize; 4]>> {
        let mut map: HashMap<((i64, i64, i64), (i64, i64, i64)), SmallVec<[usize; 4]>> = HashMap::new();

        for (face_idx, face) in self.faces.iter().enumerate() {
            if face.v.len() < 3 {
                continue;
            }

            for i in 0..face.v.len() {
                let v0_idx = face.v[i];
                let v1_idx = face.v[(i + 1) % face.v.len()];

                if v0_idx >= self.vertices.len() || v1_idx >= self.vertices.len() {
                    continue;
                }

                let v0 = Self::quantize_vertex(&self.vertices[v0_idx]);
                let v1 = Self::quantize_vertex(&self.vertices[v1_idx]);

                if v0 == v1 {
                    continue;
                }

                // Order the edge consistently regardless of face winding
                let edge = if v0 < v1 { (v0, v1) } else { (v1, v0) };
                map.entry(edge)
                   .or_insert_with(SmallVec::new)
                   .push(face_idx);
            }
        }

        map
    }

    /// Get detailed topology statistics
    /// Returns (boundary_edges, manifold_edges, non_manifold_edges)
    pub fn topology_stats(&self) -> (usize, usize, usize) {
        let adjacency = self.build_face_adjacency_map();
        let mut boundary = 0;
        let mut manifold = 0;
        let mut non_manifold = 0;

        for faces in adjacency.values() {
            match faces.len() {
                1 => boundary += 1,
                2 => manifold += 1,
                _ => non_manifold += 1,
            }
        }

        (boundary, manifold, non_manifold)
    }

    /// Removes faces connected to edges that are shared by more than 2 faces.
    /// This is the standard industry approach for repairing non-manifold geometry.
    /// Returns the number of faces removed.
    pub fn repair_non_manifold_edges(&mut self) -> usize {
        // Build map: Edge -> List of Faces
        let adjacency = self.build_face_adjacency_map();

        // Identify bad faces (faces connected to non-manifold edges)
        let mut bad_faces_set: HashSet<usize> = HashSet::new();
        for faces in adjacency.values() {
            if faces.len() > 2 {
                // This edge is non-manifold (e.g. 3+ faces meeting at a line)
                // Mark all attached faces for deletion
                for &face_idx in faces {
                    bad_faces_set.insert(face_idx);
                }
            }
        }

        if bad_faces_set.is_empty() {
            return 0;
        }

        // Rebuild the face list, filtering out the bad ones
        let old_faces = std::mem::take(&mut self.faces);
        let mut valid_faces = Vec::with_capacity(old_faces.len() - bad_faces_set.len());

        for (i, face) in old_faces.into_iter().enumerate() {
            if !bad_faces_set.contains(&i) {
                valid_faces.push(face);
            }
        }

        let removed_count = bad_faces_set.len();
        self.faces = valid_faces;

        // Invalidate cache since mesh topology changed
        {
            let mut cache = self.cache.write();
            *cache = MeshCache::default();
        }

        removed_count
    }

    /// Count the number of boundary edges (holes) in the mesh
    pub fn boundary_edge_count(&self) -> usize {
        let adjacency = self.build_face_adjacency_map();
        adjacency.values().filter(|faces| faces.len() == 1).count()
    }

    /// Check if the mesh has non-manifold edges (edges with > 2 faces).
    /// Uses the cached value from `warm_cache()` when available.
    pub fn has_non_manifold_edges(&self) -> bool {
        self.non_manifold_edge_count() > 0
    }

    /// Invalidate the mesh cache (call after external modifications)
    pub fn invalidate_cache(&mut self) {
        let mut cache = self.cache.write();
        *cache = MeshCache::default();
    }
}

/// Load a mesh from a file path
pub fn load_mesh(path: &Path) -> Result<Mesh> {
    puffin::profile_function!();
    let bytes = std::fs::read(path)?;
    load_mesh_from_bytes(&bytes, Format::from_extension(path))
}

/// Load a mesh from bytes with optional format hint
pub fn load_mesh_from_bytes(bytes: &[u8], format_hint: Option<Format>) -> Result<Mesh> {
    puffin::profile_function!();
    
    let format = format_hint
        .or_else(|| Format::from_magic_bytes(bytes))
        .ok_or_else(|| anyhow::anyhow!("Could not detect file format"))?;

    let mut mesh = match format {
        Format::STL => stl::parse(bytes)?,
        Format::OBJ => obj::parse(bytes)?,
    };
    
    // Initialize cache with pre-computed values
    {
        puffin::profile_scope!("init_mesh_cache");
        mesh.init_cache();
    }
    
    Ok(mesh)
}
