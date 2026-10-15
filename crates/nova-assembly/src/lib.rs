// NOVARIS CAD — Phase 3: Assembly System
// Multi-body geometry, part positioning, constraints

use std::collections::HashMap;
use nova_core::{Id, Error, Result};
use nova_math::{Vec3, Matrix4};
use nova_geometry::BRep;

// ═══════════════════════════════════════════════════════════
// PART — Individual solid with transformation
// ═══════════════════════════════════════════════════════════

#[derive(Clone, Debug)]
pub struct Part {
    pub id: Id,
    pub name: String,
    pub geometry: BRep,
    pub transform: Matrix4,
    pub visible: bool,
    pub material: Material,
}

impl Part {
    pub fn new(id: Id, name: String, geometry: BRep) -> Self {
        Part {
            id,
            name,
            geometry,
            transform: Matrix4::identity(),
            visible: true,
            material: Material::default(),
        }
    }

    pub fn set_position(&mut self, x: f64, y: f64, z: f64) {
        self.transform = Matrix4::translation(Vec3::new(x, y, z));
    }

    pub fn set_rotation(&mut self, rx: f64, ry: f64, rz: f64) {
        let rot_x = Matrix4::rotation_x(rx);
        let rot_y = Matrix4::rotation_y(ry);
        let rot_z = Matrix4::rotation_z(rz);
        self.transform = rot_z.multiply(&rot_y.multiply(&rot_x));
    }

    pub fn get_world_vertices(&self) -> Vec<Vec3> {
        self.geometry
            .vertices
            .iter()
            .map(|v| self.transform.transform_point(*v))
            .collect()
    }

    pub fn get_volume(&self) -> f64 {
        self.geometry.volume()
    }

    pub fn get_bounds(&self) -> (Vec3, Vec3) {
        let vertices = self.get_world_vertices();
        if vertices.is_empty() {
            return (Vec3::zero(), Vec3::zero());
        }

        let mut min = vertices[0];
        let mut max = vertices[0];

        for v in vertices.iter().skip(1) {
            min = Vec3::new(min.x.min(v.x), min.y.min(v.y), min.z.min(v.z));
            max = Vec3::new(max.x.max(v.x), max.y.max(v.y), max.z.max(v.z));
        }

        (min, max)
    }
}

#[derive(Clone, Debug)]
pub struct Material {
    pub name: String,
    pub color: [f32; 4],
    pub roughness: f32,
    pub metallic: f32,
}

impl Default for Material {
    fn default() -> Self {
        Material {
            name: "Default".to_string(),
            color: [0.8, 0.8, 0.8, 1.0],
            roughness: 0.5,
            metallic: 0.0,
        }
    }
}

// ═══════════════════════════════════════════════════════════
// ASSEMBLY CONSTRAINT — Positional relationships
// ═══════════════════════════════════════════════════════════

#[derive(Clone, Debug, PartialEq)]
pub enum ConstraintType {
    Coincident,     // Parts touch
    Parallel,       // Surfaces parallel
    Perpendicular,  // Surfaces at 90°
    Distance,       // Fixed distance apart
    Angle,          // Specific angle between surfaces
}

#[derive(Clone, Debug)]
pub struct AssemblyConstraint {
    pub id: Id,
    pub constraint_type: ConstraintType,
    pub part1_id: Id,
    pub part2_id: Id,
    pub value: f64,  // Distance or angle
    pub active: bool,
}

impl AssemblyConstraint {
    pub fn new(
        id: Id,
        constraint_type: ConstraintType,
        part1_id: Id,
        part2_id: Id,
    ) -> Self {
        AssemblyConstraint {
            id,
            constraint_type,
            part1_id,
            part2_id,
            value: 0.0,
            active: true,
        }
    }

    pub fn with_value(mut self, value: f64) -> Self {
        self.value = value;
        self
    }
}

// ═══════════════════════════════════════════════════════════
// ASSEMBLY — Collection of parts with constraints
// ═══════════════════════════════════════════════════════════

#[derive(Clone, Debug)]
pub struct Assembly {
    pub id: Id,
    pub name: String,
    pub parts: HashMap<Id, Part>,
    pub constraints: HashMap<Id, AssemblyConstraint>,
    pub root_part_id: Option<Id>,
}

