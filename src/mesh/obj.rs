//! OBJ (Wavefront) file parser
//!
//! OBJ Format Elements:
//! - v x y z: Vertex position
//! - vt u v: Texture coordinate
//! - vn x y z: Vertex normal
//! - f v1/vt1/vn1 v2/vt2/vn2 v3/vt3/vn3: Face definition
//! - o name: Object name
//! - g name: Group name
//! - mtllib file.mtl: Material library reference
//! - usemtl name: Use material

use anyhow::Result;

use super::{Face, Group, Mesh, Vec2, Vec3, MAX_TRIANGLES};

/// Parse an OBJ file from bytes
pub fn parse(bytes: &[u8]) -> Result<Mesh> {
    let content = std::str::from_utf8(bytes)?;
    let mut mesh = Mesh::default();

    let mut current_group: Option<String> = None;
    let mut current_material: Option<String> = None;
    let mut group_start_face: usize = 0;

    for line in content.lines() {
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        match parts[0] {
            "v" => {
                // Vertex position
                if parts.len() >= 4 {
                    if let (Ok(x), Ok(y), Ok(z)) = (
                        parts[1].parse::<f32>(),
                        parts[2].parse::<f32>(),
                        parts[3].parse::<f32>(),
                    ) {
                        mesh.vertices.push(Vec3(x, y, z));
                    }
                }
            }
            "vt" => {
                // Texture coordinate
                if parts.len() >= 3 {
                    if let (Ok(u), Ok(v)) = (parts[1].parse::<f32>(), parts[2].parse::<f32>()) {
                        mesh.textures.push(Vec2(u, v));
                    }
                }
            }
            "vn" => {
                // Vertex normal
                if parts.len() >= 4 {
                    if let (Ok(x), Ok(y), Ok(z)) = (
                        parts[1].parse::<f32>(),
                        parts[2].parse::<f32>(),
                        parts[3].parse::<f32>(),
                    ) {
                        mesh.normals.push(Vec3(x, y, z));
                    }
                }
            }
            "f" => {
                // Face definition
                if parts.len() >= 4 {
                    let mut face = Face::default();

                    for part in &parts[1..] {
                        if let Some((vi, vti, vni)) = parse_face_vertex(part) {
                            // OBJ indices are 1-based
                            if vi > 0 && vi <= mesh.vertices.len() {
                                face.v.push(vi - 1);
                            }
                            if let Some(vt) = vti {
                                if vt > 0 && vt <= mesh.textures.len() {
                                    face.vt.push(vt - 1);
                                }
                            }
                            if let Some(vn) = vni {
                                if vn > 0 && vn <= mesh.normals.len() {
                                    face.vn.push(vn - 1);
                                }
                            }
                        }
                    }

                    if face.v.len() >= 3 {
                        mesh.faces.push(face);
                    }
                }
            }
            "g" | "o" => {
                // Group or object name
                if parts.len() >= 2 {
                    // Save previous group
                    if let Some(name) = current_group.take() {
                        if mesh.faces.len() > group_start_face {
                            mesh.groups.push(Group {
                                name,
                                material: current_material.clone(),
                                face_range: group_start_face..mesh.faces.len(),
                            });
                        }
                    }
                    current_group = Some(parts[1..].join(" "));
                    group_start_face = mesh.faces.len();
                }
            }
            "mtllib" => {
                // Material library
                if parts.len() >= 2 {
                    mesh.matlibs.push(parts[1..].join(" "));
                }
            }
            "usemtl" => {
                // Use material
                if parts.len() >= 2 {
                    current_material = Some(parts[1..].join(" "));
                }
            }
            _ => {
                // Ignore unknown directives
            }
        }
    }

    // Save final group
    if let Some(name) = current_group {
        if mesh.faces.len() > group_start_face {
            mesh.groups.push(Group {
                name,
                material: current_material,
                face_range: group_start_face..mesh.faces.len(),
            });
        }
    }

    // Validate triangle count
    let tri_count = mesh.triangle_count();
    if tri_count > MAX_TRIANGLES as usize {
        return Err(anyhow::anyhow!(
            "Too many triangles: {} (max {})",
            tri_count,
            MAX_TRIANGLES
        ));
    }

    Ok(mesh)
}

/// Parse a face vertex specification (v/vt/vn format)
fn parse_face_vertex(s: &str) -> Option<(usize, Option<usize>, Option<usize>)> {
    let parts: Vec<&str> = s.split('/').collect();

    let vi = parts.first()?.parse::<usize>().ok()?;

    let vti = parts
        .get(1)
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse::<usize>().ok());

    let vni = parts
        .get(2)
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse::<usize>().ok());

    Some((vi, vti, vni))
}

/// Validate OBJ bytes without fully parsing
pub fn validate_bytes(bytes: &[u8]) -> bool {
    let preview = &bytes[..bytes.len().min(4096)];
    if let Ok(content) = std::str::from_utf8(preview) {
        content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .take(50)
            .any(|line| {
                let line = line.trim_start();
                line.starts_with("v ")
                    || line.starts_with("vt ")
                    || line.starts_with("vn ")
                    || line.starts_with("f ")
            })
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_face_vertex() {
        assert_eq!(parse_face_vertex("1"), Some((1, None, None)));
        assert_eq!(parse_face_vertex("1/2"), Some((1, Some(2), None)));
        assert_eq!(parse_face_vertex("1/2/3"), Some((1, Some(2), Some(3))));
        assert_eq!(parse_face_vertex("1//3"), Some((1, None, Some(3))));
    }

    #[test]
    fn test_parse_simple_obj() {
        let obj_data = b"
v 0 0 0
v 1 0 0
v 1 1 0
v 0 1 0
f 1 2 3
f 1 3 4
";
        let mesh = parse(obj_data).unwrap();
        assert_eq!(mesh.vertices.len(), 4);
        assert_eq!(mesh.faces.len(), 2);
    }
}
