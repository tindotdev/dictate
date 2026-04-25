//! Cross-platform clipboard integration.
//!
//! Platform-specific clipboard behavior:
//! - **macOS**: uses `pbcopy` (always available — ships with every macOS since 10.0)
//! - **Wayland**: requires `wl-copy` (hard error if missing)
//! - **X11**: tries `xclip`, then `xsel` (hard error if both missing)
//! - **No display**: clipboard unavailable (error if requested)
//!
//! Design principles:
//! - No silent clipboard success (fail loudly if tools are missing)
//! - Never lose transcribed text (caller must handle failure by printing to stderr)
//! - External commands preferred over library dependencies for CLI tool

use std::io::Write;
use std::process::{Command, Stdio};
use thiserror::Error;

/// Errors that can occur during clipboard operations.
#[derive(Debug, Error)]
pub enum ClipboardError {
    /// Clipboard tools are not installed (Wayland: wl-copy, X11: xclip or xsel).
    #[error(
        "clipboard tool not found: {tool}\n\
         Install with: {install_hint}\n\
         Or use --no-clipboard to skip clipboard and print to stdout"
    )]
    ToolNotFound {
        /// Name of the missing tool (e.g., "wl-copy", "xclip/xsel").
        tool: String,
        /// Installation hint for the user's platform.
        install_hint: String,
    },

    /// No display environment detected (headless / SSH session).
    #[error(
        "clipboard unavailable: no display environment detected\n\
         Use --no-clipboard or --stdout to output text without clipboard"
    )]
    NoDisplay,

    /// Clipboard tool invocation failed (spawn, write, or non-zero exit).
    #[error("clipboard operation failed: {0}")]
    OperationFailed(String),

    /// I/O error when communicating with clipboard tool.
    #[error("clipboard I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Platform session type detected from environment variables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionType {
    Wayland,
    X11,
    MacOS,
    Unknown,
}

impl SessionType {
    /// Detect the current session type from the platform and environment.
    ///
    /// On macOS, returns `MacOS` unconditionally. On Linux, checks
    /// `XDG_SESSION_TYPE` first (primary), then `WAYLAND_DISPLAY` as fallback.
    fn detect() -> Self {
        if cfg!(target_os = "macos") {
            return Self::MacOS;
        }

        if let Ok(session_type) = std::env::var("XDG_SESSION_TYPE") {
            match session_type.to_lowercase().as_str() {
                "wayland" => return Self::Wayland,
                "x11" => return Self::X11,
                _ => {}
            }
        }

        // Fallback: check WAYLAND_DISPLAY for Wayland detection
        if std::env::var("WAYLAND_DISPLAY").is_ok() {
            return Self::Wayland;
        }

        // Check DISPLAY for X11 detection
        if std::env::var("DISPLAY").is_ok() {
            return Self::X11;
        }

        Self::Unknown
    }
}

/// Copy text to the system clipboard.
///
/// Platform-specific behavior:
/// - **macOS**: uses `pbcopy` (always available)
/// - **Wayland**: uses `wl-copy` (hard error if missing)
/// - **X11**: tries `xclip`, then `xsel` (hard error if both missing)
/// - **No display**: returns `NoDisplay` error
///
/// # Errors
///
/// Returns an error if:
/// - Clipboard tools are not installed ([`ClipboardError::ToolNotFound`])
/// - No display environment is detected ([`ClipboardError::NoDisplay`])
/// - The clipboard operation fails ([`ClipboardError::OperationFailed`])
///
/// # Example
///
/// ```no_run
/// use dictate_core::clipboard::copy_to_clipboard;
///
/// match copy_to_clipboard("Hello, world!") {
///     Ok(()) => println!("Copied to clipboard"),
///     Err(err) => eprintln!("Clipboard error: {err}"),
/// }
/// ```
pub fn copy_to_clipboard(text: &str) -> Result<(), ClipboardError> {
    match SessionType::detect() {
        SessionType::MacOS => copy_via_pbcopy(text),
        SessionType::Wayland => copy_via_wl_copy(text),
        SessionType::X11 => copy_via_x11(text),
        SessionType::Unknown => Err(ClipboardError::NoDisplay),
    }
}

