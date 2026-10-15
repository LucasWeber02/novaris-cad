// NOVARIS CAD — nova-ui crate
// Production UI + Viewport Management

use std::collections::HashMap;

pub use nova_core::Id;

// ═══════════════════════════════════════
// UI PANEL SYSTEM
// ═══════════════════════════════════════

#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub enum UiPanel {
    Properties,
    FeatureTree,
    Viewport,
    Console,
    Inspector,
}

#[derive(Clone, Debug)]
pub struct PanelState {
    pub visible: bool,
    pub width: u32,
    pub height: u32,
    pub x: u32,
    pub y: u32,
}

#[derive(Clone, Debug)]
pub struct UiLayout {
    pub panels: HashMap<UiPanel, PanelState>,
    pub window_width: u32,
    pub window_height: u32,
    pub zoom_level: f32,
}

impl UiLayout {
    pub fn new(width: u32, height: u32) -> Self {
        let mut panels = HashMap::new();
        let left_width = width / 4;
        let center_width = (width * 7) / 10;

        panels.insert(
            UiPanel::FeatureTree,
            PanelState {
                visible: true,
                width: left_width,
                height: height / 2,
                x: 0,
                y: 0,
            },
        );

        panels.insert(
            UiPanel::Properties,
            PanelState {
                visible: true,
                width: left_width,
                height: height / 2,
                x: 0,
                y: height / 2,
            },
        );

        panels.insert(
            UiPanel::Viewport,
            PanelState {
                visible: true,
                width: center_width,
                height,
                x: left_width,
                y: 0,
            },
        );

        panels.insert(
            UiPanel::Inspector,
            PanelState {
                visible: true,
                width: left_width,
                height,
                x: left_width + center_width,
                y: 0,
            },
        );

        UiLayout {
            panels,
            window_width: width,
            window_height: height,
            zoom_level: 1.0,
        }
    }

    pub fn toggle_panel(&mut self, panel: UiPanel) {
        if let Some(state) = self.panels.get_mut(&panel) {
            state.visible = !state.visible;
        }
    }
}

// ═══════════════════════════════════════
// VIEWPORT STATE
// ═══════════════════════════════════════

#[derive(Clone, Debug, PartialEq)]
pub enum RenderMode {
    Shaded,
    Wireframe,
    ShadedWireframe,
    Normals,
}

#[derive(Clone, Debug)]
pub struct ViewportState {
    pub fps: u32,
    pub triangle_count: u32,
    pub vertex_count: u32,
    pub draw_calls: u32,
    pub render_mode: RenderMode,
}

impl ViewportState {
    pub fn new() -> Self {
        ViewportState {
            fps: 60,
            triangle_count: 0,
            vertex_count: 0,
            draw_calls: 0,
            render_mode: RenderMode::Shaded,
        }
    }

    pub fn update_stats(&mut self, triangles: u32, vertices: u32, draws: u32) {
        self.triangle_count = triangles;
        self.vertex_count = vertices;
        self.draw_calls = draws;
    }

    pub fn cycle_render_mode(&mut self) {
        self.render_mode = match self.render_mode {
            RenderMode::Shaded => RenderMode::Wireframe,
            RenderMode::Wireframe => RenderMode::ShadedWireframe,
            RenderMode::ShadedWireframe => RenderMode::Normals,
            RenderMode::Normals => RenderMode::Shaded,
        };
    }
}

// ═══════════════════════════════════════
// FEATURE TREE PANEL
// ═══════════════════════════════════════

#[derive(Clone, Debug)]
pub struct FeatureTreeItem {
    pub id: Id,
    pub name: String,
    pub feature_type: String,
    pub selected: bool,
}

#[derive(Clone, Debug)]
pub struct FeatureTreePanel {
    pub items: HashMap<Id, FeatureTreeItem>,
    pub selected_id: Option<Id>,
}

impl FeatureTreePanel {
    pub fn new() -> Self {
        FeatureTreePanel {
            items: HashMap::new(),
            selected_id: None,
        }
    }

    pub fn add_item(&mut self, id: Id, name: String, feature_type: String) {
        self.items.insert(
            id,
            FeatureTreeItem {
                id,
                name,
                feature_type,
                selected: false,
            },
        );
    }

    pub fn select(&mut self, id: Id) {
        if let Some(old_selected) = self.selected_id {
            if let Some(item) = self.items.get_mut(&old_selected) {
                item.selected = false;
            }
        }

        if let Some(item) = self.items.get_mut(&id) {
            item.selected = true;
        }
        self.selected_id = Some(id);
    }

    pub fn item_count(&self) -> usize {
        self.items.len()
    }
}

// ═══════════════════════════════════════
// PROPERTIES PANEL
// ═══════════════════════════════════════

#[derive(Clone, Debug)]
pub enum PropertyValue {
    Float(f64),
    String(String),
}

#[derive(Clone, Debug)]
pub struct Property {
    pub name: String,
    pub value: PropertyValue,
    pub readonly: bool,
}

#[derive(Clone, Debug)]
pub struct PropertiesPanel {
    pub properties: Vec<Property>,
    pub selected_feature_id: Option<Id>,
}

impl PropertiesPanel {
    pub fn new() -> Self {
        PropertiesPanel {
            properties: Vec::new(),
            selected_feature_id: None,
        }
    }

    pub fn set_properties_for_feature(&mut self, feature_id: Id) {
        self.selected_feature_id = Some(feature_id);
        self.properties.clear();

        self.properties.push(Property {
            name: "Name".to_string(),
            value: PropertyValue::String(format!("Feature{:?}", feature_id)),
            readonly: false,
        });

        self.properties.push(Property {
            name: "Depth".to_string(),
            value: PropertyValue::Float(20.0),
            readonly: false,
        });

        self.properties.push(Property {
            name: "Volume".to_string(),
            value: PropertyValue::Float(120_000.0),
            readonly: true,
        });
    }

    pub fn update_property(&mut self, name: &str, value: PropertyValue) {
        for prop in &mut self.properties {
            if prop.name == name && !prop.readonly {
                prop.value = value;
                break;
            }
        }
    }

    pub fn get_property(&self, name: &str) -> Option<PropertyValue> {
        self.properties
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.value.clone())
    }
}

// ═══════════════════════════════════════
// APPLICATION
// ═══════════════════════════════════════

#[derive(Clone, Debug)]
pub struct Application {
    pub layout: UiLayout,
    pub viewport: ViewportState,
    pub feature_tree: FeatureTreePanel,
    pub properties: PropertiesPanel,
    pub title: String,
    pub running: bool,
}

impl Application {
    pub fn new() -> Self {
        Application {
            layout: UiLayout::new(1920, 1080),
            viewport: ViewportState::new(),
            feature_tree: FeatureTreePanel::new(),
            properties: PropertiesPanel::new(),
            title: "NOVARIS CAD — Phase 2 Production UI".to_string(),
            running: true,
        }
    }
}
