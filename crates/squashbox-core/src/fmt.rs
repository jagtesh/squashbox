//! Terminal box-drawing table formatter.
//!
//! Provides a builder for rendering structured data in Unicode box-drawing
//! tables. Supports headers, sections, key-value rows, and free-form rows.
//!
//! # Example
//!
//! ```
//! use squashbox_core::fmt::Table;
//!
//! let output = Table::new(50)
//!     .header("My Title")
//!     .section("Stats")
//!     .kv("Count", "42")
//!     .kv("Size", "1.2 MB")
//!     .end_section()
//!     .build();
//! print!("{}", output);
//! ```
//!
//! Produces:
//! ```text
//! ╔══════════════════════════════════════════════════╗
//! ║                    My Title                      ║
//! ╚══════════════════════════════════════════════════╝
//!
//!   ┌─ Stats ────────────────────────────────────────┐
//!   │ Count:    42                                   │
//!   │ Size:     1.2 MB                               │
//!   └────────────────────────────────────────────────┘
//! ```

use std::fmt::Write;

/// Width of the box-drawing table (number of visible characters between
/// outer edges, inclusive of the border characters).
const DEFAULT_WIDTH: usize = 50;

/// A box-drawing table builder.
///
/// Accumulates rows and sections, then renders them to a `String` via
/// `build()`. All output uses Unicode box-drawing characters.
pub struct Table {
    width: usize,
    rows: Vec<Row>,
}

enum Row {
    /// ╔═══════════════════════════════╗
    /// ║           Title               ║
    /// ╚═══════════════════════════════╝
    Header(String),

    /// ┌─ Label ───────────────────────┐
    SectionStart(String),

    /// └───────────────────────────────┘
    SectionEnd,

    /// │ key:   value                  │
    KeyValue(String, String),

    /// │ icon text                     │
    FreeRow(String),

    /// Blank line (no borders)
    Blank,
}

impl Table {
    /// Create a new table with the given visible width.
    pub fn new(width: usize) -> Self {
        Self {
            width,
            rows: Vec::new(),
        }
    }

    /// Create a table with the default width (50 chars).
    pub fn default_width() -> Self {
        Self::new(DEFAULT_WIDTH)
    }

    /// Add a double-bordered header block.
    pub fn header(mut self, title: &str) -> Self {
        self.rows.push(Row::Header(title.to_string()));
        self
    }

    /// Start a named section with a single-line top border.
    pub fn section(mut self, label: &str) -> Self {
        self.rows.push(Row::SectionStart(label.to_string()));
        self
    }

    /// End the current section with a bottom border.
    pub fn end_section(mut self) -> Self {
        self.rows.push(Row::SectionEnd);
        self
    }

    /// Add a key-value row inside a section.
    pub fn kv(mut self, key: &str, value: &str) -> Self {
        self.rows
            .push(Row::KeyValue(key.to_string(), value.to_string()));
        self
    }

    /// Add a key-value row, formatting the value with `format!`.
    pub fn kvf(self, key: &str, value: impl std::fmt::Display) -> Self {
        self.kv(key, &format!("{}", value))
    }

    /// Add a free-form row inside a section.
    pub fn row(mut self, content: &str) -> Self {
        self.rows.push(Row::FreeRow(content.to_string()));
        self
    }

    /// Add a blank line (no borders).
    pub fn blank(mut self) -> Self {
        self.rows.push(Row::Blank);
        self
    }

    /// Render the table to a `String`.
    pub fn build(&self) -> String {
        let mut out = String::new();
        let w = self.width;

        for row in &self.rows {
            match row {
                Row::Header(title) => {
                    // ╔══════════╗
                    // ║  Title   ║
                    // ╚══════════╝
                    let inner = w - 2; // inside the ╔ and ╗
                    writeln!(&mut out, "╔{}╗", "═".repeat(inner)).unwrap();
                    let padded = center_pad(title, inner);
                    writeln!(&mut out, "║{}║", padded).unwrap();
                    writeln!(&mut out, "╚{}╝", "═".repeat(inner)).unwrap();
                }

                Row::SectionStart(label) => {
                    // ┌─ Label ──────────────────────────┐
                    let prefix = format!("─ {} ", label);
                    let remaining = w.saturating_sub(4 + prefix.len()); // 2 indent + ┌ + ┐
                    writeln!(&mut out, "  ┌{}{}┐", prefix, "─".repeat(remaining)).unwrap();
                }

                Row::SectionEnd => {
                    // └──────────────────────────────────┘
                    let inner = w - 4; // 2 indent + └ + ┘
                    writeln!(&mut out, "  └{}┘", "─".repeat(inner)).unwrap();
                }

                Row::KeyValue(key, value) => {
                    let content = format!(" {:<15} {}", format!("{}:", key), value);
                    let inner = w - 4; // 2 indent + │ + │
                    let padded = left_pad_to(&content, inner);
                    writeln!(&mut out, "  │{}│", padded).unwrap();
                }

                Row::FreeRow(content) => {
                    let prefixed = format!(" {}", content);
                    let inner = w - 4;
                    let padded = left_pad_to(&prefixed, inner);
                    writeln!(&mut out, "  │{}│", padded).unwrap();
                }

                Row::Blank => {
                    writeln!(&mut out).unwrap();
                }
            }
        }

        out
    }
}

/// Center a string within `width` characters, padding with spaces.
fn center_pad(s: &str, width: usize) -> String {
    let len = display_width(s);
    if len >= width {
        return s.to_string();
    }
    let left = (width - len) / 2;
    let right = width - len - left;
    format!("{}{}{}", " ".repeat(left), s, " ".repeat(right))
}

/// Left-pad a string to `width` characters with trailing spaces.
fn left_pad_to(s: &str, width: usize) -> String {
    let len = display_width(s);
    if len >= width {
        return s.to_string();
    }
    format!("{}{}", s, " ".repeat(width - len))
}

/// Approximate display width of a string.
///
/// Counts most characters as width 1, but emoji (common multi-byte sequences)
/// are counted as width 2 for terminal display. This is a simple heuristic —
/// a full implementation would use the `unicode-width` crate.
fn display_width(s: &str) -> usize {
    s.chars()
        .map(|c| if c.len_utf8() >= 3 { 2 } else { 1 })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_renders() {
        let out = Table::new(30).header("Test").build();
        assert!(out.contains("╔"));
        assert!(out.contains("Test"));
        assert!(out.contains("╝"));
    }

    #[test]
    fn section_renders() {
        let out = Table::new(40)
            .section("Stats")
            .kv("Count", "42")
            .end_section()
            .build();
        assert!(out.contains("┌─ Stats"));
        assert!(out.contains("Count:"));
        assert!(out.contains("42"));
        assert!(out.contains("┘"));
    }

    #[test]
    fn free_row_renders() {
        let out = Table::new(40)
            .section("Items")
            .row("📁 hello")
            .end_section()
            .build();
        assert!(out.contains("📁 hello"));
    }

    #[test]
    fn center_pad_works() {
        let padded = center_pad("hi", 10);
        assert_eq!(padded.len(), 10);
        assert!(padded.contains("hi"));
    }
}
