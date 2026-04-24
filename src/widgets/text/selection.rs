//! Selection state machine + word-boundary + clipboard extraction.
use super::layout::TextCanvasData;

/// Row-level position used for selection anchors/heads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextPosition {
    pub row: usize,
    pub col: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionMode {
    Char,
    Word,
}

#[derive(Debug, Clone, Copy)]
pub struct TextSelection {
    pub anchor: TextPosition,
    pub head: TextPosition,
    pub dragging: bool,
    pub mode: SelectionMode,
    pub anchor_word: Option<(TextPosition, TextPosition)>,
}

impl TextSelection {
    pub fn ordered(&self) -> (TextPosition, TextPosition) {
        if (self.anchor.row, self.anchor.col) <= (self.head.row, self.head.col) {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }
}

impl TextCanvasData {
    /// Word-group range around `pos`. Whitespace, word ([A-Za-z0-9_]), and
    /// symbol runs each count as a word. Non-selectable or empty rows yield
    /// `(pos, pos)`.
    pub fn word_range_at(&self, pos: TextPosition) -> (TextPosition, TextPosition) {
        let row = match self.rows.get(pos.row) {
            Some(r) if r.selectable() => r,
            _ => return (pos, pos),
        };
        let raw = row.raw_text();
        let chars: Vec<char> = raw.chars().collect();
        if chars.is_empty() {
            return (pos, pos);
        }
        let idx = pos.col.min(chars.len().saturating_sub(1));
        let class = char_class(chars[idx]);
        let mut start = idx;
        while start > 0 && char_class(chars[start - 1]) == class {
            start -= 1;
        }
        let mut end = idx + 1;
        while end < chars.len() && char_class(chars[end]) == class {
            end += 1;
        }
        (
            TextPosition {
                row: pos.row,
                col: start,
            },
            TextPosition {
                row: pos.row,
                col: end,
            },
        )
    }
}

/// Build the selected text from rows + selection, filtering out
/// non-selectable rows so cross-hunk selections exclude separator lines.
pub fn selection_to_text(data: &TextCanvasData, selection: &TextSelection) -> String {
    if selection.is_empty() {
        return String::new();
    }
    let (start, end) = selection.ordered();
    let mut pieces: Vec<String> = Vec::new();
    for (idx, row) in data.rows.iter().enumerate() {
        if idx < start.row || idx > end.row {
            continue;
        }
        if !row.selectable() {
            continue;
        }
        let raw = row.raw_text();
        let chars: Vec<char> = raw.chars().collect();
        let from = if idx == start.row { start.col } else { 0 };
        let to = if idx == end.row { end.col } else { chars.len() };
        let from = from.min(chars.len());
        let to = to.min(chars.len()).max(from);
        pieces.push(chars[from..to].iter().collect());
    }
    pieces.join("\n")
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum CharClass {
    Word,
    Whitespace,
    Symbol,
}

fn char_class(c: char) -> CharClass {
    if c.is_ascii_alphanumeric() || c == '_' {
        CharClass::Word
    } else if c == ' ' || c == '\t' {
        CharClass::Whitespace
    } else {
        CharClass::Symbol
    }
}
