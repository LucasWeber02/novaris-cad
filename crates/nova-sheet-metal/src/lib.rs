// NOVARIS CAD — Phase 3: Sheet Metal System
// Bend simulation, unfold patterns, relief corners

use std::f64::consts::PI;
use nova_core::Id;
use nova_math::Vec3;

// ═══════════════════════════════════════════════════════════
// SHEET METAL PROPERTIES
// ═══════════════════════════════════════════════════════════

#[derive(Clone, Debug)]
pub struct SheetMetalProperties {
    pub id: Id,
    pub thickness: f64,     // Material thickness (mm)
    pub bend_radius: f64,   // Bend radius (mm)
    pub relief_distance: f64, // Relief corner distance
    pub material_grade: String,
}

impl SheetMetalProperties {
    pub fn new(id: Id, thickness: f64, bend_radius: f64) -> Self {
        SheetMetalProperties {
            id,
            thickness,
            bend_radius,
            relief_distance: thickness,
            material_grade: "Steel".to_string(),
        }
    }

    pub fn validate(&self) -> bool {
        self.thickness > 0.0
            && self.thickness < 100.0
            && self.bend_radius > 0.0
            && self.bend_radius >= self.thickness
    }

    pub fn get_k_factor(&self) -> f64 {
        // K-factor = bend neutral axis position / thickness
        // Typical range: 0.3-0.5 depending on material
        0.4
    }
}

// ═══════════════════════════════════════════════════════════
// BEND FEATURE
// ═══════════════════════════════════════════════════════════

#[derive(Clone, Debug, PartialEq)]
pub enum BendType {
    Positive, // Outward bend
    Negative, // Inward bend
}

#[derive(Clone, Debug)]
pub struct Bend {
    pub id: Id,
    pub name: String,
    pub angle: f64,           // Bend angle in degrees
    pub radius: f64,          // Bend radius in mm
    pub length: f64,          // Bend length in mm
    pub bend_type: BendType,
    pub relief_enabled: bool,
}

impl Bend {
    pub fn new(id: Id, angle: f64, radius: f64, length: f64) -> Self {
        Bend {
            id,
            name: format!("Bend{}", id),
            angle,
            radius,
            length,
            bend_type: BendType::Positive,
            relief_enabled: true,
        }
    }

    pub fn calculate_flat_length(&self, k_factor: f64) -> f64 {
        // Flat pattern length calculation
        let angle_rad = self.angle.to_radians();
        let bend_allowance = (self.radius + k_factor * 1.0) * angle_rad;
        self.length + bend_allowance
    }

    pub fn is_valid(&self) -> bool {
        self.angle > 0.0 && self.angle <= 180.0 && self.radius > 0.0 && self.length > 0.0
    }

    pub fn get_minimum_relief_distance(&self, thickness: f64) -> f64 {
        // Relief distance depends on bend radius and thickness
        (self.radius + thickness).sqrt() * thickness
    }
}

// ═══════════════════════════════════════════════════════════
// RELIEF CORNER
// ═══════════════════════════════════════════════════════════

#[derive(Clone, Debug)]
pub struct ReliefCorner {
    pub id: Id,
    pub position: Vec3,
    pub relief_type: ReliefType,
    pub size: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ReliefType {
    Rectangular,
    Circular,
    Trapezoidal,
}

impl ReliefCorner {
    pub fn new(id: Id, position: Vec3, relief_type: ReliefType) -> Self {
        ReliefCorner {
            id,
            position,
            relief_type,
            size: 5.0, // Default 5mm relief
        }
    }

    pub fn calculate_area(&self) -> f64 {
        match self.relief_type {
            ReliefType::Rectangular => self.size * self.size,
            ReliefType::Circular => PI * self.size * self.size / 4.0,
            ReliefType::Trapezoidal => self.size * self.size * 0.75,
        }
    }
}

// ═══════════════════════════════════════════════════════════
// FLAT PATTERN / UNFOLD
// ═══════════════════════════════════════════════════════════

#[derive(Clone, Debug)]
pub struct FlatPattern {
    pub id: Id,
    pub name: String,
    pub bends: Vec<Bend>,
    pub relief_corners: Vec<ReliefCorner>,
    pub total_flat_area: f64,
    pub bending_sequence: Vec<Id>, // Order to apply bends
}

impl FlatPattern {
    pub fn new(id: Id, name: String) -> Self {
        FlatPattern {
            id,
            name,
            bends: Vec::new(),
            relief_corners: Vec::new(),
            total_flat_area: 0.0,
            bending_sequence: Vec::new(),
        }
    }

    pub fn add_bend(&mut self, bend: Bend) {
        self.bending_sequence.push(bend.id);
        self.bends.push(bend);
    }

    pub fn add_relief(&mut self, relief: ReliefCorner) {
        self.relief_corners.push(relief);
    }

