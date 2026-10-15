//! Extrusion operation - the fundamental 3D feature

use crate::BRep;
use nova_core::Result;
use glam::DVec3;

/// Extrude a 2D profile (closed loop) to create a 3D solid
///
/// # Arguments
/// * `profile` - Closed 2D points [[x, y], ...] in order (CCW)
/// * `depth` - Extrusion depth (positive = upward along Z)
///
/// # Returns
/// A B-Rep solid representing the extruded shape
pub fn extrude_profile(profile: &[[f64; 2]], depth: f64) -> Result<BRep> {
    if profile.len() < 3 {
        return Err(nova_core::NovaError::InvalidSketch(
            "Profile must have at least 3 points".into(),
        ));
    }

    if depth.abs() < 1e-10 {
        return Err(nova_core::NovaError::InvalidFeature(
            "Depth must be non-zero".into(),
        ));
    }

    let mut brep = BRep::new();

    // Add bottom vertices (z = 0)
    let mut bottom_verts = Vec::new();
    for &[x, y] in profile {
        let idx = brep.add_vertex(DVec3::new(x, y, 0.0));
        bottom_verts.push(idx);
    }

    // Add top vertices (z = depth)
    let mut top_verts = Vec::new();
    for &[x, y] in profile {
        let idx = brep.add_vertex(DVec3::new(x, y, depth));
        top_verts.push(idx);
    }

    let n = profile.len();

    // Bottom face (z = 0, reversed for outward normal)
    let mut bottom = bottom_verts.clone();
    bottom.reverse();
    brep.add_face(bottom);

    // Top face (z = depth)
    brep.add_face(top_verts.clone());

    // Side faces (quads, tessellated as triangles)
    for i in 0..n {
        let j = (i + 1) % n;

        // Quad: bottom[i] -> bottom[j] -> top[j] -> top[i]
        // Split into two triangles
        brep.add_face(vec![bottom_verts[i], bottom_verts[j], top_verts[j]]);
        brep.add_face(vec![bottom_verts[i], top_verts[j], top_verts[i]]);
    }

    Ok(brep)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_rectangle_extrusion() {
        // Rectangle: 100 x 60 mm
        let profile = vec![
            [0.0, 0.0],
            [100.0, 0.0],
            [100.0, 60.0],
            [0.0, 60.0],
        ];

        let result = extrude_profile(&profile, 20.0);
        assert!(result.is_ok(), "Extrusion should succeed");

        let brep = result.unwrap();
        println!("OK - Extrusion succeeded");
        println!("  Vertices: {}", brep.vertices.len());
        println!("  Faces: {}", brep.faces.len());
        println!("  Volume: {} mm3", brep.volume());
        println!("  Bbox: {:?}", brep.bounding_box());

        // Check volume
        let vol = brep.volume();
        assert!(
            (vol - 120_000.0).abs() < 10.0,
            "Volume should be ~120000, got {}",
            vol
        );
    }

    #[test]
    fn test_invalid_profile() {
        let profile = vec![[0.0, 0.0], [1.0, 1.0]]; // Only 2 points
        let result = extrude_profile(&profile, 10.0);
        assert!(result.is_err(), "Should reject profile with < 3 points");
    }

    #[test]
    fn test_invalid_depth() {
        let profile = vec![
            [0.0, 0.0],
            [100.0, 0.0],
            [100.0, 60.0],
            [0.0, 60.0],
        ];
        let result = extrude_profile(&profile, 0.0);
        assert!(result.is_err(), "Should reject zero depth");
    }
}
