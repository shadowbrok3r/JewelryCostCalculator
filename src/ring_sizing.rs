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

/// A robust inner-bore measurement: the largest empty cylinder threading the ring.
#[derive(Debug, Clone, Copy)]
pub struct InnerBore {
    /// Unit direction of the finger axis (may be non-cardinal for tilted models).
    pub axis_dir: [f64; 3],
    /// A point on the bore axis (the bore center).
    pub center: [f64; 3],
    pub diameter_mm: f64,
    /// Angular coverage of the bore wall (0..1); proxy for confidence.
    pub coverage: f64,
}

#[inline]
fn vsub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
#[inline]
fn vdot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
#[inline]
fn vcross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
#[inline]
fn vnorm(a: [f64; 3]) -> [f64; 3] {
    let l = vdot(a, a).sqrt();
    if l < 1e-12 {
        a
    } else {
        [a[0] / l, a[1] / l, a[2] / l]
    }
}

/// Orthonormal basis (u, v) spanning the plane perpendicular to unit `n`.
fn plane_basis(n: [f64; 3]) -> ([f64; 3], [f64; 3]) {
    let (ax, ay, az) = (n[0].abs(), n[1].abs(), n[2].abs());
    let e = if ax <= ay && ax <= az {
        [1.0, 0.0, 0.0]
    } else if ay <= az {
        [0.0, 1.0, 0.0]
    } else {
        [0.0, 0.0, 1.0]
    };
    let u = vnorm(vcross(n, e));
    let v = vcross(n, u);
    (u, v)
}

/// Tilt unit `n` by `ang` radians toward unit `axis` (axis ⊥ n).
fn tilt(n: [f64; 3], axis: [f64; 3], ang: f64) -> [f64; 3] {
    let (s, c) = ang.sin_cos();
    vnorm([
        n[0] * c + axis[0] * s,
        n[1] * c + axis[1] * s,
        n[2] * c + axis[2] * s,
    ])
}

/// Distance from (px,py) to the nearest of `pts`.
fn nearest_dist(px: f64, py: f64, pts: &[(f64, f64)]) -> f64 {
    let mut best = f64::INFINITY;
    for &(x, y) in pts {
        let d = (x - px).powi(2) + (y - py).powi(2);
        if d < best {
            best = d;
        }
    }
    best.sqrt()
}

/// Largest circle centered in `pts`' bbox that contains no point (pole of
/// inaccessibility), via grid refinement. Returns (center, radius).
fn largest_empty_circle(pts: &[(f64, f64)]) -> ((f64, f64), f64) {
    let (mut bx0, mut bx1) = (f64::INFINITY, f64::NEG_INFINITY);
    let (mut by0, mut by1) = (f64::INFINITY, f64::NEG_INFINITY);
    for &(x, y) in pts {
        bx0 = bx0.min(x);
        bx1 = bx1.max(x);
        by0 = by0.min(y);
        by1 = by1.max(y);
    }
    let mut best_c = ((bx0 + bx1) * 0.5, (by0 + by1) * 0.5);
    let mut best_r = nearest_dist(best_c.0, best_c.1, pts);
    for _ in 0..7 {
        let gx = (bx1 - bx0) / 8.0;
        let gy = (by1 - by0) / 8.0;
        for i in 0..=8 {
            for j in 0..=8 {
                let cx = bx0 + gx * i as f64;
                let cy = by0 + gy * j as f64;
                let r = nearest_dist(cx, cy, pts);
                if r > best_r {
                    best_r = r;
                    best_c = (cx, cy);
                }
            }
        }
        bx0 = best_c.0 - gx;
        bx1 = best_c.0 + gx;
        by0 = best_c.1 - gy;
        by1 = best_c.1 + gy;
    }
    (best_c, best_r)
}

