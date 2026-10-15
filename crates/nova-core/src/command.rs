//! Command pattern for undo/redo and parametric updates

use crate::Result;

/// A command that can be executed, undone, and redone
pub trait Command {
    /// Execute the command
    fn execute(&mut self) -> Result<()>;

    /// Undo the command
    fn undo(&mut self) -> Result<()>;

    /// Redo the command
    fn redo(&mut self) -> Result<()>;

    /// Human-readable description
    fn description(&self) -> &str;
}

/// Executes commands with undo/redo support
pub struct CommandExecutor {
    undo_stack: Vec<Box<dyn Command>>,
    redo_stack: Vec<Box<dyn Command>>,
}

impl CommandExecutor {
    /// Create a new command executor
    pub fn new() -> Self {
        CommandExecutor {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    /// Execute a command and store it for undo
    pub fn execute(&mut self, mut cmd: Box<dyn Command>) -> Result<()> {
        cmd.execute()?;
        self.undo_stack.push(cmd);
        self.redo_stack.clear();
        Ok(())
    }

    /// Undo the last command
    pub fn undo(&mut self) -> Result<()> {
        if let Some(mut cmd) = self.undo_stack.pop() {
            cmd.undo()?;
            self.redo_stack.push(cmd);
            Ok(())
        } else {
            Err(crate::NovaError::OperationFailed("Nothing to undo".into()))
        }
    }

    /// Redo the last undone command
    pub fn redo(&mut self) -> Result<()> {
        if let Some(mut cmd) = self.redo_stack.pop() {
            cmd.redo()?;
            self.undo_stack.push(cmd);
            Ok(())
        } else {
            Err(crate::NovaError::OperationFailed("Nothing to redo".into()))
        }
    }

    /// Check if undo is possible
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Check if redo is possible
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }
}

impl Default for CommandExecutor {
    fn default() -> Self {
        Self::new()
    }
}
