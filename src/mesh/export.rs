//! Mesh export functionality for STL and OBJ formats

use std::io::Write;
use byteorder::{LittleEndian, WriteBytesExt};

use super::Mesh;
use crate::database::files::ExportFormat;

/// Export a mesh to binary STL format
pub fn export_stl(mesh: &Mesh) -> anyhow::Result<Vec<u8>> {
    let mut buffer = Vec::new();
    
    // 80-byte header (padded with spaces)
    let mut header = [0u8; 80];
    let text = b"Jewelry Cost Calculator Export - Binary STL";
    header[..text.len()].copy_from_slice(text);
    // Fill rest with spaces
    for byte in header[text.len()..].iter_mut() {
        *byte = b' ';
    }
    buffer.write_all(&header)?;
    
    // Triangle count
    let triangle_count = mesh.triangle_count() as u32;
    buffer.write_u32::<LittleEndian>(triangle_count)?;
    
    // Write each triangle
    for face in &mesh.faces {
        if face.v.len() < 3 {
            continue;
        }
        
        let v0_idx = face.v[0];
        let v0 = match mesh.vertices.get(v0_idx) {
            Some(v) => v,
            None => continue,
        };
        
        // Fan triangulation for polygons
        for i in 1..(face.v.len() - 1) {
            let v1_idx = face.v[i];
            let v2_idx = face.v[i + 1];
            
            let v1 = match mesh.vertices.get(v1_idx) {
                Some(v) => v,
                None => continue,
            };
            let v2 = match mesh.vertices.get(v2_idx) {
                Some(v) => v,
                None => continue,
            };
            
            // Calculate normal
            let edge1 = v1.subtract(*v0);
            let edge2 = v2.subtract(*v0);
            let normal = edge1.cross(edge2).normalize();
            
            // Write normal
            buffer.write_f32::<LittleEndian>(normal.0)?;
            buffer.write_f32::<LittleEndian>(normal.1)?;
            buffer.write_f32::<LittleEndian>(normal.2)?;
            
            // Write vertices
            buffer.write_f32::<LittleEndian>(v0.0)?;
            buffer.write_f32::<LittleEndian>(v0.1)?;
            buffer.write_f32::<LittleEndian>(v0.2)?;
            
            buffer.write_f32::<LittleEndian>(v1.0)?;
            buffer.write_f32::<LittleEndian>(v1.1)?;
            buffer.write_f32::<LittleEndian>(v1.2)?;
            
            buffer.write_f32::<LittleEndian>(v2.0)?;
            buffer.write_f32::<LittleEndian>(v2.1)?;
            buffer.write_f32::<LittleEndian>(v2.2)?;
            
            // Attribute byte count (unused, set to 0)
            buffer.write_u16::<LittleEndian>(0)?;
        }
    }
    
    Ok(buffer)
}

/// Export a mesh to OBJ format
pub fn export_obj(mesh: &Mesh) -> anyhow::Result<Vec<u8>> {
    let mut buffer = Vec::new();
    
    // Header comment
    writeln!(buffer, "# Jewelry Cost Calculator Export")?;
    writeln!(buffer, "# Vertices: {}", mesh.vertices.len())?;
    writeln!(buffer, "# Faces: {}", mesh.faces.len())?;
    writeln!(buffer)?;
    
    // Write vertices
    for v in &mesh.vertices {
        writeln!(buffer, "v {} {} {}", v.0, v.1, v.2)?;
    }
    
    writeln!(buffer)?;
    
    // Write vertex normals if available
    if !mesh.normals.is_empty() {
        for n in &mesh.normals {
            writeln!(buffer, "vn {} {} {}", n.0, n.1, n.2)?;
        }
        writeln!(buffer)?;
    }
    
    // Write faces (OBJ uses 1-based indices)
    for face in &mesh.faces {
        if face.v.is_empty() {
            continue;
        }
        
        let mut line = String::from("f");
        
        for (i, &v_idx) in face.v.iter().enumerate() {
            let v = v_idx + 1; // 1-based index
            
            // Check if we have matching normals
            if !face.vn.is_empty() && i < face.vn.len() {
                let vn = face.vn[i] + 1;
                line.push_str(&format!(" {}//{}", v, vn));
            } else {
                line.push_str(&format!(" {}", v));
            }
        }
        
        writeln!(buffer, "{}", line)?;
    }
    
    Ok(buffer)
}

/// Export mesh in the specified format
pub fn export_mesh(mesh: &Mesh, format: ExportFormat) -> anyhow::Result<Vec<u8>> {
    match format {
        ExportFormat::STL => export_stl(mesh),
        ExportFormat::OBJ => export_obj(mesh),
    }
}

/// Create a scaled copy of the mesh and export it
pub fn export_scaled_mesh(
    mesh: &Mesh, 
    scale_factor: f64, 
    format: ExportFormat
) -> anyhow::Result<Vec<u8>> {
    // Create a scaled copy
    let mut scaled_mesh = mesh.clone();
    
    // Get the center for scaling around center
    let (min_v, max_v) = scaled_mesh.bounds()?;
    let center_x = (min_v.0 + max_v.0) / 2.0;
    let center_y = (min_v.1 + max_v.1) / 2.0;
    let center_z = (min_v.2 + max_v.2) / 2.0;
    
    // Scale around center
    let scale = scale_factor as f32;
    for v in &mut scaled_mesh.vertices {
        v.0 = (v.0 - center_x) * scale + center_x;
        v.1 = (v.1 - center_y) * scale + center_y;
        v.2 = (v.2 - center_z) * scale + center_z;
    }
    
    export_mesh(&scaled_mesh, format)
}

/// Export multiple ring sizes
pub fn export_ring_sizes(
    mesh: &Mesh,
    current_diameter: f64,
    target_sizes: &[crate::ring_sizing::RingSize],
    format: ExportFormat,
) -> anyhow::Result<Vec<(crate::ring_sizing::RingSize, Vec<u8>)>> {
    let mut exports = Vec::with_capacity(target_sizes.len());
    
    for size in target_sizes {
        let target_diameter = size.inner_diameter_mm();
        let scale_factor = target_diameter / current_diameter;
        
        let data = export_scaled_mesh(mesh, scale_factor, format)?;
        exports.push((*size, data));
    }
    
    Ok(exports)
}
