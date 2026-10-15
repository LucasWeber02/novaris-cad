// NOVARIS CAD — Phase 3 Week 4: Command & Control System
// Undo/Redo, keyboard shortcuts, performance monitoring

use std::collections::VecDeque;
use nova_core::Id;

// ═══════════════════════════════════════════════════════════
// COMMAND PATTERN — Undo/Redo
// ═══════════════════════════════════════════════════════════

pub trait Command: Send {
    fn execute(&mut self) -> Result<(), String>;
    fn undo(&mut self) -> Result<(), String>;
    fn get_name(&self) -> &str;
}

#[derive(Clone, Debug)]
pub struct CreateFeatureCommand {
    pub feature_id: Id,
    pub feature_name: String,
    pub executed: bool,
}

impl CreateFeatureCommand {
    pub fn new(feature_id: Id, feature_name: String) -> Self {
        CreateFeatureCommand {
            feature_id,
            feature_name,
            executed: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ModifyPropertyCommand {
    pub object_id: Id,
    pub property_name: String,
    pub old_value: String,
    pub new_value: String,
    pub executed: bool,
}

impl ModifyPropertyCommand {
    pub fn new(
        object_id: Id,
        property_name: String,
        old_value: String,
        new_value: String,
    ) -> Self {
        ModifyPropertyCommand {
            object_id,
            property_name,
            old_value,
            new_value,
            executed: false,
        }
    }
}

// ═══════════════════════════════════════════════════════════
// COMMAND HISTORY — Undo/Redo Stack
// ═══════════════════════════════════════════════════════════

#[derive(Clone, Debug)]
pub struct CommandHistory {
    pub undo_stack: VecDeque<String>, // Store command names
    pub redo_stack: VecDeque<String>,
    pub max_history: usize,
    pub current_position: usize,
}

impl CommandHistory {
    pub fn new(max_history: usize) -> Self {
        CommandHistory {
            undo_stack: VecDeque::new(),
            redo_stack: VecDeque::new(),
            max_history,
            current_position: 0,
        }
    }

    pub fn push_command(&mut self, command_name: String) {
        if self.undo_stack.len() >= self.max_history {
            self.undo_stack.pop_front();
        }

        self.undo_stack.push_back(command_name);
        self.redo_stack.clear(); // Clear redo when new command executed
        self.current_position += 1;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn undo(&mut self) -> Option<String> {
        if let Some(cmd) = self.undo_stack.pop_back() {
            self.redo_stack.push_back(cmd.clone());
            self.current_position = self.current_position.saturating_sub(1);
            Some(cmd)
        } else {
            None
        }
    }

    pub fn redo(&mut self) -> Option<String> {
        if let Some(cmd) = self.redo_stack.pop_back() {
            self.undo_stack.push_back(cmd.clone());
            self.current_position += 1;
            Some(cmd)
        } else {
            None
        }
    }

    pub fn get_history_count(&self) -> usize {
        self.undo_stack.len()
    }

    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.current_position = 0;
    }
}

// ═══════════════════════════════════════════════════════════
// KEYBOARD SHORTCUTS
// ═══════════════════════════════════════════════════════════

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeyCode {
    Z,
    Y,
    S,
    E,
    C,
    V,
    Delete,
    Escape,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Modifier {
    Ctrl,
    Shift,
    Alt,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KeyBinding {
    pub key: KeyCode,
    pub modifier: Modifier,
}

impl KeyBinding {
    pub fn new(key: KeyCode, modifier: Modifier) -> Self {
        KeyBinding { key, modifier }
    }

    pub fn ctrl_z() -> Self {
        KeyBinding {
            key: KeyCode::Z,
            modifier: Modifier::Ctrl,
        }
    }

    pub fn ctrl_y() -> Self {
        KeyBinding {
            key: KeyCode::Y,
            modifier: Modifier::Ctrl,
        }
    }

    pub fn ctrl_s() -> Self {
        KeyBinding {
            key: KeyCode::S,
            modifier: Modifier::Ctrl,
        }
    }

    pub fn to_string(&self) -> String {
        let modifier_str = match self.modifier {
            Modifier::Ctrl => "Ctrl+",
            Modifier::Shift => "Shift+",
            Modifier::Alt => "Alt+",
            Modifier::None => "",
        };

        let key_str = match self.key {
            KeyCode::Z => "Z",
            KeyCode::Y => "Y",
            KeyCode::S => "S",
            KeyCode::E => "E",
            KeyCode::C => "C",
            KeyCode::V => "V",
            KeyCode::Delete => "Delete",
            KeyCode::Escape => "Esc",
        };

        format!("{}{}", modifier_str, key_str)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Undo,
    Redo,
    Save,
    Copy,
    Paste,
    Delete,
    SelectAll,
    Deselect,
    CreateExtrusion,
    CreateBend,
}

pub struct KeyboardShortcuts {
    pub bindings: std::collections::HashMap<KeyBinding, Action>,
}

impl KeyboardShortcuts {
    pub fn new() -> Self {
        let mut bindings = std::collections::HashMap::new();

        // Standard CAD shortcuts
        bindings.insert(KeyBinding::ctrl_z(), Action::Undo);
        bindings.insert(KeyBinding::ctrl_y(), Action::Redo);
        bindings.insert(KeyBinding::ctrl_s(), Action::Save);
        bindings.insert(KeyBinding::new(KeyCode::C, Modifier::Ctrl), Action::Copy);
        bindings.insert(KeyBinding::new(KeyCode::V, Modifier::Ctrl), Action::Paste);
        bindings.insert(KeyBinding::new(KeyCode::Delete, Modifier::None), Action::Delete);

        KeyboardShortcuts { bindings }
    }

    pub fn get_action(&self, binding: &KeyBinding) -> Option<&Action> {
        self.bindings.get(binding)
    }

    pub fn rebind(&mut self, binding: KeyBinding, action: Action) {
        self.bindings.insert(binding, action);
    }

    pub fn get_binding(&self, action: &Action) -> Option<KeyBinding> {
        self.bindings
            .iter()
            .find(|(_, a)| std::mem::discriminant(*a) == std::mem::discriminant(action))
            .map(|(k, _)| *k)
    }
}

// ═══════════════════════════════════════════════════════════
// PERFORMANCE MONITORING
// ═══════════════════════════════════════════════════════════

#[derive(Clone, Debug)]
pub struct PerformanceMetrics {
    pub frame_times: Vec<f64>,
    pub operation_times: std::collections::HashMap<String, f64>,
    pub memory_usage: u64,
    pub triangle_count: u32,
}

impl PerformanceMetrics {
    pub fn new() -> Self {
        PerformanceMetrics {
            frame_times: Vec::with_capacity(60),
            operation_times: std::collections::HashMap::new(),
            memory_usage: 0,
            triangle_count: 0,
        }
    }

    pub fn record_frame_time(&mut self, time_ms: f64) {
        if self.frame_times.len() >= 60 {
            self.frame_times.remove(0);
        }
        self.frame_times.push(time_ms);
    }

    pub fn get_average_frame_time(&self) -> f64 {
        if self.frame_times.is_empty() {
            return 0.0;
        }
        self.frame_times.iter().sum::<f64>() / self.frame_times.len() as f64
    }

    pub fn get_fps(&self) -> f64 {
        let avg = self.get_average_frame_time();
        if avg > 0.0 {
            1000.0 / avg
        } else {
            0.0
        }
    }

    pub fn record_operation(&mut self, name: String, time_ms: f64) {
        self.operation_times.insert(name, time_ms);
    }

    pub fn get_slowest_operation(&self) -> Option<(String, f64)> {
        self.operation_times
            .iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(k, v)| (k.clone(), *v))
    }

    pub fn get_total_operation_time(&self) -> f64 {
        self.operation_times.values().sum()
    }
}

// ═══════════════════════════════════════════════════════════
// UNIT TESTS
// ═══════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_history() {
        let mut history = CommandHistory::new(10);
        history.push_command("CreateFeature".to_string());
        history.push_command("ModifyProperty".to_string());

        assert_eq!(history.get_history_count(), 2);
        assert!(history.can_undo());
    }

    #[test]
    fn test_undo_redo() {
        let mut history = CommandHistory::new(10);
        history.push_command("Cmd1".to_string());
        history.push_command("Cmd2".to_string());

        assert_eq!(history.undo(), Some("Cmd2".to_string()));
        assert!(history.can_redo());
        assert_eq!(history.redo(), Some("Cmd2".to_string()));
    }

    #[test]
    fn test_redo_clear_on_new_command() {
        let mut history = CommandHistory::new(10);
        history.push_command("Cmd1".to_string());
        history.undo();
        history.push_command("Cmd2".to_string());

        assert!(!history.can_redo());
    }

    #[test]
    fn test_key_binding() {
        let binding = KeyBinding::ctrl_z();
        assert_eq!(binding.key, KeyCode::Z);
        assert_eq!(binding.modifier, Modifier::Ctrl);
        assert_eq!(binding.to_string(), "Ctrl+Z");
    }

    #[test]
    fn test_keyboard_shortcuts() {
        let shortcuts = KeyboardShortcuts::new();
        let binding = KeyBinding::ctrl_z();
        assert_eq!(shortcuts.get_action(&binding), Some(&Action::Undo));
    }

    #[test]
    fn test_rebind_shortcut() {
        let mut shortcuts = KeyboardShortcuts::new();
        let new_binding = KeyBinding::new(KeyCode::Delete, Modifier::None);
        shortcuts.rebind(new_binding, Action::Undo);

        assert_eq!(shortcuts.get_action(&new_binding), Some(&Action::Undo));
    }

    #[test]
    fn test_performance_metrics() {
        let mut metrics = PerformanceMetrics::new();
        metrics.record_frame_time(16.67);
        metrics.record_frame_time(16.67);
        metrics.record_frame_time(16.67);

        assert!((metrics.get_average_frame_time() - 16.67).abs() < 0.01);
        assert!(metrics.get_fps() > 59.0 && metrics.get_fps() < 61.0);
    }

    #[test]
    fn test_slowest_operation() {
        let mut metrics = PerformanceMetrics::new();
        metrics.record_operation("Fast".to_string(), 0.5);
        metrics.record_operation("Slow".to_string(), 2.5);

        let (name, time) = metrics.get_slowest_operation().unwrap();
        assert_eq!(name, "Slow");
        assert_eq!(time, 2.5);
    }
}
