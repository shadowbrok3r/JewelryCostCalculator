//! Ring sizing calculations and detection
//!
//! US Ring Sizes are based on inner circumference:
//! - Size = (circumference_mm - 36.5) / 2.55
//! - Circumference = 36.5 + (size * 2.55)
//!
//! Common US sizes range from 3 to 15

use serde::{Deserialize, Serialize};
use log::{info, debug};

use crate::mesh::{Mesh, Vec3};

/// US Ring size with half-size precision
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RingSize(pub f64);

impl RingSize {
    /// Create a new ring size (validates it's a half-size increment)
    pub fn new(size: f64) -> Self {
        // Round to nearest 0.5
        let rounded = (size * 2.0).round() / 2.0;
        RingSize(rounded)
    }

    /// Get the inner diameter in mm for this ring size
    pub fn inner_diameter_mm(&self) -> f64 {
        // Formula derived from standard US ring size chart
        // Size 7 = 17.3mm diameter
        // Each size step = ~0.8mm diameter change
        let circumference = self.inner_circumference_mm();
        circumference / std::f64::consts::PI
    }

    /// Get the inner circumference in mm for this ring size
    pub fn inner_circumference_mm(&self) -> f64 {
        36.5 + (self.0 * 2.55)
    }

    /// Create a ring size from inner diameter
    pub fn from_diameter_mm(diameter: f64) -> Self {
        let circumference = diameter * std::f64::consts::PI;
        Self::from_circumference_mm(circumference)
    }

    /// Create a ring size from inner circumference
    pub fn from_circumference_mm(circumference: f64) -> Self {
        let size = (circumference - 36.5) / 2.55;
        RingSize::new(size)
    }

    /// Format as a display string (e.g., "US 7" or "US 7.5")
    pub fn display(&self) -> String {
        if self.0.fract() == 0.0 {
            format!("US {}", self.0 as i32)
        } else {
            format!("US {:.1}", self.0)
        }
    }

    /// Generate a range of ring sizes in 0.5 increments
    pub fn range(start: f64, end: f64) -> Vec<RingSize> {
        let mut sizes = Vec::new();
        let mut current = (start * 2.0).round() / 2.0;
        let end_rounded = (end * 2.0).round() / 2.0;

        while current <= end_rounded {
            sizes.push(RingSize(current));
            current += 0.5;
        }
        sizes
    }

    /// Common US ring sizes (3 to 15 in 0.5 increments)
    pub fn common_sizes() -> Vec<RingSize> {
        Self::range(3.0, 15.0)
    }
}

impl std::fmt::Display for RingSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display())
    }
}

/// Ring size lookup table with diameter/circumference values
#[derive(Debug, Clone)]
pub struct RingSizeChart {
    sizes: Vec<(RingSize, f64, f64)>, // (size, diameter_mm, circumference_mm)
}

impl Default for RingSizeChart {
    fn default() -> Self {
        let sizes: Vec<(RingSize, f64, f64)> = RingSize::common_sizes()
            .into_iter()
            .map(|size| {
                (
                    size,
                    size.inner_diameter_mm(),
                    size.inner_circumference_mm(),
                )
            })
            .collect();
        Self { sizes }
    }
}

impl RingSizeChart {
    /// Find the closest ring size for a given diameter
    pub fn closest_size_for_diameter(&self, diameter_mm: f64) -> RingSize {
        self.sizes
            .iter()
            .min_by(|a, b| {
                let diff_a = (a.1 - diameter_mm).abs();
                let diff_b = (b.1 - diameter_mm).abs();
                diff_a.partial_cmp(&diff_b).unwrap()
            })
            .map(|(size, _, _)| *size)
            .unwrap_or(RingSize(7.0))
    }

    /// Get all sizes as a formatted table string
    pub fn as_table(&self) -> String {
        let mut table = String::from("Size | Diameter (mm) | Circumference (mm)\n");
        table.push_str("-----|---------------|-------------------\n");
        for (size, diam, circ) in &self.sizes {
            table.push_str(&format!("{:5} | {:13.2} | {:17.2}\n", size.display(), diam, circ));
        }
        table
    }
}

/// Detected ring hole information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedRingHole {
    /// Estimated center of the hole
    pub center: [f64; 3],
    /// Estimated inner diameter in mm
    pub diameter_mm: f64,
    /// Confidence level (0.0 to 1.0)
    pub confidence: f64,
    /// The axis of the hole (0=X, 1=Y, 2=Z)
    pub axis: usize,
    /// Direction vector of the hole axis (normalized)
    pub axis_direction: [f64; 3],
}

/// Axis identifiers for ring hole detection
#[derive(Debug, Clone, Copy)]
enum Axis {
    X = 0,
    Y = 1,
    Z = 2,
}

impl Axis {
    fn direction(&self) -> [f64; 3] {
        match self {
            Axis::X => [1.0, 0.0, 0.0],
            Axis::Y => [0.0, 1.0, 0.0],
            Axis::Z => [0.0, 0.0, 1.0],
        }
    }

