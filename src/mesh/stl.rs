//! STL file parser supporting both binary and ASCII formats
//!
//! STL Binary Format:
//! - Bytes 0-79: 80-byte header
//! - Bytes 80-83: 4-byte unsigned int (number of triangles)
//! - Bytes 84-end: Triangle data (50 bytes per triangle)
//!   - 12 bytes: normal vector (3 * f32)
//!   - 36 bytes: 3 vertices (3 * 3 * f32)
//!   - 2 bytes: attribute byte count (usually 0)

use std::sync::Arc;

use anyhow::Result;
use parking_lot::RwLock;
use rayon::prelude::*;

use super::{Face, Mesh, MeshCache, Vec3, MAX_TRIANGLES};

/// Parse an STL file from bytes (auto-detects binary vs ASCII)
pub fn parse(bytes: &[u8]) -> Result<Mesh> {
    puffin::profile_function!();
    
    if is_ascii(bytes) {
        parse_ascii(bytes)
    } else {
        parse_binary(bytes)
    }
}

/// Check if the STL file is ASCII format
fn is_ascii(bytes: &[u8]) -> bool {
    if !bytes.starts_with(b"solid") {
        return false;
    }

    // Check for "facet" keyword in the first 1KB
    let check_len = bytes.len().min(1024);
    if let Ok(header) = std::str::from_utf8(&bytes[..check_len]) {
        header.contains("facet")
    } else {
        false
    }
}

/// Parse a binary STL file with parallel processing
fn parse_binary(bytes: &[u8]) -> Result<Mesh> {
    puffin::profile_function!();
    
    if bytes.len() < 84 {
        return Err(anyhow::anyhow!("Binary STL file too small"));
    }

    let declared_count = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
    
    // Calculate actual available triangles based on file size
    let data_len = bytes.len().saturating_sub(84);
    let physical_count = data_len / 50;

    let triangle_count = if declared_count == 0 || declared_count > physical_count {
        physical_count
    } else {
        declared_count
    };

    if triangle_count > MAX_TRIANGLES as usize {
        return Err(anyhow::anyhow!(
            "Too many triangles: {} (max {})",
            triangle_count,
            MAX_TRIANGLES
        ));
    }

    // Parse triangles in parallel chunks
    let triangle_data = &bytes[84..];
    
    // Process in chunks for better cache locality
    const CHUNK_SIZE: usize = 10000;
    let chunks: Vec<_> = (0..triangle_count)
        .collect::<Vec<_>>()
        .par_chunks(CHUNK_SIZE)
        .map(|chunk_indices| {
            let mut vertices = Vec::with_capacity(chunk_indices.len() * 3);
            let mut faces = Vec::with_capacity(chunk_indices.len());
            
            for &i in chunk_indices {
                let offset = i * 50;
                if offset + 50 > triangle_data.len() {
                    continue;
                }
                
                // Skip normal (12 bytes), read 3 vertices (36 bytes), skip attribute (2 bytes)
                let tri_bytes = &triangle_data[offset..offset + 50];
                
                let mut face = Face::default();
                let base_idx = vertices.len();
                
                for v in 0..3 {
                    let v_offset = 12 + v * 12; // Skip normal, then vertex data
                    let x = f32::from_le_bytes([
                        tri_bytes[v_offset],
                        tri_bytes[v_offset + 1],
                        tri_bytes[v_offset + 2],
                        tri_bytes[v_offset + 3],
                    ]);
                    let y = f32::from_le_bytes([
                        tri_bytes[v_offset + 4],
                        tri_bytes[v_offset + 5],
                        tri_bytes[v_offset + 6],
                        tri_bytes[v_offset + 7],
                    ]);
                    let z = f32::from_le_bytes([
                        tri_bytes[v_offset + 8],
                        tri_bytes[v_offset + 9],
                        tri_bytes[v_offset + 10],
                        tri_bytes[v_offset + 11],
                    ]);
                    
                    vertices.push(Vec3(x, y, z));
                    face.v.push(base_idx + v);
                }
                
                faces.push(face);
            }
            
            (vertices, faces)
        })
        .collect();
    
    // Combine all chunks
    let total_vertices: usize = chunks.iter().map(|(v, _)| v.len()).sum();
    let total_faces: usize = chunks.iter().map(|(_, f)| f.len()).sum();
    
    let mut mesh = Mesh {
        vertices: Vec::with_capacity(total_vertices),
        normals: Vec::new(),
        textures: Vec::new(),
        faces: Vec::with_capacity(total_faces),
        groups: Vec::new(),
        matlibs: Vec::new(),
        cache: Arc::new(RwLock::new(MeshCache::default())),
    };
    
    let mut vertex_offset = 0;
    for (vertices, faces) in chunks {
        // Adjust face indices
        for mut face in faces {
            for idx in &mut face.v {
                *idx += vertex_offset;
            }
            mesh.faces.push(face);
        }
        mesh.vertices.extend(vertices.iter());
        vertex_offset += vertices.len();
    }

    Ok(mesh)
}