impl Assembly {
    pub fn new(id: Id, name: String) -> Self {
        Assembly {
            id,
            name,
            parts: HashMap::new(),
            constraints: HashMap::new(),
            root_part_id: None,
        }
    }

    pub fn add_part(&mut self, part: Part) {
        if self.root_part_id.is_none() {
            self.root_part_id = Some(part.id);
        }
        self.parts.insert(part.id, part);
    }

    pub fn remove_part(&mut self, part_id: Id) -> Option<Part> {
        self.parts.remove(&part_id)
    }

    pub fn get_part(&self, part_id: Id) -> Option<&Part> {
        self.parts.get(&part_id)
    }

    pub fn get_part_mut(&mut self, part_id: Id) -> Option<&mut Part> {
        self.parts.get_mut(&part_id)
    }

    pub fn add_constraint(&mut self, constraint: AssemblyConstraint) {
        self.constraints.insert(constraint.id, constraint);
    }

    pub fn part_count(&self) -> usize {
        self.parts.len()
    }

    pub fn constraint_count(&self) -> usize {
        self.constraints.len()
    }

    pub fn get_total_volume(&self) -> f64 {
        self.parts.values().map(|p| p.get_volume()).sum()
    }

    pub fn get_assembly_bounds(&self) -> (Vec3, Vec3) {
        if self.parts.is_empty() {
            return (Vec3::zero(), Vec3::zero());
        }

        let mut min = Vec3::new(f64::MAX, f64::MAX, f64::MAX);
        let mut max = Vec3::new(f64::MIN, f64::MIN, f64::MIN);

        for part in self.parts.values() {
            let (pmin, pmax) = part.get_bounds();
            min = Vec3::new(
                min.x.min(pmin.x),
                min.y.min(pmin.y),
                min.z.min(pmin.z),
            );
            max = Vec3::new(
                max.x.max(pmax.x),
                max.y.max(pmax.y),
                max.z.max(pmax.z),
            );
        }

        (min, max)
    }
}

// ═══════════════════════════════════════════════════════════
// BILL OF MATERIALS (BOM)
// ═══════════════════════════════════════════════════════════

#[derive(Clone, Debug)]
pub struct BomItem {
    pub part_id: Id,
    pub part_name: String,
    pub quantity: usize,
    pub volume: f64,
    pub material: String,
}

#[derive(Clone, Debug)]
pub struct BillOfMaterials {
    pub items: Vec<BomItem>,
    pub total_volume: f64,
}

pub fn generate_bom(assembly: &Assembly) -> BillOfMaterials {
    let mut items = Vec::new();
    let mut total_volume = 0.0;

    for part in assembly.parts.values() {
        let volume = part.get_volume();
        total_volume += volume;

        items.push(BomItem {
            part_id: part.id,
            part_name: part.name.clone(),
            quantity: 1,
            volume,
            material: part.material.name.clone(),
        });
    }

    BillOfMaterials {
        items,
        total_volume,
    }
}

// ═══════════════════════════════════════════════════════════
// COLLISION DETECTION (basic AABB)
// ═══════════════════════════════════════════════════════════

pub fn check_collision(part1: &Part, part2: &Part) -> bool {
    let (min1, max1) = part1.get_bounds();
    let (min2, max2) = part2.get_bounds();

    // AABB (Axis-Aligned Bounding Box) collision
    min1.x <= max2.x
        && max1.x >= min2.x
        && min1.y <= max2.y
        && max1.y >= min2.y
        && min1.z <= max2.z
        && max1.z >= min2.z
}

pub fn find_collisions(assembly: &Assembly) -> Vec<(Id, Id)> {
    let mut collisions = Vec::new();
    let part_ids: Vec<Id> = assembly.parts.keys().copied().collect();

    for i in 0..part_ids.len() {
        for j in (i + 1)..part_ids.len() {
            if let (Some(p1), Some(p2)) = (
                assembly.get_part(part_ids[i]),
                assembly.get_part(part_ids[j]),
            ) {
                if check_collision(p1, p2) {
                    collisions.push((part_ids[i], part_ids[j]));
                }
            }
        }
    }

    collisions
}

