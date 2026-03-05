//! Terminal UI helpers for dictate CLI
//!
//! Provides consistent styling for output and interactive prompts.

use owo_colors::OwoColorize;

/// Print an info message with blue info icon
pub fn info(text: &str) {
    println!("{} {}", "ℹ".blue(), text);
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
