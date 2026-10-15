//! Sketch editing session: tools, snapping, relations and dimensions.
//!
//! Pure logic in sketch coordinates; the app turns mouse events into calls
//! here, so everything can be tested without a window.

use glam::DVec2;
use nova_core::Id;
use nova_features::FeatureId;
use nova_kernel::ArcGeom;
use nova_sketch::{Constraint, ConstraintType, Sketch, SketchElement, SolveReport};

/// Lines within this angle of an axis get an automatic H/V relation
const AUTO_HV_DEGREES: f64 = 2.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Line,
    Rectangle,
    Circle,
    /// Center point arc: center, start, end (counter-clockwise)
    Arc,
    /// Construction line, e.g. a revolve axis
    Centerline,
    /// Lone point, e.g. a hole position
    Point,
    SmartDimension,
}

impl Tool {
    pub fn label(&self) -> &'static str {
        match self {
            Tool::Select => "Sélection",
            Tool::Line => "Ligne",
            Tool::Rectangle => "Rectangle",
            Tool::Circle => "Cercle",
            Tool::Arc => "Arc centre",
            Tool::Centerline => "Axe",
            Tool::Point => "Point",
            Tool::SmartDimension => "Cotation intelligente",
        }
    }
}

/// Something that can be selected in a sketch
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SelItem {
    Point(Id),
    /// Line, circle or arc
    Entity(Id),
    /// Index into `sketch.constraints`
    Constraint(usize),
}

/// Relations offered for the current selection
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relation {
    Horizontal,
    Vertical,
    Coincident,
    Parallel,
    Perpendicular,
    Tangent,
    Equal,
    Midpoint,
    Fix,
}

impl Relation {
    pub fn label(&self) -> &'static str {
        match self {
            Relation::Horizontal => "Horizontale",
            Relation::Vertical => "Verticale",
            Relation::Coincident => "Coïncidente",
            Relation::Parallel => "Parallèle",
            Relation::Perpendicular => "Perpendiculaire",
            Relation::Tangent => "Tangente",
            Relation::Equal => "Égale",
            Relation::Midpoint => "Milieu",
            Relation::Fix => "Fixe",
        }
    }
}

/// Short text shown next to a relation in lists
pub fn constraint_label(c: &ConstraintType) -> String {
    match c {
        ConstraintType::Fixed { .. } => "Fixe".into(),
        ConstraintType::Horizontal { .. } | ConstraintType::EqualY { .. } => "Horizontale".into(),
        ConstraintType::Vertical { .. } | ConstraintType::EqualX { .. } => "Verticale".into(),
        ConstraintType::Coincident { .. } | ConstraintType::PointOnLine { .. } | ConstraintType::PointOnCircle { .. } => {
            "Coïncidente".into()
        }
        ConstraintType::Parallel { .. } => "Parallèle".into(),
        ConstraintType::Perpendicular { .. } => "Perpendiculaire".into(),
        ConstraintType::Tangent { .. } => "Tangente".into(),
        ConstraintType::Equal { .. } => "Égale".into(),
        ConstraintType::Midpoint { .. } => "Milieu".into(),
        ConstraintType::Distance { distance, .. } => format!("Distance {:.2}", distance),
        ConstraintType::Length { length, .. } => format!("Longueur {:.2}", length),
        ConstraintType::Radius { radius, .. } => format!("Rayon {:.2}", radius),
    }
}

/// Dimensions carry an editable value
pub fn dimension_value(c: &ConstraintType) -> Option<f64> {
    match *c {
        ConstraintType::Distance { distance, .. } => Some(distance),
        ConstraintType::Length { length, .. } => Some(length),
        ConstraintType::Radius { radius, .. } => Some(radius),
        _ => None,
    }
}

