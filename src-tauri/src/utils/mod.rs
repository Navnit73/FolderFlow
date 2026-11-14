//! Shared low-level helpers.

pub mod clock;
pub mod time;

/// RFC-4180 CSV field escaping with formula-injection protection (`=`, `+`, `-`, `@`, `\t`, `\r`).
pub fn csv_escape(s: &str) -> String {
    let guarded = if s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{s}")
    } else {
        s.to_owned()
    };
    if guarded.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", guarded.replace('"', "\"\""))
    } else {
        guarded
    }
}
