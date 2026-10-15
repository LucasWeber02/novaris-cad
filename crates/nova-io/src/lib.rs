//! NOVARIS CAD — Part files (.nvcad) and STEP/STL export
//!
//! A `.nvcad` file stores the parametric history (sketches and features) as
//! JSON, not the resulting geometry: opening a part rebuilds it, exactly
//! like SolidWorks replays its FeatureManager tree.

use nova_core::{NovaError, Result};
use nova_features::FeatureTree;
use nova_kernel::Solid;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;

/// File format marker and current version
pub const FORMAT: &str = "nvcad-part";
pub const FORMAT_VERSION: u32 = 1;

/// A part document: a name and its feature history
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PartDocument {
    pub name: String,
    pub tree: FeatureTree,
}

#[derive(Serialize, Deserialize)]
struct PartFile {
    format: String,
    version: u32,
    #[serde(flatten)]
    part: PartDocument,
}

impl PartDocument {
    pub fn new(name: impl Into<String>) -> Self {
        PartDocument {
            name: name.into(),
            tree: FeatureTree::new(),
        }
    }

    /// Serialise to the .nvcad JSON text
    pub fn to_json(&self) -> Result<String> {
        let file = PartFile {
            format: FORMAT.into(),
            version: FORMAT_VERSION,
            part: self.clone(),
        };
        serde_json::to_string_pretty(&file).map_err(|e| NovaError::DocumentError(e.to_string()))
    }

    /// Parse .nvcad JSON text and rebuild the part
    pub fn from_json(text: &str) -> Result<Self> {
        let mut part = Self::from_json_unbuilt(text)?;
        part.tree.rebuild();
        Ok(part)
    }

    /// Parse .nvcad JSON text without rebuilding the geometry yet
    pub fn from_json_unbuilt(text: &str) -> Result<Self> {
        let header: serde_json::Value =
            serde_json::from_str(text).map_err(|e| NovaError::DocumentError(format!("not a valid part file: {e}")))?;
        if header.get("format").and_then(|f| f.as_str()) != Some(FORMAT) {
            return Err(NovaError::DocumentError("not a NOVARIS part file".into()));
        }
        let version = header.get("version").and_then(|v| v.as_u64()).unwrap_or(0);
        if version > FORMAT_VERSION as u64 {
            return Err(NovaError::DocumentError(format!(
                "part file version {version} is newer than this program supports ({FORMAT_VERSION})"
            )));
        }
        let file: PartFile =
            serde_json::from_value(header).map_err(|e| NovaError::DocumentError(format!("corrupt part file: {e}")))?;
        Ok(file.part)
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        std::fs::write(path, self.to_json()?)?;
        Ok(())
    }

    /// Read and rebuild a part file
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_json(&std::fs::read_to_string(path)?)
    }

    /// Read a part file; call `tree.rebuild()` before using its geometry
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_json_unbuilt(&std::fs::read_to_string(path)?)
    }
}

/// Write a solid as a STEP AP203 file
pub fn export_step(solid: &Solid, path: impl AsRef<Path>) -> Result<()> {
    solid.write_step(path.as_ref())
}

/// Binary STL bytes; `tol` is the chord tolerance in mm
pub fn stl_bytes(solid: &Solid, tol: f64) -> Vec<u8> {
    let mesh = solid.tessellate(tol);
    let count = mesh.indices.len() / 3;
    let mut out = Vec::with_capacity(84 + count * 50);
    let mut header = [0u8; 80];
    let tag = b"NOVARIS CAD binary STL";
    header[..tag.len()].copy_from_slice(tag);
    out.extend_from_slice(&header);
    out.extend_from_slice(&(count as u32).to_le_bytes());
    for tri in mesh.indices.chunks_exact(3) {
        let p = |i: u32| glam::Vec3::from(mesh.positions[i as usize]);
        let (a, b, c) = (p(tri[0]), p(tri[1]), p(tri[2]));
        let n = (b - a).cross(c - a).normalize_or_zero();
        for v in [n, a, b, c] {
            for f in v.to_array() {
                out.extend_from_slice(&f.to_le_bytes());
            }
        }
        out.extend_from_slice(&0u16.to_le_bytes());
    }
    out
}

/// Write a solid as a binary STL file
pub fn export_stl(solid: &Solid, path: impl AsRef<Path>, tol: f64) -> Result<()> {
    let mut f = std::fs::File::create(path)?;
    f.write_all(&stl_bytes(solid, tol))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_features::{EndCondition, Extrude, ExtrudeOp};
    use nova_sketch::Sketch;

    fn plate() -> PartDocument {
        let mut doc = PartDocument::new("Plaque");
        let mut s = Sketch::new("");
        s.add_rectangle(0.0, 0.0, 100.0, 60.0).unwrap();
        let c = s.add_point(50.0, 30.0);
        s.add_circle(c, 10.0).unwrap();
        let sk = doc.tree.add_sketch(s);
        doc.tree
            .add_extrude(Extrude { sketch: sk, end: EndCondition::Blind { depth: 20.0 }, reverse: false, op: ExtrudeOp::Boss })
            .unwrap();
        doc.tree.rebuild();
        doc
    }

    #[test]
    fn save_and_reopen_gives_same_part() {
        let doc = plate();
        let v = doc.tree.body().unwrap().volume();
        let dir = std::env::temp_dir().join(format!("nvcad-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("plaque.nvcad");
        doc.save(&path).unwrap();
        let back = PartDocument::open(&path).unwrap();
        assert_eq!(back.name, "Plaque");
        assert_eq!(back.tree.len(), 2);
        assert!((back.tree.body().unwrap().volume() - v).abs() < 1e-6);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn rejects_foreign_or_newer_files() {
        assert!(PartDocument::from_json("{}").is_err());
        assert!(PartDocument::from_json("not json").is_err());
        let newer = plate().to_json().unwrap().replace("\"version\": 1", "\"version\": 99");
        assert!(PartDocument::from_json(&newer).unwrap_err().to_string().contains("newer"));
    }

    #[test]
    fn stl_has_expected_layout() {
        let doc = plate();
        let bytes = stl_bytes(doc.tree.body().unwrap(), 0.1);
        let count = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
        assert!(count > 12);
        assert_eq!(bytes.len(), 84 + count * 50);
    }

    #[test]
    fn step_file_is_a_closed_solid() {
        let doc = plate();
        let dir = std::env::temp_dir().join(format!("nvcad-step-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("plaque.step");
        export_step(doc.tree.body().unwrap(), &path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("ISO-10303-21;"));
        assert_eq!(text.matches("MANIFOLD_SOLID_BREP").count(), 1);
        assert!(text.contains("CYLINDRICAL_SURFACE"), "the hole stays an exact cylinder");
        std::fs::remove_dir_all(dir).ok();
    }
}