pub fn set_dimension_value(c: &mut ConstraintType, value: f64) {
    match c {
        ConstraintType::Distance { distance, .. } => *distance = value,
        ConstraintType::Length { length, .. } => *length = value,
        ConstraintType::Radius { radius, .. } => *radius = value,
        _ => {}
    }
}

/// Rubber-band preview of the tool in progress
#[derive(Clone, Debug, PartialEq)]
pub enum Preview {
    Line(DVec2, DVec2),
    Circle(DVec2, f64),
    Arc { center: DVec2, start: DVec2, end: DVec2 },
}

pub struct Sketcher {
    /// Sketch feature being edited (None = new sketch)
    pub feature: Option<FeatureId>,
    pub sketch: Sketch,
    pub tool: Tool,
    /// Points placed so far by the current tool operation
    pub stage: Vec<Id>,
    /// First point of the current line chain (clicking it closes the chain)
    chain_start: Option<Id>,
    pub selection: Vec<SelItem>,
    pub report: Option<SolveReport>,
    pub error: Option<String>,
}

impl Sketcher {
    pub fn new(feature: Option<FeatureId>, sketch: Sketch) -> Self {
        let mut s = Sketcher {
            feature,
            sketch,
            tool: Tool::Select,
            stage: Vec::new(),
            chain_start: None,
            selection: Vec::new(),
            report: None,
            error: None,
        };
        s.solve();
        s
    }