/// Measure the inner bore for one candidate finger-axis direction `n`: slab the
/// vertices through the band perpendicular to `n`, project onto the plane, and
/// fit the largest empty circle. Returns the bore (3D center, diameter, coverage).
fn bore_for_axis(coords: &[[f64; 3]], centroid: [f64; 3], n: [f64; 3]) -> Option<InnerBore> {
    let (u, v) = plane_basis(n);
    let mut tvals: Vec<f64> = coords.iter().map(|c| vdot(vsub(*c, centroid), n)).collect();
    tvals.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let t_med = tvals[tvals.len() / 2];
    let span = tvals[tvals.len() - 1] - tvals[0];
    let half = (span * 0.05).max(0.8);

    let slab: Vec<(f64, f64)> = coords
        .iter()
        .filter(|c| (vdot(vsub(**c, centroid), n) - t_med).abs() <= half)
        .map(|c| {
            let d = vsub(*c, centroid);
            (vdot(d, u), vdot(d, v))
        })
        .collect();
    if slab.len() < 100 {
        return None;
    }
    let step = (slab.len() / 4000).max(1);
    let sub: Vec<(f64, f64)> = slab.iter().copied().step_by(step).collect();
    let (c2d, r) = largest_empty_circle(&sub);
    let diameter = r * 2.0;

    let mut buckets = [false; 72];
    for &(x, y) in &slab {
        let d = (x - c2d.0).hypot(y - c2d.1);
        if d >= r && d <= r + 1.0 {
            let ang = (y - c2d.1).atan2(x - c2d.0) + std::f64::consts::PI;
            let b = ((ang / (2.0 * std::f64::consts::PI)) * 72.0) as usize;
            buckets[b.min(71)] = true;
        }
    }
    let coverage = buckets.iter().filter(|b| **b).count() as f64 / 72.0;
    let center = [
        centroid[0] + u[0] * c2d.0 + v[0] * c2d.1 + n[0] * t_med,
        centroid[1] + u[1] * c2d.0 + v[1] * c2d.1 + n[1] * t_med,
        centroid[2] + u[2] * c2d.0 + v[2] * c2d.1 + n[2] * t_med,
    ];
    Some(InnerBore { axis_dir: n, center, diameter_mm: diameter, coverage })
}

/// Score a candidate bore: coverage dominates (the true finger axis is the only
/// direction whose inscribed circle has full angular wall coverage); a plausible
/// diameter breaks ties toward the larger empty cylinder.
fn bore_score(b: &InnerBore) -> f64 {
    let plausible = (12.0..=24.0).contains(&b.diameter_mm);
    b.coverage * 100.0 + if plausible { b.diameter_mm } else { -100.0 }
}

/// Measure a ring's inner finger-bore diameter as the largest empty cylinder
/// threading the band — robust to tall settings, galleries, stacked bands, an
/// off-center hole, AND a non-cardinal (tilted) finger axis.
///
/// The bore center is found by a largest-empty-circle fit (so it self-corrects
/// instead of assuming the bbox center); the finger axis is found by searching
/// orientations and keeping the one whose inscribed circle has the fullest wall
/// coverage and largest plausible diameter. A hemisphere seed sweep locates the
/// basin, then a shrinking local tilt search refines the axis.
pub fn measure_inner_diameter(mesh: &Mesh) -> Option<InnerBore> {
    let coords_full: Vec<[f64; 3]> = mesh
        .vertices
        .iter()
        .map(|v| [v.0 as f64, v.1 as f64, v.2 as f64])
        .collect();
    measure_inner_bore(&coords_full)
}