/// Parse an ASCII STL file with parallel processing
fn parse_ascii(bytes: &[u8]) -> Result<Mesh> {
    puffin::profile_function!();
    
    let content = std::str::from_utf8(bytes)?;
    
    // Find all "facet" blocks to enable parallel parsing
    // Each facet block contains one triangle
    let facet_starts: Vec<usize> = {
        puffin::profile_scope!("find_facet_blocks");
        content
            .match_indices("facet normal")
            .map(|(idx, _)| idx)
            .collect()
    };
    
    if facet_starts.is_empty() {
        // Fallback to sequential parsing for malformed files
        return parse_ascii_sequential(content);
    }
    
    // Parse facets in parallel
    let parsed_triangles: Vec<Option<(Vec<Vec3>, Face)>> = {
        puffin::profile_scope!("parse_facets_parallel");
        
        facet_starts
            .par_iter()
            .enumerate()
            .map(|(i, &start)| {
                // Find the end of this facet block
                let end = if i + 1 < facet_starts.len() {
                    facet_starts[i + 1]
                } else {
                    content.len()
                };
                
                let block = &content[start..end];
                let mut vertices = Vec::with_capacity(3);
                
                for line in block.lines() {
                    let line = line.trim();
                    if line.starts_with("vertex") {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() == 4 {
                            if let (Ok(x), Ok(y), Ok(z)) = (
                                parts[1].parse::<f32>(),
                                parts[2].parse::<f32>(),
                                parts[3].parse::<f32>(),
                            ) {
                                vertices.push(Vec3(x, y, z));
                            }
                        }
                    }
                }
                
                if vertices.len() >= 3 {
                    let face = Face {
                        v: smallvec::smallvec![0, 1, 2],
                        vn: smallvec::SmallVec::new(),
                        vt: smallvec::SmallVec::new(),
                    };
                    Some((vertices, face))
                } else {
                    None
                }
            })
            .collect()
    };
    
    // Combine results
    let valid_triangles: Vec<_> = parsed_triangles.into_iter().flatten().collect();
    
    if valid_triangles.len() > MAX_TRIANGLES as usize {
        return Err(anyhow::anyhow!(
            "Too many triangles: {} (max {})",
            valid_triangles.len(),
            MAX_TRIANGLES
        ));
    }
    
    // Build final mesh
    let total_vertices = valid_triangles.len() * 3;
    let mut mesh = Mesh {
        vertices: Vec::with_capacity(total_vertices),
        normals: Vec::new(),
        textures: Vec::new(),
        faces: Vec::with_capacity(valid_triangles.len()),
        groups: Vec::new(),
        matlibs: Vec::new(),
        cache: Arc::new(RwLock::new(MeshCache::default())),
    };
    
    for (vertices, mut face) in valid_triangles {
        let base_idx = mesh.vertices.len();
        mesh.vertices.extend(vertices);
        // Adjust face indices
        for idx in &mut face.v {
            *idx += base_idx;
        }
        mesh.faces.push(face);
    }
    
    Ok(mesh)
}

/// Sequential ASCII parsing fallback for malformed files
fn parse_ascii_sequential(content: &str) -> Result<Mesh> {
    let mut mesh = Mesh {
        vertices: Vec::new(),
        normals: Vec::new(),
        textures: Vec::new(),
        faces: Vec::new(),
        groups: Vec::new(),
        matlibs: Vec::new(),
        cache: Arc::new(RwLock::new(MeshCache::default())),
    };
    let mut face = Face::default();

    for line in content.lines() {
        let line = line.trim();

        if line.starts_with("vertex") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() == 4 {
                if let (Ok(x), Ok(y), Ok(z)) = (
                    parts[1].parse::<f32>(),
                    parts[2].parse::<f32>(),
                    parts[3].parse::<f32>(),
                ) {
                    mesh.vertices.push(Vec3(x, y, z));
                    face.v.push(mesh.vertices.len() - 1);
                }
            }
        } else if (line.starts_with("endfacet") || line.starts_with("endloop"))
            && !face.v.is_empty()
        {
            mesh.faces.push(face);
            face = Face::default();
        }
    }

    if mesh.faces.len() > MAX_TRIANGLES as usize {
        return Err(anyhow::anyhow!(
            "Too many triangles: {} (max {})",
            mesh.faces.len(),
            MAX_TRIANGLES
        ));
    }

    Ok(mesh)
}

/// Validate STL bytes without fully parsing
pub fn validate_bytes(bytes: &[u8]) -> bool {
    if is_ascii(bytes) {
        return true;
    }

    // Binary STL must have at least header + count
    if bytes.len() < 84 {
        return false;
    }

    let triangle_count = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
    let data_len = bytes.len() - 84;

    if triangle_count > MAX_TRIANGLES as usize {
        return false;
    }

    // Allow zero triangle count if there's still data (common export bug)
    if triangle_count == 0 {
        return data_len >= 50;
    }

    let expected_min_data = triangle_count * 50;
    data_len >= expected_min_data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_ascii() {
        assert!(is_ascii(b"solid cube\nfacet normal 0 0 1\n"));
        assert!(!is_ascii(b"\x00\x00\x00\x00"));
        assert!(!is_ascii(b"solid")); // No facet keyword
    }
}