    pub fn solve(&mut self) {
        match self.sketch.solve() {
            Ok(r) => {
                self.report = Some(r);
                self.error = None;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    pub fn set_tool(&mut self, tool: Tool) {
        self.tool = tool;
        self.cancel();
    }

    /// Abort the operation in progress (Esc)
    pub fn cancel(&mut self) {
        self.stage.clear();
        self.chain_start = None;
    }

    pub fn point(&self, id: Id) -> Option<DVec2> {
        self.sketch.get_point(id).map(|p| DVec2::new(p.x, p.y))
    }

    /// Reuse a snapped point or create one
    fn point_at(&mut self, p: DVec2, snap: Option<Id>) -> Id {
        snap.filter(|id| self.sketch.get_point(*id).is_some())
            .unwrap_or_else(|| self.sketch.add_point(p.x, p.y))
    }

    /// Add a line, with SolidWorks-style automatic horizontal/vertical
    fn add_line(&mut self, a: Id, b: Id) {
        let (Some(pa), Some(pb)) = (self.point(a), self.point(b)) else { return };
        let added = if self.tool == Tool::Centerline { self.sketch.add_centerline(a, b) } else { self.sketch.add_line(a, b) };
        let Ok(line) = added else { return };
        let d = pb - pa;
        let tol = AUTO_HV_DEGREES.to_radians().tan();
        if d.y.abs() <= tol * d.x.abs() {
            self.sketch.add_constraint(Constraint::horizontal(line));
        } else if d.x.abs() <= tol * d.y.abs() {
            self.sketch.add_constraint(Constraint::vertical(line));
        }
    }

    /// A click in the sketch with the current tool.
    /// `snap`: existing point under the cursor; `hit`: entity under the cursor.
    /// Returns a constraint index when a new dimension should be edited.
    pub fn click(&mut self, p: DVec2, snap: Option<Id>, hit: Option<SelItem>, additive: bool) -> Option<usize> {
        let mut new_dimension = None;
        match self.tool {
            Tool::Select => {
                if !additive {
                    self.selection.clear();
                }
                if let Some(item) = hit {
                    if let Some(i) = self.selection.iter().position(|s| *s == item) {
                        self.selection.remove(i);
                    } else {
                        self.selection.push(item);
                    }
                }
                return None;
            }
            Tool::Point => {
                if snap.is_none() {
                    self.sketch.add_point(p.x, p.y);
                }
            }
            Tool::Centerline => {
                // A single segment per centerline
                let id = self.point_at(p, snap);
                match self.stage.first().copied() {
                    None => self.stage.push(id),
                    Some(first) if first != id => {
                        self.add_line(first, id);
                        self.cancel();
                    }
                    _ => {}
                }
            }
            Tool::Line => {
                let id = self.point_at(p, snap);
                match self.stage.last().copied() {
                    None => {
                        self.stage.push(id);
                        self.chain_start = Some(id);
                    }
                    Some(prev) if prev == id => {}
                    Some(prev) => {
                        self.add_line(prev, id);
                        if Some(id) == self.chain_start {
                            self.cancel(); // closed the contour
                        } else {
                            self.stage = vec![id];
                        }
                    }
                }
            }
            Tool::Rectangle => match self.stage.first().copied() {
                None => {
                    let id = self.point_at(p, snap);
                    self.stage.push(id);
                }
                Some(first) => {
                    let Some(a) = self.point(first) else {
                        self.cancel();
                        return None;
                    };
                    if (p - a).x.abs() < 1e-9 || (p - a).y.abs() < 1e-9 {
                        return None; // degenerate, wait for a proper corner
                    }
                    let c = self.point_at(p, snap);
                    let b = self.sketch.add_point(p.x, a.y);
                    let d = self.sketch.add_point(a.x, p.y);
                    for (u, v) in [(first, b), (b, c), (c, d), (d, first)] {
                        if let Ok(line) = self.sketch.add_line(u, v) {
                            let (pu, pv) = (self.point(u).unwrap(), self.point(v).unwrap());
                            self.sketch.add_constraint(if (pu.y - pv.y).abs() < 1e-12 {
                                Constraint::horizontal(line)
                            } else {
                                Constraint::vertical(line)
                            });
                        }
                    }
                    self.cancel();
                }
            },
            Tool::Circle => match self.stage.first().copied() {
                None => {
                    let id = self.point_at(p, snap);
                    self.stage.push(id);
                }
                Some(center) => {
                    let r = self.point(center).map(|c| (p - c).length()).unwrap_or(0.0);
                    if r > 1e-9 {
                        let _ = self.sketch.add_circle(center, r);
                    }
                    self.cancel();
                }
            },
            Tool::Arc => match self.stage.len() {
                0 | 1 => {
                    let id = self.point_at(p, snap);
                    if self.stage.last() != Some(&id) {
                        self.stage.push(id);
                    }
                }
                _ => {
                    let (center, start) = (self.stage[0], self.stage[1]);
                    let (c, s) = (self.point(center).unwrap(), self.point(start).unwrap());
                    // End lies on the arc's circle, in the clicked direction
                    let dir = (p - c).normalize_or_zero();
                    if dir != DVec2::ZERO {
                        let e = c + dir * (s - c).length();
                        let end = match snap {
                            Some(id) => id,
                            None => self.sketch.add_point(e.x, e.y),
                        };
                        if end != start {
                            let _ = self.sketch.add_arc(center, start, end);
                        }
                    }
                    self.cancel();
                }
            },
            Tool::SmartDimension => {
                match (hit, self.stage.first().copied()) {
                    (Some(SelItem::Entity(id)), None) => {
                        let c = match self.sketch.elements.get(&id) {
                            Some(SketchElement::Line(l)) => {
                                let len = (self.point(l.p2).unwrap() - self.point(l.p1).unwrap()).length();
                                Some(Constraint::length(id, round_dim(len)))
                            }
                            Some(SketchElement::Circle(c)) => Some(Constraint::radius(id, round_dim(c.radius))),
                            Some(SketchElement::Arc(a)) => {
                                let r = (self.point(a.start).unwrap() - self.point(a.center).unwrap()).length();
                                Some(Constraint::radius(id, round_dim(r)))
                            }
                            _ => None,
                        };
                        if let Some(c) = c {
                            self.sketch.add_constraint(c);
                            new_dimension = Some(self.sketch.constraints.len() - 1);
                        }
                    }
                    (Some(SelItem::Point(id)), None) => self.stage.push(id),
                    (Some(SelItem::Point(id)), Some(first)) if id != first => {
                        let d = (self.point(id).unwrap() - self.point(first).unwrap()).length();
                        self.sketch.add_constraint(Constraint::distance(first, id, round_dim(d)));
                        new_dimension = Some(self.sketch.constraints.len() - 1);
                        self.cancel();
                    }
                    _ => self.cancel(),
                }
            }
        }
        self.solve();
        new_dimension
    }

    /// Preview of the operation in progress with the cursor at `p`
    pub fn preview(&self, p: DVec2) -> Vec<Preview> {
        let pts: Vec<DVec2> = self.stage.iter().filter_map(|id| self.point(*id)).collect();
        match (self.tool, pts.as_slice()) {
            (Tool::Line | Tool::Centerline, [a]) => vec![Preview::Line(*a, p)],
            (Tool::Rectangle, [a]) => {
                let (b, d) = (DVec2::new(p.x, a.y), DVec2::new(a.x, p.y));
                vec![Preview::Line(*a, b), Preview::Line(b, p), Preview::Line(p, d), Preview::Line(d, *a)]
            }
            (Tool::Circle, [c]) => vec![Preview::Circle(*c, (p - *c).length())],
            (Tool::Arc, [c]) => vec![Preview::Line(*c, p)],
            (Tool::Arc, [c, s]) => {
                let dir = (p - *c).normalize_or_zero();
                vec![Preview::Arc { center: *c, start: *s, end: *c + dir * (*s - *c).length() }]
            }
            _ => Vec::new(),
        }
    }

    fn kind(&self, item: SelItem) -> Kind {
        match item {
            SelItem::Point(_) => Kind::Point,
            SelItem::Constraint(_) => Kind::Other,
            SelItem::Entity(id) => match self.sketch.elements.get(&id) {
                Some(SketchElement::Line(_)) => Kind::Line,
                Some(SketchElement::Circle(_)) | Some(SketchElement::Arc(_)) => Kind::Curve,
                _ => Kind::Other,
            },
        }
    }

    /// Relations that make sense for the current selection
    pub fn available_relations(&self) -> Vec<Relation> {
        use Kind::*;
        use Relation::*;
        let kinds: Vec<Kind> = self.selection.iter().map(|s| self.kind(*s)).collect();
        match kinds.as_slice() {
            [Line] => vec![Horizontal, Vertical],
            [Point] => vec![Fix],
            [Line, Line] => vec![Parallel, Perpendicular, Equal],
            [Line, Curve] | [Curve, Line] => vec![Tangent],
            [Curve, Curve] => vec![Equal],
            [Point, Point] => vec![Coincident, Horizontal, Vertical],
            [Point, Line] | [Line, Point] => vec![Coincident, Midpoint],
            [Point, Curve] | [Curve, Point] => vec![Coincident],
            _ => Vec::new(),
        }
    }

    pub fn apply_relation(&mut self, relation: Relation) {
        let ids: Vec<(Kind, Id)> = self
            .selection
            .iter()
            .filter_map(|s| match *s {
                SelItem::Point(id) | SelItem::Entity(id) => Some((self.kind(*s), id)),
                SelItem::Constraint(_) => None,
            })
            .collect();
        let find = |k: Kind| ids.iter().find(|(kk, _)| *kk == k).map(|(_, id)| *id);
        use ConstraintType as C;
        let c = match (relation, ids.as_slice()) {
            (Relation::Horizontal, [(Kind::Line, l)]) => Some(C::Horizontal { line_id: *l }),
            (Relation::Vertical, [(Kind::Line, l)]) => Some(C::Vertical { line_id: *l }),
            (Relation::Horizontal, [(_, a), (_, b)]) => Some(C::EqualY { p1: *a, p2: *b }),
            (Relation::Vertical, [(_, a), (_, b)]) => Some(C::EqualX { p1: *a, p2: *b }),
            (Relation::Fix, [(Kind::Point, p)]) => self.point(*p).map(|q| C::Fixed { point: *p, x: q.x, y: q.y }),
            (Relation::Parallel, [(_, a), (_, b)]) => Some(C::Parallel { l1: *a, l2: *b }),
            (Relation::Perpendicular, [(_, a), (_, b)]) => Some(C::Perpendicular { l1: *a, l2: *b }),
            (Relation::Equal, [(_, a), (_, b)]) => Some(C::Equal { e1: *a, e2: *b }),
            (Relation::Tangent, _) => find(Kind::Line).zip(find(Kind::Curve)).map(|(line, curve)| C::Tangent { line, curve }),
            (Relation::Midpoint, _) => find(Kind::Point).zip(find(Kind::Line)).map(|(point, line)| C::Midpoint { point, line }),
            (Relation::Coincident, [(Kind::Point, a), (Kind::Point, b)]) => Some(C::Coincident { p1: *a, p2: *b }),
            (Relation::Coincident, _) => match (find(Kind::Point), find(Kind::Line), find(Kind::Curve)) {
                (Some(point), Some(line), _) => Some(C::PointOnLine { point, line }),
                (Some(point), _, Some(curve)) => Some(C::PointOnCircle { point, curve }),
                _ => None,
            },
            _ => None,
        };
        if let Some(c) = c {
            self.sketch.add_constraint(Constraint::new(c));
            self.selection.clear();
            self.solve();
        }
    }

    /// Change a dimension's value and re-solve
    pub fn set_dimension(&mut self, index: usize, value: f64) {
        if value > 0.0 && value.is_finite() {
            if let Some(c) = self.sketch.constraints.get_mut(index) {
                set_dimension_value(&mut c.constraint_type, value);
                self.solve();
            }
        }
    }

    pub fn delete_selection(&mut self) {
        let mut constraints: Vec<usize> = self
            .selection
            .iter()
            .filter_map(|s| match s {
                SelItem::Constraint(i) => Some(*i),
                _ => None,
            })
            .collect();
        constraints.sort_unstable_by(|a, b| b.cmp(a));
        for i in constraints {
            if i < self.sketch.constraints.len() {
                self.sketch.constraints.remove(i);
            }
        }
        for s in std::mem::take(&mut self.selection) {
            if let SelItem::Point(id) | SelItem::Entity(id) = s {
                self.sketch.remove_element(id);
            }
        }
        self.cancel();
        self.solve();
    }

    /// Drag a point; the solver keeps the constraints satisfied
    pub fn drag_point(&mut self, id: Id, p: DVec2) {
        if let Some(SketchElement::Point(pt)) = self.sketch.elements.get_mut(&id) {
            pt.x = p.x;
            pt.y = p.y;
        }
        self.solve();
    }

    /// Geometry for hit testing and drawing an arc
    pub fn arc_geom(&self, center: Id, start: Id, end: Id) -> Option<ArcGeom> {
        let (c, s, e) = (self.point(center)?, self.point(start)?, self.point(end)?);
        let r = (s - c).length();
        let a0 = (s - c).y.atan2((s - c).x);
        let a1 = (e - c).y.atan2((e - c).x);
        Some(ArcGeom { center: c, radius: r, start_angle: a0, sweep: (a1 - a0).rem_euclid(std::f64::consts::TAU) })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Point,
    Line,
    Curve,
    Other,
}

/// Round a measured value for a new dimension, like SolidWorks does
fn round_dim(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_sketch::SolveStatus;

    fn session() -> Sketcher {
        Sketcher::new(None, Sketch::new("S"))
    }

    fn lines(s: &Sketcher) -> usize {
        s.sketch.elements.values().filter(|e| matches!(e, SketchElement::Line(_))).count()
    }

    #[test]
    fn line_chain_closes_on_first_point() {
        let mut s = session();
        s.set_tool(Tool::Line);
        s.click(DVec2::new(0.0, 0.0), None, None, false);
        s.click(DVec2::new(10.0, 0.0), None, None, false);
        s.click(DVec2::new(5.0, 8.0), None, None, false);
        let first = s.stage.is_empty();
        assert!(!first);
        let start = s.chain_start.unwrap();
        s.click(DVec2::new(0.0, 0.0), Some(start), None, false);
        assert!(s.stage.is_empty(), "chain ends after closing");
        assert_eq!(lines(&s), 3);
        assert_eq!(s.sketch.profiles().unwrap().len(), 1);
        // the first segment was almost horizontal
        assert!(s.sketch.constraints.iter().any(|c| matches!(c.constraint_type, ConstraintType::Horizontal { .. })));
    }

    #[test]
    fn rectangle_gets_hv_relations() {
        let mut s = session();
        s.set_tool(Tool::Rectangle);
        s.click(DVec2::new(0.0, 0.0), None, None, false);
        s.click(DVec2::new(100.0, 60.0), None, None, false);
        assert_eq!(lines(&s), 4);
        assert_eq!(s.sketch.constraints.len(), 4);
        // 8 coordinates - 4 relations
        assert_eq!(s.report.unwrap().dof, 4);
    }

    #[test]
    fn smart_dimension_fully_defines_rectangle() {
        let mut s = session();
        s.set_tool(Tool::Rectangle);
        s.click(DVec2::new(0.0, 0.0), None, None, false);
        s.click(DVec2::new(100.0, 60.0), None, None, false);
        let corner = s.sketch.elements.values().find_map(|e| match e {
            SketchElement::Point(p) if p.x == 0.0 && p.y == 0.0 => Some(p.id),
            _ => None,
        });
        s.set_tool(Tool::Select);
        s.click(DVec2::ZERO, None, Some(SelItem::Point(corner.unwrap())), false);
        assert_eq!(s.available_relations(), vec![Relation::Fix]);
        s.apply_relation(Relation::Fix);

        let (h, v) = {
            let mut h = None;
            let mut v = None;
            for e in s.sketch.elements.values() {
                if let SketchElement::Line(l) = e {
                    let (a, b) = (s.point(l.p1).unwrap(), s.point(l.p2).unwrap());
                    if (a.y - b.y).abs() < 1e-9 {
                        h = Some(l.id);
                    } else {
                        v = Some(l.id);
                    }
                }
            }
            (h.unwrap(), v.unwrap())
        };
        s.set_tool(Tool::SmartDimension);
        let i = s.click(DVec2::ZERO, None, Some(SelItem::Entity(h)), false).unwrap();
        s.set_dimension(i, 120.0);
        let j = s.click(DVec2::ZERO, None, Some(SelItem::Entity(v)), false).unwrap();
        s.set_dimension(j, 45.0);
        assert_eq!(s.report.unwrap().status, SolveStatus::FullyConstrained);
        assert!(s.sketch.elements.keys().all(|id| s.sketch.is_defined(*id)));
        let area = s.sketch.profiles().unwrap()[0].area();
        assert!((area - 120.0 * 45.0).abs() < 1e-6, "{}", area);
    }

    #[test]
    fn circle_and_arc_tools() {
        let mut s = session();
        s.set_tool(Tool::Circle);
        s.click(DVec2::new(5.0, 5.0), None, None, false);
        s.click(DVec2::new(8.0, 9.0), None, None, false);
        let r = s.sketch.elements.values().find_map(|e| match e {
            SketchElement::Circle(c) => Some(c.radius),
            _ => None,
        });
        assert!((r.unwrap() - 5.0).abs() < 1e-9);

        s.set_tool(Tool::Arc);
        s.click(DVec2::new(0.0, 0.0), None, None, false);
        s.click(DVec2::new(10.0, 0.0), None, None, false);
        assert_eq!(s.preview(DVec2::new(0.0, 3.0)).len(), 1);
        s.click(DVec2::new(0.0, 3.0), None, None, false);
        let arc = s.sketch.elements.values().find_map(|e| match e {
            SketchElement::Arc(a) => Some(*a),
            _ => None,
        });
        let arc = arc.unwrap();
        // end snapped onto the circle of radius 10
        assert!((s.point(arc.end).unwrap() - DVec2::new(0.0, 10.0)).length() < 1e-6);
    }

    #[test]
    fn relations_offered_by_selection() {
        let mut s = session();
        s.set_tool(Tool::Line);
        s.click(DVec2::new(0.0, 0.0), None, None, false);
        s.click(DVec2::new(10.0, 4.0), None, None, false);
        s.cancel();
        s.click(DVec2::new(0.0, 10.0), None, None, false);
        s.click(DVec2::new(4.0, 20.0), None, None, false);
        let ids: Vec<Id> = s.sketch.elements.values().filter_map(|e| match e {
            SketchElement::Line(l) => Some(l.id),
            _ => None,
        }).collect();
        s.set_tool(Tool::Select);
        s.click(DVec2::ZERO, None, Some(SelItem::Entity(ids[0])), false);
        s.click(DVec2::ZERO, None, Some(SelItem::Entity(ids[1])), true);
        assert_eq!(s.available_relations(), vec![Relation::Parallel, Relation::Perpendicular, Relation::Equal]);
        s.apply_relation(Relation::Perpendicular);
        assert!(s.selection.is_empty());
        assert!(s.sketch.constraints.iter().all(|c| c.is_satisfied));
    }

    #[test]
    fn delete_selection_removes_geometry_and_relations() {
        let mut s = session();
        s.set_tool(Tool::Rectangle);
        s.click(DVec2::new(0.0, 0.0), None, None, false);
        s.click(DVec2::new(10.0, 10.0), None, None, false);
        let line = s.sketch.elements.values().find_map(|e| match e {
            SketchElement::Line(l) => Some(l.id),
            _ => None,
        });
        s.set_tool(Tool::Select);
        s.click(DVec2::ZERO, None, Some(SelItem::Entity(line.unwrap())), false);
        s.delete_selection();
        assert_eq!(lines(&s), 3);
        assert_eq!(s.sketch.constraints.len(), 3);
    }

    #[test]
    fn centerline_and_point_tools() {
        let mut s = session();
        s.set_tool(Tool::Centerline);
        s.click(DVec2::new(0.0, -5.0), None, None, false);
        s.click(DVec2::new(0.0, 30.0), None, None, false);
        assert_eq!(s.sketch.centerlines().len(), 1);
        // vertical axis gets an automatic vertical relation
        assert!(s.sketch.constraints.iter().any(|c| matches!(c.constraint_type, ConstraintType::Vertical { .. })));
        s.set_tool(Tool::Point);
        s.click(DVec2::new(4.0, 4.0), None, None, false);
        assert_eq!(s.sketch.free_points().len(), 1);
        // axes are not profiles
        assert!(s.sketch.profiles().is_err());
    }

    #[test]
    fn dragging_respects_relations() {
        let mut s = session();
        s.set_tool(Tool::Line);
        let a = DVec2::new(0.0, 0.0);
        s.click(a, None, None, false);
        s.click(DVec2::new(10.0, 0.0), None, None, false);
        let end = s.stage[0];
        s.cancel();
        s.drag_point(end, DVec2::new(15.0, 3.0));
        // horizontal relation pulls it back to y = start.y
        let start_y = s.sketch.elements.values().find_map(|e| match e {
            SketchElement::Point(p) if p.id != end => Some(p.y),
            _ => None,
        }).unwrap();
        assert!((s.point(end).unwrap().y - start_y).abs() < 1e-6);
    }
}
