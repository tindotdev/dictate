//! Terminal UI helpers for dictate CLI
//!
//! Provides consistent styling for output and interactive prompts.

use owo_colors::OwoColorize;

/// Print an info message with blue info icon
pub fn info(text: &str) {
    println!("{} {}", "ℹ".blue(), text);
}

/// Format a device index badge (colored bracket)
pub fn device_index(index: usize) -> String {
    format!("[{index}]").cyan().to_string()
}

/// Format a default marker (green checkmark)
pub fn default_marker() -> String {
    " ✓".green().bold().to_string()
}

// Future: Interactive prompts
//
// When implementing setup wizard (Phase 2+), use dialoguer for device selection:
//
// use dialoguer::Select;
// let choice = Select::new()
//     .with_prompt("Choose microphone:")
//     .items(&device_names)
//     .interact()?;