/// Pre-flight check that clipboard support is available.
///
/// Validates that a display session is detected and that the required
/// clipboard tool is installed, without actually copying anything.
/// Call this before recording starts to fail fast on permanent errors
/// (missing tool, headless session) rather than losing transcribed text.
///
/// # Errors
///
/// Returns an error if:
/// - No display environment is detected ([`ClipboardError::NoDisplay`])
/// - Required clipboard tool is not installed ([`ClipboardError::ToolNotFound`])
pub fn check_clipboard_available() -> Result<(), ClipboardError> {
    match SessionType::detect() {
        SessionType::MacOS => Ok(()),
        SessionType::Wayland => {
            if !command_exists("wl-copy") {
                return Err(ClipboardError::ToolNotFound {
                    tool: "wl-copy".to_string(),
                    install_hint: "sudo apt install wl-clipboard  # or equivalent for your distro"
                        .to_string(),
                });
            }
            Ok(())
        }
        SessionType::X11 => {
            if !command_exists("xclip") && !command_exists("xsel") {
                return Err(ClipboardError::ToolNotFound {
                    tool: "xclip or xsel".to_string(),
                    install_hint: "sudo apt install xclip  # or xsel".to_string(),
                });
            }
            Ok(())
        }
        SessionType::Unknown => Err(ClipboardError::NoDisplay),
    }
}

/// Copy text to clipboard using `wl-copy` (Wayland).
fn copy_via_wl_copy(text: &str) -> Result<(), ClipboardError> {
    // Check if wl-copy is available
    if !command_exists("wl-copy") {
        return Err(ClipboardError::ToolNotFound {
            tool: "wl-copy".to_string(),
            install_hint: "sudo apt install wl-clipboard  # or equivalent for your distro"
                .to_string(),
        });
    }

    run_clipboard_command("wl-copy", &[], text)
}

/// Copy text to clipboard using `xclip` or `xsel` (X11).
///
/// Tries `xclip` first, then `xsel` as fallback. Returns error if both are missing.
fn copy_via_x11(text: &str) -> Result<(), ClipboardError> {
    // Try xclip first
    if command_exists("xclip") {
        return copy_via_xclip(text);
    }

    // Fallback to xsel
    if command_exists("xsel") {
        return copy_via_xsel(text);
    }

    // Both missing
    Err(ClipboardError::ToolNotFound {
        tool: "xclip or xsel".to_string(),
        install_hint: "sudo apt install xclip  # or xsel".to_string(),
    })
}

/// Copy text to clipboard using `xclip`.
fn copy_via_xclip(text: &str) -> Result<(), ClipboardError> {
    run_clipboard_command("xclip", &["-selection", "clipboard"], text)
}

/// Copy text to clipboard using `xsel`.
fn copy_via_xsel(text: &str) -> Result<(), ClipboardError> {
    run_clipboard_command("xsel", &["--clipboard", "--input"], text)
}

/// Copy text to clipboard using `pbcopy` (macOS).
///
/// `pbcopy` ships with every macOS since 10.0, so no `command_exists` guard
/// is needed — `spawn()` produces a clear error if somehow absent.
fn copy_via_pbcopy(text: &str) -> Result<(), ClipboardError> {
    run_clipboard_command("pbcopy", &[], text)
}