/// Core of [`measure_inner_diameter`] over a raw vertex cloud (unit-testable and
/// orientation-independent): returns the largest empty cylinder threading it.
pub fn measure_inner_bore(coords_full: &[[f64; 3]]) -> Option<InnerBore> {
    if coords_full.len() < 100 {
        return None;
    }
    let centroid_of = |pts: &[[f64; 3]]| -> [f64; 3] {
        let n = pts.len() as f64;
        [
            pts.iter().map(|c| c[0]).sum::<f64>() / n,
            pts.iter().map(|c| c[1]).sum::<f64>() / n,
            pts.iter().map(|c| c[2]).sum::<f64>() / n,
        ]
    };
    // The axis search runs over a subsample (keeps each candidate eval cheap on
    // dense meshes); the winning axis is re-measured at full resolution below.
    let step = (coords_full.len() / 6000).max(1);
    let coords: Vec<[f64; 3]> = coords_full.iter().step_by(step).copied().collect();
    let centroid = centroid_of(&coords);

    // Seed directions: the 3 cardinal axes + a Fibonacci hemisphere sweep (n ≡ -n,
    // so fold to the upper hemisphere). Covers tilted finger axes the cardinals miss.
    let mut seeds: Vec<[f64; 3]> = vec![[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let m = 96usize;
    let golden = std::f64::consts::PI * (3.0 - 5.0_f64.sqrt());
    for i in 0..m {
        let y = 1.0 - (i as f64 + 0.5) / m as f64; // 1 -> ~0
        let rad = (1.0 - y * y).max(0.0).sqrt();
        let phi = i as f64 * golden;
        let mut dir = [rad * phi.cos(), y, rad * phi.sin()];
        if dir[1] < 0.0 {
            dir = [-dir[0], -dir[1], -dir[2]];
        }
        seeds.push(vnorm(dir));
    }

    let mut best: Option<InnerBore> = None;
    let mut best_score = f64::NEG_INFINITY;
    for s in &seeds {
        if let Some(b) = bore_for_axis(&coords, centroid, *s) {
            let sc = bore_score(&b);
            if sc > best_score {
                best_score = sc;
                best = Some(b);
            }
        }
    }
    let mut best = best?;

    // Local refinement: tilt the axis by shrinking angles, hill-climbing the score.
    for &deg in &[8.0_f64, 4.0, 2.0, 1.0, 0.5] {
        let ang = deg.to_radians();
        loop {
            let (u, v) = plane_basis(best.axis_dir);
            let cands = [
                tilt(best.axis_dir, u, ang),
                tilt(best.axis_dir, u, -ang),
                tilt(best.axis_dir, v, ang),
                tilt(best.axis_dir, v, -ang),
            ];
            let mut improved = false;
            for c in cands {
                if let Some(b) = bore_for_axis(&coords, centroid, c) {
                    let sc = bore_score(&b);
                    if sc > best_score {
                        best_score = sc;
                        best = b;
                        improved = true;
                    }
                }
            }
            if !improved {
                break;
            }
        }
    }

    // Re-measure the winning axis at full resolution for a precise diameter.
    if let Some(b) = bore_for_axis(coords_full, centroid_of(coords_full), best.axis_dir) {
        if (12.0..=24.0).contains(&b.diameter_mm) {
            best = b;
        }
    }
    Some(best)
}

impl InnerBore {
    /// Adapt to the GUI overlay's `DetectedRingHole` (coverage → confidence;
    /// axis_dir kept for correct tilted-circle rendering; `axis` = dominant cardinal).
    pub fn to_detected_hole(&self) -> DetectedRingHole {
        let a = self.axis_dir;
        let axis = if a[0].abs() >= a[1].abs() && a[0].abs() >= a[2].abs() {
            0
        } else if a[1].abs() >= a[2].abs() {
            1
        } else {
            2
        };
        DetectedRingHole {
            center: self.center,
            diameter_mm: self.diameter_mm,
            confidence: self.coverage,
            axis,
            axis_direction: self.axis_dir,
        }
    }
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

    /// Surface points of a torus in the XY plane (finger axis = Z).
    fn torus_points(major_r: f64, tube_r: f64, nu: usize, nv: usize) -> Vec<[f64; 3]> {
        let tau = 2.0 * std::f64::consts::PI;
        let mut pts = Vec::with_capacity(nu * nv);
        for i in 0..nu {
            let u = tau * i as f64 / nu as f64;
            for j in 0..nv {
                let v = tau * j as f64 / nv as f64;
                let r = major_r + tube_r * v.cos();
                pts.push([r * u.cos(), r * u.sin(), tube_r * v.sin()]);
            }
        }
        pts
    }

    /// Rotate a point cloud by Z*Y*X Euler degrees.
    fn rotate(pts: &[[f64; 3]], rx: f64, ry: f64, rz: f64) -> Vec<[f64; 3]> {
        let (sx, cx) = rx.to_radians().sin_cos();
        let (sy, cy) = ry.to_radians().sin_cos();
        let (sz, cz) = rz.to_radians().sin_cos();
        let r = [
            [cz * cy, cz * sy * sx - sz * cx, cz * sy * cx + sz * sx],
            [sz * cy, sz * sy * sx + cz * cx, sz * sy * cx - cz * sx],
            [-sy, cy * sx, cy * cx],
        ];
        pts.iter()
            .map(|p| {
                [
                    r[0][0] * p[0] + r[0][1] * p[1] + r[0][2] * p[2],
                    r[1][0] * p[0] + r[1][1] * p[1] + r[1][2] * p[2],
                    r[2][0] * p[0] + r[2][1] * p[1] + r[2][2] * p[2],
                ]
            })
            .collect()
    }

    #[test]
    fn bore_measures_torus_and_is_rotation_invariant() {
        // Bore radius = major - tube = 8.675 -> Ø17.35mm (~US 7).
        let pts = torus_points(11.175, 2.5, 96, 48);
        let base = measure_inner_bore(&pts).expect("torus bore");
        assert!(
            (base.diameter_mm - 17.35).abs() < 0.7,
            "axis-aligned Ø{:.2} expected ~17.35",
            base.diameter_mm
        );
        assert!(base.coverage > 0.85, "coverage {:.2}", base.coverage);

        // Under arbitrary rotation the size must hold and the axis must track Z.
        for (rx, ry, rz) in [(37.0, 53.0, 19.0), (80.0, 15.0, 65.0)] {
            let rp = rotate(&pts, rx, ry, rz);
            let b = measure_inner_bore(&rp).expect("rotated torus bore");
            assert!(
                (b.diameter_mm - base.diameter_mm).abs() < 0.5,
                "rotated Ø{:.2} vs base Ø{:.2}",
                b.diameter_mm,
                base.diameter_mm
            );
            let z = rotate(&[[0.0, 0.0, 1.0]], rx, ry, rz)[0];
            let dot = (b.axis_dir[0] * z[0] + b.axis_dir[1] * z[1] + b.axis_dir[2] * z[2]).abs();
            assert!(dot > 0.95, "axis·Z = {:.3}, axis {:?}", dot, b.axis_dir);
        }
    }
}