    fn perpendicular_axes(&self) -> (usize, usize) {
        match self {
            Axis::X => (1, 2), // Y, Z
            Axis::Y => (0, 2), // X, Z
            Axis::Z => (0, 1), // X, Y
        }
    }
}

/// Attempt to detect the inner ring hole from mesh geometry
/// 
/// Algorithm:
/// 1. Find the mesh center (bounding box center)
/// 2. For each axis (X, Y, Z), cast rays outward from the center
/// 3. Find where rays first hit the mesh (inner surface)
/// 4. Calculate diameter from consistent ray hits
/// 5. Choose the axis with the best circular fit
pub fn detect_ring_hole(mesh: &Mesh) -> Option<DetectedRingHole> {
    let center = mesh.center().ok()?;
    let (min_v, max_v) = mesh.bounds().ok()?;
    
    info!("Detecting ring hole from center: ({:.2}, {:.2}, {:.2})", center.0, center.1, center.2);
    
    // Build triangle list for ray casting
    let triangles = build_triangle_list(mesh);
    if triangles.is_empty() {
        info!("No triangles found for ray casting");
        return None;
    }
    
    // Test each axis
    let axes = [Axis::X, Axis::Y, Axis::Z];
    let mut best_result: Option<DetectedRingHole> = None;
    let mut best_confidence = 0.0;
    
    for axis in &axes {
        debug!("Testing axis {:?}", axis);
        
        if let Some(result) = detect_hole_on_axis(mesh, &triangles, center, *axis, &min_v, &max_v) {
            info!("Axis {:?}: diameter={:.2}mm, confidence={:.2}", axis, result.diameter_mm, result.confidence);
            
            // Prefer higher confidence and reasonable ring sizes (10-25mm diameter)
            let is_reasonable_size = result.diameter_mm >= 10.0 && result.diameter_mm <= 30.0;
            let score = result.confidence * if is_reasonable_size { 1.5 } else { 0.5 };
            
            if score > best_confidence {
                best_confidence = score;
                best_result = Some(result);
            }
        }
    }
    
    best_result
}

/// Build a list of triangles for ray intersection testing
fn build_triangle_list(mesh: &Mesh) -> Vec<([f64; 3], [f64; 3], [f64; 3])> {
    let mut triangles = Vec::new();
    
    for face in &mesh.faces {
        if face.v.len() < 3 {
            continue;
        }
        
        let v0_idx = face.v[0];
        if v0_idx >= mesh.vertices.len() {
            continue;
        }
        let v0 = &mesh.vertices[v0_idx];
        
        for i in 1..(face.v.len() - 1) {
            let v1_idx = face.v[i];
            let v2_idx = face.v[i + 1];
            
            if v1_idx >= mesh.vertices.len() || v2_idx >= mesh.vertices.len() {
                continue;
            }
            
            let v1 = &mesh.vertices[v1_idx];
            let v2 = &mesh.vertices[v2_idx];
            
            triangles.push((
                [v0.0 as f64, v0.1 as f64, v0.2 as f64],
                [v1.0 as f64, v1.1 as f64, v1.2 as f64],
                [v2.0 as f64, v2.1 as f64, v2.2 as f64],
            ));
        }
    }
    
    triangles
}

/// Detect hole along a specific axis
fn detect_hole_on_axis(
    _mesh: &Mesh,
    triangles: &[([f64; 3], [f64; 3], [f64; 3])],
    center: Vec3,
    axis: Axis,
    min_v: &Vec3,
    max_v: &Vec3,
) -> Option<DetectedRingHole> {
    let center_f64 = [center.0 as f64, center.1 as f64, center.2 as f64];
    let (perp_a, perp_b) = axis.perpendicular_axes();
    
    // Maximum ray distance (diagonal of bounding box)
    let diagonal = ((max_v.0 - min_v.0).powi(2) + 
                   (max_v.1 - min_v.1).powi(2) + 
                   (max_v.2 - min_v.2).powi(2)).sqrt() as f64;
    
    // Cast rays in a circle around the axis
    let num_rays = 16;
    let mut hit_distances: Vec<f64> = Vec::new();
    
    for i in 0..num_rays {
        let angle = (i as f64) * 2.0 * std::f64::consts::PI / (num_rays as f64);
        
        // Create ray direction perpendicular to the axis
        let mut ray_dir = [0.0, 0.0, 0.0];
        ray_dir[perp_a] = angle.cos();
        ray_dir[perp_b] = angle.sin();
        
        // Find closest intersection in this direction
        if let Some(dist) = cast_ray(triangles, &center_f64, &ray_dir, diagonal) {
            hit_distances.push(dist);
        }
    }
    
    if hit_distances.len() < num_rays / 2 {
        debug!("Not enough ray hits: {} / {}", hit_distances.len(), num_rays);
        return None;
    }
    
    // Calculate statistics
    let mean_dist: f64 = hit_distances.iter().sum::<f64>() / hit_distances.len() as f64;
    let variance: f64 = hit_distances.iter()
        .map(|d| (d - mean_dist).powi(2))
        .sum::<f64>() / hit_distances.len() as f64;
    let std_dev = variance.sqrt();
    
    // Confidence based on how circular the hole is (low std dev = more circular)
    // For a perfect circle, std_dev should be 0
    let relative_std = std_dev / mean_dist;
    let confidence = (1.0 - relative_std * 5.0).max(0.0).min(1.0);
    
    let diameter = mean_dist * 2.0;
    
    debug!("Axis {:?}: mean_dist={:.2}, std_dev={:.2}, diameter={:.2}, confidence={:.2}", 
           axis, mean_dist, std_dev, diameter, confidence);
    
    Some(DetectedRingHole {
        center: center_f64,
        diameter_mm: diameter,
        confidence,
        axis: axis as usize,
        axis_direction: axis.direction(),
    })
}