// ═══════════════════════════════════════════════════════════
// UNIT TESTS
// ═══════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use nova_core::Id;

    fn create_test_part(id: Id, name: &str) -> Part {
        let brep = BRep {
            vertices: vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(100.0, 0.0, 0.0),
                Vec3::new(100.0, 60.0, 0.0),
                Vec3::new(0.0, 60.0, 0.0),
                Vec3::new(0.0, 0.0, 20.0),
                Vec3::new(100.0, 0.0, 20.0),
                Vec3::new(100.0, 60.0, 20.0),
                Vec3::new(0.0, 60.0, 20.0),
            ],
            faces: vec![
                vec![0, 1, 2, 3],
                vec![4, 7, 6, 5],
                vec![0, 4, 5, 1],
                vec![1, 5, 6, 2],
                vec![2, 6, 7, 3],
                vec![3, 7, 4, 0],
            ],
        };

        Part::new(id, name.to_string(), brep)
    }

    #[test]
    fn test_part_creation() {
        let id = Id::new();
        let part = create_test_part(id, "TestPart");
        assert_eq!(part.name, "TestPart");
        assert_eq!(part.visible, true);
        assert!(part.transform.is_identity());
    }

    #[test]
    fn test_part_positioning() {
        let id = Id::new();
        let mut part = create_test_part(id, "TestPart");
        part.set_position(100.0, 200.0, 300.0);

        let vertices = part.get_world_vertices();
        assert_eq!(vertices[0].x, 100.0);
        assert_eq!(vertices[0].y, 200.0);
        assert_eq!(vertices[0].z, 300.0);
    }

    #[test]
    fn test_assembly_creation() {
        let assembly_id = Id::new();
        let mut assembly = Assembly::new(assembly_id, "TestAssembly".to_string());

        let part_id = Id::new();
        let part = create_test_part(part_id, "Part1");
        assembly.add_part(part);

        assert_eq!(assembly.part_count(), 1);
        assert!(assembly.get_part(part_id).is_some());
    }

    #[test]
    fn test_assembly_constraints() {
        let assembly_id = Id::new();
        let mut assembly = Assembly::new(assembly_id, "TestAssembly".to_string());

        let part1_id = Id::new();
        let part2_id = Id::new();

        assembly.add_part(create_test_part(part1_id, "Part1"));
        assembly.add_part(create_test_part(part2_id, "Part2"));

        let constraint = AssemblyConstraint::new(
            Id::new(),
            ConstraintType::Distance,
            part1_id,
            part2_id,
        )
        .with_value(50.0);

        assembly.add_constraint(constraint);

        assert_eq!(assembly.constraint_count(), 1);
    }

    #[test]
    fn test_bom_generation() {
        let assembly_id = Id::new();
        let mut assembly = Assembly::new(assembly_id, "TestAssembly".to_string());

        let part1_id = Id::new();
        let part2_id = Id::new();

        assembly.add_part(create_test_part(part1_id, "Part1"));
        assembly.add_part(create_test_part(part2_id, "Part2"));

        let bom = generate_bom(&assembly);
        assert_eq!(bom.items.len(), 2);
        assert!(bom.total_volume > 0.0);
    }

    #[test]
    fn test_collision_detection() {
        let assembly_id = Id::new();
        let mut assembly = Assembly::new(assembly_id, "TestAssembly".to_string());

        let part1_id = Id::new();
        let mut part1 = create_test_part(part1_id, "Part1");
        part1.set_position(0.0, 0.0, 0.0);

        let part2_id = Id::new();
        let mut part2 = create_test_part(part2_id, "Part2");
        part2.set_position(50.0, 0.0, 0.0);

        assembly.add_part(part1);
        assembly.add_part(part2);

        let collisions = find_collisions(&assembly);
        assert!(!collisions.is_empty()); // Parts overlap
    }
}

fn main() {
    println!("╔════════════════════════════════════════╗");
    println!("║   NOVARIS CAD — Phase 3: Assembly    ║");
    println!("║         Multi-body system              ║");
    println!("╚════════════════════════════════════════╝\n");

    let mut assembly = Assembly::new(Id::new(), "Sample Assembly".to_string());

    let part1 = Part::new(Id::new(), "Base".to_string(), BRep::default());
    let part2 = Part::new(Id::new(), "Cover".to_string(), BRep::default());

    assembly.add_part(part1);
    assembly.add_part(part2);

    println!("✓ Assembly: {}", assembly.name);
    println!("✓ Parts: {}", assembly.part_count());
    println!("✓ Total Volume: {:.2} mm³", assembly.get_total_volume());
}
