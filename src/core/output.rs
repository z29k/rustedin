//! The output contract every command honours.
//!
//! **stdout carries a single JSON document**; progress, warnings and request
//! traces go to stderr. That split is what makes the binary safe to drive from
//! a script or an agent: `rustedin ... | jq` never sees a log line.

use serde_json::Value;

/// Print the one JSON document a command produces.
pub fn emit(value: &Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).unwrap_or_default()
    );
}

/// Write a progress line to stderr, prefixed so it is obvious it is not output.
///
/// ```ignore
/// note!("Uploading {} ({})...", name, size);
/// note!("Warning: {} has no linked Instagram account.", page.name);
/// ```
#[macro_export]
macro_rules! note {
    ($($arg:tt)*) => {
        eprintln!("[rustedin] {}", format_args!($($arg)*))
    };
}
