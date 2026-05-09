//! Volume calculation for 3D meshes using the signed tetrahedra method
//!
//! The volume is calculated by summing the signed volumes of tetrahedra
//! formed by each triangle and the origin. This method works for any
//! closed (watertight) mesh regardless of its position.

use rayon::prelude::*;

use super::{Face, Mesh, Triangle, Vec3};

/// Calculate the volume of a mesh in cubic units (mm³ if vertices are in mm)
/// Uses the signed tetrahedra method with Kahan summation for numerical stability
pub fn calculate_volume(mesh: &Mesh) -> f64 {
    if mesh.faces.is_empty() {
        return 0.0;
    }

    const PARALLEL_THRESHOLD: usize = 1000;
    const CHUNK_SIZE: usize = 1000;

    let total_volume: f64 = if mesh.faces.len() >= PARALLEL_THRESHOLD {
        mesh.faces
            .par_chunks(CHUNK_SIZE)
            .map(|chunk| kahan_sum_faces(chunk, &mesh.vertices))
            .sum()
    } else {
        kahan_sum_faces(&mesh.faces, &mesh.vertices)
    };

    total_volume.abs()
}

/// Calculate volume using Kahan summation algorithm for better numerical precision
#[inline]
fn kahan_sum_faces(faces: &[Face], vertices: &[Vec3]) -> f64 {
    let mut sum = 0.0_f64;
    let mut compensation = 0.0_f64;

    for face in faces {
        let indices = &face.v;
        let n = indices.len();
        if n < 3 {
            continue;
        }

        let v0 = vertices[indices[0]];

        // Fan triangulation: connect v0 to each pair of adjacent vertices
        for i in 1..(n - 1) {
            let v1 = vertices[indices[i]];
            let v2 = vertices[indices[i + 1]];

            let volume = Triangle {
                vertices: [v0, v1, v2],
            }
            .signed_volume();

            // Kahan summation
            let y = volume - compensation;
            let t = sum + y;
            compensation = (t - sum) - y;
            sum = t;
        }
    }

    sum
}

/// Calculate volume in cubic centimeters (cm³)
/// Assumes input mesh vertices are in millimeters
pub fn calculate_volume_cm3(mesh: &Mesh) -> f64 {
    // 1 cm³ = 1000 mm³
    calculate_volume(mesh) / 1000.0
}

/// Calculate the surface area of a mesh
pub fn calculate_surface_area(mesh: &Mesh) -> f64 {
    if mesh.faces.is_empty() {
        return 0.0;
    }

    mesh.faces
        .par_iter()
        .map(|face| {
            let indices = &face.v;
            let n = indices.len();
            if n < 3 {
                return 0.0;
            }

            let v0 = mesh.vertices[indices[0]];
            let mut area = 0.0;

            for i in 1..(n - 1) {
                let v1 = mesh.vertices[indices[i]];
                let v2 = mesh.vertices[indices[i + 1]];

                let a = v1.subtract(v0);
                let b = v2.subtract(v0);
                let cross = a.cross(b);

                // Area of triangle = 0.5 * |a × b|
                area += cross.length() as f64 / 2.0;
            }

            area
        })
        .sum()
}

/// Calculate the center of mass of a mesh (assuming uniform density)
pub fn calculate_centroid(mesh: &Mesh) -> Vec3 {
    if mesh.vertices.is_empty() {
        return Vec3::default();
    }

    let (sum_x, sum_y, sum_z): (f64, f64, f64) = mesh
        .vertices
        .par_iter()
        .map(|v| (v.0 as f64, v.1 as f64, v.2 as f64))
        .reduce(
            || (0.0, 0.0, 0.0),
            |(ax, ay, az), (bx, by, bz)| (ax + bx, ay + by, az + bz),
        );

    let n = mesh.vertices.len() as f64;
    Vec3(
        (sum_x / n) as f32,
        (sum_y / n) as f32,
        (sum_z / n) as f32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::Face;
    use smallvec::smallvec;

    fn create_unit_cube() -> Mesh {
        // Create a unit cube (1x1x1) centered at origin
        let mut mesh = Mesh::new();

        // 8 vertices of the cube
        mesh.vertices = vec![
            Vec3(-0.5, -0.5, -0.5),
            Vec3(0.5, -0.5, -0.5),
            Vec3(0.5, 0.5, -0.5),
            Vec3(-0.5, 0.5, -0.5),
            Vec3(-0.5, -0.5, 0.5),
            Vec3(0.5, -0.5, 0.5),
            Vec3(0.5, 0.5, 0.5),
            Vec3(-0.5, 0.5, 0.5),
        ];

        // 12 triangles (2 per face)
        mesh.faces = vec![
            // Front face
            Face { v: smallvec![0, 1, 2], vn: smallvec![], vt: smallvec![] },
            Face { v: smallvec![0, 2, 3], vn: smallvec![], vt: smallvec![] },
            // Back face
            Face { v: smallvec![5, 4, 7], vn: smallvec![], vt: smallvec![] },
            Face { v: smallvec![5, 7, 6], vn: smallvec![], vt: smallvec![] },
            // Top face
            Face { v: smallvec![3, 2, 6], vn: smallvec![], vt: smallvec![] },
            Face { v: smallvec![3, 6, 7], vn: smallvec![], vt: smallvec![] },
            // Bottom face
            Face { v: smallvec![4, 5, 1], vn: smallvec![], vt: smallvec![] },
            Face { v: smallvec![4, 1, 0], vn: smallvec![], vt: smallvec![] },
            // Right face
            Face { v: smallvec![1, 5, 6], vn: smallvec![], vt: smallvec![] },
            Face { v: smallvec![1, 6, 2], vn: smallvec![], vt: smallvec![] },
            // Left face
            Face { v: smallvec![4, 0, 3], vn: smallvec![], vt: smallvec![] },
            Face { v: smallvec![4, 3, 7], vn: smallvec![], vt: smallvec![] },
        ];

        mesh
    }

    #[test]
    fn test_unit_cube_volume() {
        let cube = create_unit_cube();
        let volume = calculate_volume(&cube);
        // Unit cube has volume of 1
        assert!((volume - 1.0).abs() < 1e-6);
    }
}