fn run_clipboard_command(program: &str, args: &[&str], text: &str) -> Result<(), ClipboardError> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| {
            ClipboardError::OperationFailed(format!("failed to spawn {program}: {err}"))
        })?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(text.as_bytes()).map_err(|err| {
            ClipboardError::OperationFailed(format!("failed to write to {program}: {err}"))
        })?;
    }

    let status = child.wait().map_err(|err| {
        ClipboardError::OperationFailed(format!("failed to wait for {program}: {err}"))
    })?;

    if !status.success() {
        return Err(ClipboardError::OperationFailed(format!(
            "{program} exited with non-zero status"
        )));
    }

    Ok(())
}

/// Check if a command exists in PATH.
fn command_exists(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Cross-platform tests ────────────────────────────────────────────────

    #[test]
    fn clipboard_error_tool_not_found_includes_hint() {
        let err = ClipboardError::ToolNotFound {
            tool: "wl-copy".to_string(),
            install_hint: "sudo apt install wl-clipboard".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("wl-copy"));
        assert!(msg.contains("sudo apt install wl-clipboard"));
        assert!(msg.contains("--no-clipboard"));
    }

    #[test]
    fn clipboard_error_no_display_suggests_flags() {
        let err = ClipboardError::NoDisplay;
        let msg = err.to_string();
        assert!(msg.contains("no display environment"));
        assert!(msg.contains("--no-clipboard"));
        assert!(msg.contains("--stdout"));
    }

    // ─── Linux-only tests (env-var manipulation) ─────────────────────────────

    #[cfg(target_os = "linux")]
    mod linux {
        use super::*;
        use std::sync::Mutex;

        // Serialize access to environment variables across parallel tests
        // to prevent race conditions when modifying and detecting session type.
        static ENV_LOCK: Mutex<()> = Mutex::new(());

        #[test]
        fn session_type_detect_wayland_from_xdg() {
            let _guard = ENV_LOCK.lock().unwrap();
            // SAFETY: We hold the lock, so no other test modifies env vars concurrently.
            unsafe {
                std::env::set_var("XDG_SESSION_TYPE", "wayland");
                std::env::remove_var("WAYLAND_DISPLAY");
            }
            assert_eq!(SessionType::detect(), SessionType::Wayland);
            // SAFETY: Cleanup of environment variable (still holding lock).
            unsafe {
                std::env::remove_var("XDG_SESSION_TYPE");
            }
        }

        #[test]
        fn session_type_detect_wayland_from_display() {
            let _guard = ENV_LOCK.lock().unwrap();
            // SAFETY: We hold the lock, so no other test modifies env vars concurrently.
            unsafe {
                std::env::remove_var("XDG_SESSION_TYPE");
                std::env::set_var("WAYLAND_DISPLAY", "wayland-0");
            }
            assert_eq!(SessionType::detect(), SessionType::Wayland);
            // SAFETY: Cleanup of environment variable (still holding lock).
            unsafe {
                std::env::remove_var("WAYLAND_DISPLAY");
            }
        }

        #[test]
        fn session_type_detect_x11_from_xdg() {
            let _guard = ENV_LOCK.lock().unwrap();
            // SAFETY: We hold the lock, so no other test modifies env vars concurrently.
            unsafe {
                std::env::set_var("XDG_SESSION_TYPE", "x11");
                std::env::remove_var("DISPLAY");
            }
            assert_eq!(SessionType::detect(), SessionType::X11);
            // SAFETY: Cleanup of environment variable (still holding lock).
            unsafe {
                std::env::remove_var("XDG_SESSION_TYPE");
            }
        }

        #[test]
        fn session_type_detect_x11_from_display() {
            let _guard = ENV_LOCK.lock().unwrap();
            // SAFETY: We hold the lock, so no other test modifies env vars concurrently.
            unsafe {
                std::env::remove_var("XDG_SESSION_TYPE");
                std::env::set_var("DISPLAY", ":0");
            }
            assert_eq!(SessionType::detect(), SessionType::X11);
            // SAFETY: Cleanup of environment variable (still holding lock).
            unsafe {
                std::env::remove_var("DISPLAY");
            }
        }

        #[test]
        fn session_type_detect_unknown_when_no_env() {
            let _guard = ENV_LOCK.lock().unwrap();
            // SAFETY: We hold the lock, so no other test modifies env vars concurrently.
            unsafe {
                std::env::remove_var("XDG_SESSION_TYPE");
                std::env::remove_var("WAYLAND_DISPLAY");
                std::env::remove_var("DISPLAY");
            }
            assert_eq!(SessionType::detect(), SessionType::Unknown);
        }

        #[test]
        fn check_clipboard_available_no_display_returns_error() {
            let _guard = ENV_LOCK.lock().unwrap();
            // SAFETY: We hold the lock, so no other test modifies env vars concurrently.
            unsafe {
                std::env::remove_var("XDG_SESSION_TYPE");
                std::env::remove_var("WAYLAND_DISPLAY");
                std::env::remove_var("DISPLAY");
            }
            let result = check_clipboard_available();
            assert!(result.is_err());
            assert!(
                matches!(result.unwrap_err(), ClipboardError::NoDisplay),
                "expected NoDisplay error when no display env is set"
            );
        }

        #[test]
        fn check_clipboard_available_wayland_session() {
            let _guard = ENV_LOCK.lock().unwrap();
            // SAFETY: We hold the lock, so no other test modifies env vars concurrently.
            unsafe {
                std::env::set_var("XDG_SESSION_TYPE", "wayland");
                std::env::remove_var("WAYLAND_DISPLAY");
                std::env::remove_var("DISPLAY");
            }
            let result = check_clipboard_available();
            // In CI, wl-copy may not be installed — accept Ok or ToolNotFound.
            match result {
                Ok(()) => {} // wl-copy found
                Err(ClipboardError::ToolNotFound { tool, .. }) => {
                    assert!(tool.contains("wl-copy"));
                }
                Err(other) => panic!("unexpected error: {other}"),
            }
            // SAFETY: Cleanup of environment variable (still holding lock).
            unsafe {
                std::env::remove_var("XDG_SESSION_TYPE");
            }
        }

        #[test]
        fn check_clipboard_available_x11_session() {
            let _guard = ENV_LOCK.lock().unwrap();
            // SAFETY: We hold the lock, so no other test modifies env vars concurrently.
            unsafe {
                std::env::set_var("XDG_SESSION_TYPE", "x11");
                std::env::remove_var("WAYLAND_DISPLAY");
                std::env::remove_var("DISPLAY");
            }
            let result = check_clipboard_available();
            // In CI, xclip/xsel may not be installed — accept Ok or ToolNotFound.
            match result {
                Ok(()) => {} // xclip or xsel found
                Err(ClipboardError::ToolNotFound { tool, .. }) => {
                    assert!(tool.contains("xclip") || tool.contains("xsel"));
                }
                Err(other) => panic!("unexpected error: {other}"),
            }
            // SAFETY: Cleanup of environment variable (still holding lock).
            unsafe {
                std::env::remove_var("XDG_SESSION_TYPE");
            }
        }
    }

    // ─── macOS-only tests ────────────────────────────────────────────────────

    #[cfg(target_os = "macos")]
    mod macos {
        use super::*;

        #[test]
        fn session_type_detect_macos() {
            assert_eq!(SessionType::detect(), SessionType::MacOS);
        }

        #[test]
        fn check_clipboard_available_macos_always_ok() {
            assert!(check_clipboard_available().is_ok());
        }

        #[test]
        fn copy_to_clipboard_macos_roundtrip() {
            use std::process::Command;

            let test_text = "dictate-clipboard-test-ΔΩ∑";
            copy_to_clipboard(test_text).expect("pbcopy should succeed");

            let output = Command::new("pbpaste")
                .output()
                .expect("pbpaste should succeed");

            let pasted = String::from_utf8_lossy(&output.stdout);
            assert_eq!(pasted, test_text);
        }
    }
}