/// Cast a ray and find the closest intersection with the mesh
fn cast_ray(
    triangles: &[([f64; 3], [f64; 3], [f64; 3])],
    origin: &[f64; 3],
    direction: &[f64; 3],
    max_dist: f64,
) -> Option<f64> {
    let mut closest_dist: Option<f64> = None;
    
    for (v0, v1, v2) in triangles {
        if let Some(t) = ray_triangle_intersection(origin, direction, v0, v1, v2) {
            if t > 0.001 && t < max_dist {
                closest_dist = Some(closest_dist.map_or(t, |d| d.min(t)));
            }
        }
    }
    
    closest_dist
}

/// Möller–Trumbore ray-triangle intersection algorithm
fn ray_triangle_intersection(
    origin: &[f64; 3],
    direction: &[f64; 3],
    v0: &[f64; 3],
    v1: &[f64; 3],
    v2: &[f64; 3],
) -> Option<f64> {
    const EPSILON: f64 = 1e-9;
    
    // Edge vectors
    let edge1 = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
    let edge2 = [v2[0] - v0[0], v2[1] - v0[1], v2[2] - v0[2]];
    
    // Cross product of direction and edge2
    let h = cross(direction, &edge2);
    let a = dot(&edge1, &h);
    
    if a.abs() < EPSILON {
        return None; // Ray is parallel to triangle
    }
    
    let f = 1.0 / a;
    let s = [origin[0] - v0[0], origin[1] - v0[1], origin[2] - v0[2]];
    let u = f * dot(&s, &h);
    
    if u < 0.0 || u > 1.0 {
        return None;
    }
    
    let q = cross(&s, &edge1);
    let v = f * dot(direction, &q);
    
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    
    let t = f * dot(&edge2, &q);
    
    if t > EPSILON {
        Some(t)
    } else {
        None
    }
}

/// Cross product of two 3D vectors
#[inline]
fn cross(a: &[f64; 3], b: &[f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Dot product of two 3D vectors
#[inline]
fn dot(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Calculate the scale factor to resize a ring from one size to another
pub fn calculate_scale_factor(current_diameter: f64, target_size: RingSize) -> f64 {
    let target_diameter = target_size.inner_diameter_mm();
    target_diameter / current_diameter
}

/// Calculate the new volume after scaling
pub fn calculate_scaled_volume(original_volume: f64, scale_factor: f64) -> f64 {
    // Volume scales with the cube of the linear scale factor
    original_volume * scale_factor.powi(3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ring_size_diameter() {
        let size_7 = RingSize::new(7.0);
        let diameter = size_7.inner_diameter_mm();
        // Size 7 should be approximately 17.3mm diameter
        assert!((diameter - 17.3).abs() < 0.5);
    }

    #[test]
    fn test_ring_size_from_diameter() {
        let size = RingSize::from_diameter_mm(17.3);
        // Should be close to size 7
        assert!((size.0 - 7.0).abs() < 0.5);
    }

    #[test]
    fn test_ring_size_range() {
        let sizes = RingSize::range(5.0, 8.0);
        assert_eq!(sizes.len(), 7); // 5, 5.5, 6, 6.5, 7, 7.5, 8
        assert_eq!(sizes[0].0, 5.0);
        assert_eq!(sizes[6].0, 8.0);
    }

    #[test]
    fn test_scale_factor() {
        let current_diameter = 17.3; // Size 7
        let target = RingSize::new(8.0);
        let scale = calculate_scale_factor(current_diameter, target);
        
        // Size 8 is about 18.2mm, so scale should be ~1.05
        assert!(scale > 1.0 && scale < 1.1);
    }

    #[test]
    fn test_scaled_volume() {
        let original = 1.0;
        let scale = 2.0;
        let scaled = calculate_scaled_volume(original, scale);
        assert!((scaled - 8.0).abs() < 0.001); // 2^3 = 8
    }

    #[test]
    fn test_display() {
        assert_eq!(RingSize::new(7.0).display(), "US 7");
        assert_eq!(RingSize::new(7.5).display(), "US 7.5");
    }
    
    #[test]
    fn test_ring_size_9() {
        let size_9 = RingSize::new(9.0);
        let diameter = size_9.inner_diameter_mm();
        // Size 9 should be approximately 19.0mm diameter
        assert!((diameter - 19.0).abs() < 0.5, "Size 9 diameter was {}", diameter);
    }
}