    pub fn calculate_material_usage(&self, thickness: f64, k_factor: f64) -> f64 {
        let mut total = 0.0;

        for bend in &self.bends {
            total += bend.calculate_flat_length(k_factor) * bend.length;
        }

        // Add relief areas
        for relief in &self.relief_corners {
            total -= relief.calculate_area();
        }

        total * thickness
    }

    pub fn get_bending_sequence(&self) -> Vec<String> {
        self.bending_sequence
            .iter()
            .map(|id| format!("Bend{}", id))
            .collect()
    }
}

// ═══════════════════════════════════════════════════════════
// PATTERN FEATURES (Linear/Circular/Mirror)
// ═══════════════════════════════════════════════════════════

#[derive(Clone, Debug, PartialEq)]
pub enum PatternType {
    Linear,   // Array in one direction
    Circular, // Radial array
    Mirror,   // Mirrored copy
}

#[derive(Clone, Debug)]
pub struct PatternFeature {
    pub id: Id,
    pub name: String,
    pub pattern_type: PatternType,
    pub count: usize,
    pub spacing: f64,    // Distance between copies
    pub radius: f64,     // For circular patterns
    pub angle: f64,      // For circular patterns
}

impl PatternFeature {
    pub fn new_linear(id: Id, count: usize, spacing: f64) -> Self {
        PatternFeature {
            id,
            name: format!("LinearPattern{}", id),
            pattern_type: PatternType::Linear,
            count,
            spacing,
            radius: 0.0,
            angle: 0.0,
        }
    }

    pub fn new_circular(id: Id, count: usize, radius: f64) -> Self {
        let angle = 360.0 / count as f64;
        PatternFeature {
            id,
            name: format!("CircularPattern{}", id),
            pattern_type: PatternType::Circular,
            count,
            spacing: 0.0,
            radius,
            angle,
        }
    }

    pub fn new_mirror(id: Id) -> Self {
        PatternFeature {
            id,
            name: format!("Mirror{}", id),
            pattern_type: PatternType::Mirror,
            count: 2,
            spacing: 0.0,
            radius: 0.0,
            angle: 0.0,
        }
    }

    pub fn calculate_total_instances(&self) -> usize {
        match self.pattern_type {
            PatternType::Mirror => 2,
            _ => self.count,
        }
    }

    pub fn is_valid(&self) -> bool {
        match self.pattern_type {
            PatternType::Linear => self.count > 0 && self.spacing > 0.0,
            PatternType::Circular => self.count > 0 && self.radius > 0.0,
            PatternType::Mirror => true,
        }
    }
}

// ═══════════════════════════════════════════════════════════
// UNIT TESTS
// ═══════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use nova_core::Id;

    #[test]
    fn test_sheet_metal_properties() {
        let props = SheetMetalProperties::new(Id::new(), 2.0, 5.0);
        assert_eq!(props.thickness, 2.0);
        assert_eq!(props.bend_radius, 5.0);
        assert!(props.validate());
    }

    #[test]
    fn test_bend_creation() {
        let bend = Bend::new(Id::new(), 90.0, 5.0, 100.0);
        assert_eq!(bend.angle, 90.0);
        assert!(bend.is_valid());
    }

    #[test]
    fn test_bend_flat_length() {
        let bend = Bend::new(Id::new(), 90.0, 5.0, 100.0);
        let flat_length = bend.calculate_flat_length(0.4);
        assert!(flat_length > 100.0); // Should be longer due to bend allowance
    }

    #[test]
    fn test_relief_corner() {
        let relief = ReliefCorner::new(
            Id::new(),
            Vec3::new(0.0, 0.0, 0.0),
            ReliefType::Rectangular,
        );
        let area = relief.calculate_area();
        assert!(area > 0.0);
    }

    #[test]
    fn test_flat_pattern() {
        let mut pattern = FlatPattern::new(Id::new(), "TestPattern".to_string());
        let bend = Bend::new(Id::new(), 90.0, 5.0, 100.0);
        pattern.add_bend(bend);
        assert_eq!(pattern.bends.len(), 1);
    }

    #[test]
    fn test_linear_pattern() {
        let pattern = PatternFeature::new_linear(Id::new(), 5, 10.0);
        assert_eq!(pattern.pattern_type, PatternType::Linear);
        assert_eq!(pattern.calculate_total_instances(), 5);
        assert!(pattern.is_valid());
    }

    #[test]
    fn test_circular_pattern() {
        let pattern = PatternFeature::new_circular(Id::new(), 6, 50.0);
        assert_eq!(pattern.pattern_type, PatternType::Circular);
        assert_eq!(pattern.calculate_total_instances(), 6);
        assert_eq!(pattern.angle, 60.0); // 360/6
        assert!(pattern.is_valid());
    }

    #[test]
    fn test_mirror_pattern() {
        let pattern = PatternFeature::new_mirror(Id::new());
        assert_eq!(pattern.pattern_type, PatternType::Mirror);
        assert_eq!(pattern.calculate_total_instances(), 2);
        assert!(pattern.is_valid());
    }
}
