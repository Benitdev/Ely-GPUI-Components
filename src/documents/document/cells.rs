//! A table cell opens alone, as Typora edits tables.

use std::ops::Range;

use markdown::mdast::Node;

use crate::documents::{
    markdown::{file_tree, span, walk},
    render::CellAt,
};

/// A cell: which, its source, its caret.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Cell {
    pub at: CellAt,
    pub range: Range<usize>,
    pub caret: usize,
}

/// What names a cell in a block.
#[derive(Clone, Copy, Debug)]
pub(super) enum Aim {
    /// The cell holding this byte of the text.
    Byte(usize),
    /// This cell, its caret at the end.
    At(CellAt),
    First,
    Last,
}

/// A way out of a cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Move {
    Next,
    Previous,
    Up,
    Down,
}

/// The rows of each table in `root`, in drawing order.
fn tables(root: &Node) -> Vec<Vec<&Node>> {
    let mut nodes = Vec::new();
    walk(root, &mut nodes);
    nodes
        .into_iter()
        .filter(|node| matches!(node, Node::Table(_)))
        .map(|table| {
            table
                .children()
                .expect("a table holds rows")
                .iter()
                .collect()
        })
        .collect()
}

fn cells(row: &Node) -> &[Node] {
    row.children().expect("a row holds cells")
}

/// The cell `aim` names in `block` of `text`.
pub(super) fn find(text: &str, block: Range<usize>, aim: Aim) -> Option<Cell> {
    let root = file_tree(&text[block.clone()]);
    let tables = tables(&root);
    let at = match aim {
        Aim::At(at) => at,
        Aim::First if !tables.is_empty() => CellAt {
            table: 0,
            row: 0,
            column: 0,
        },
        Aim::Last => {
            let table = tables.len().checked_sub(1)?;
            let row = tables[table].len() - 1;
            let column = cells(tables[table][row]).len().checked_sub(1)?;
            CellAt { table, row, column }
        }
        Aim::First => return None,
        Aim::Byte(byte) => {
            let byte = byte.checked_sub(block.start)?;
            let table = (tables.iter()).position(|rows| {
                let (first, last) = (rows[0], rows[rows.len() - 1]);
                (span(first).start..=span(last).end).contains(&byte)
            })?;
            let rows = &tables[table];
            let row = rows.iter().rposition(|row| span(row).start <= byte)?;
            let column = (cells(rows[row]).iter()).rposition(|cell| span(cell).start <= byte)?;
            CellAt { table, row, column }
        }
    };
    let cell = cells(tables.get(at.table)?.get(at.row)?).get(at.column)?;
    let range = written(&text[block.clone()], cell);
    let caret = match aim {
        Aim::Byte(byte) => (byte - block.start).clamp(range.start, range.end) - range.start,
        _ => range.len(),
    };
    Some(Cell {
        at,
        range: block.start + range.start..block.start + range.end,
        caret,
    })
}

/// The cell `by` reaches from `at`; none past the edge.
pub(super) fn beside(text: &str, block: Range<usize>, at: CellAt, by: Move) -> Option<CellAt> {
    let root = file_tree(&text[block]);
    let tables = tables(&root);
    let rows = tables.get(at.table)?;
    let width = |row: usize| rows.get(row).map_or(0, |row| cells(row).len());
    let (row, column) = match by {
        Move::Next if at.column + 1 < width(at.row) => (at.row, at.column + 1),
        Move::Next => (at.row + 1, 0),
        Move::Previous if at.column > 0 => (at.row, at.column - 1),
        Move::Previous => {
            let row = at.row.checked_sub(1)?;
            (row, width(row).checked_sub(1)?)
        }
        Move::Up => (at.row.checked_sub(1)?, at.column),
        Move::Down => (at.row + 1, at.column),
    };
    let column = column.min(width(row).checked_sub(1)?);
    Some(CellAt {
        table: at.table,
        row,
        column,
    })
}

/// A cell's source; empty, just past its opening pipe.
fn written(text: &str, cell: &Node) -> Range<usize> {
    let inside = cell.children().expect("a cell holds inlines");
    if let (Some(first), Some(last)) = (inside.first(), inside.last()) {
        return span(first).start..span(last).end;
    }
    let span = span(cell);
    let mut at = span.start;
    if text[at..span.end].starts_with('|') {
        at += 1;
    }
    if text[at..span.end].starts_with([' ', '\t']) {
        at += 1;
    }
    at..at
}

/// `typed` with each bare `|` escaped.
fn guarded(typed: &str) -> String {
    let mut out = String::with_capacity(typed.len());
    let mut slashes = 0;
    for ch in typed.chars() {
        if ch == '|' && slashes % 2 == 0 {
            out.push('\\');
        }
        slashes = if ch == '\\' { slashes + 1 } else { 0 };
        out.push(ch);
    }
    out
}

/// Whether a pipe follows the cell source ending at `end`.
pub(super) fn closed(text: &str, end: usize) -> bool {
    text[end..].starts_with('|')
}

/// `typed` as a cell keeps it, a `closed` pipe intact.
pub(super) fn kept(typed: &str, closed: bool) -> String {
    let mut out = guarded(typed);
    let slashes = out.len() - out.trim_end_matches('\\').len();
    if closed && slashes % 2 == 1 {
        out.push('\\');
    }
    out
}

/// Byte `at` of `typed` within `kept(typed, closed)`.
pub(super) fn kept_at(typed: &str, at: usize, closed: bool) -> usize {
    match at == typed.len() {
        true => kept(typed, closed).len(),
        false => guarded(&typed[..at]).len(),
    }
}

#[cfg(test)]
mod tests {
    use super::{Aim, CellAt, Move, beside, closed, find, guarded, kept, kept_at};

    const TABLE: &str = "intro\n\n| a | **b** |\n| --- | --- |\n| c |  |\n| d | e |\n";

    fn block() -> std::ops::Range<usize> {
        7..TABLE.len() - 1
    }

    fn at(row: usize, column: usize) -> CellAt {
        CellAt {
            table: 0,
            row,
            column,
        }
    }

    #[test]
    fn a_byte_opens_its_cell_with_the_caret_there() {
        let byte = TABLE.find("**b**").expect("in the table") + 2;
        let cell = find(TABLE, block(), Aim::Byte(byte)).expect("a cell");
        assert_eq!(cell.at, at(0, 1));
        assert_eq!(&TABLE[cell.range.clone()], "**b**");
        assert_eq!(cell.caret, 2);
    }

    #[test]
    fn an_empty_cell_opens_between_its_pipes() {
        let cell = find(TABLE, block(), Aim::At(at(1, 1))).expect("a cell");
        assert!(cell.range.is_empty());
        let written = [&TABLE[..cell.range.start], "x", &TABLE[cell.range.end..]].concat();
        assert!(written.contains("| c | x |"), "{written:?}");
    }

    #[test]
    fn ends_open_the_first_and_last_cells() {
        let first = find(TABLE, block(), Aim::First).expect("a cell");
        assert_eq!((first.at, &TABLE[first.range]), (at(0, 0), "a"));
        let last = find(TABLE, block(), Aim::Last).expect("a cell");
        assert_eq!((last.at, &TABLE[last.range]), (at(2, 1), "e"));
        assert_eq!(find("plain\n", 0..5, Aim::First), None);
    }

    #[test]
    fn moves_wrap_rows_and_stop_at_edges() {
        let to = |from, by| beside(TABLE, block(), from, by);
        assert_eq!(to(at(0, 1), Move::Next), Some(at(1, 0)));
        assert_eq!(to(at(1, 0), Move::Previous), Some(at(0, 1)));
        assert_eq!(to(at(0, 0), Move::Previous), None);
        assert_eq!(to(at(2, 1), Move::Next), None);
        assert_eq!(to(at(1, 1), Move::Up), Some(at(0, 1)));
        assert_eq!(to(at(2, 0), Move::Down), None);
    }

    #[test]
    fn bare_pipes_escape_and_escaped_stay() {
        assert_eq!(guarded("a|b"), "a\\|b");
        assert_eq!(guarded("a\\|b"), "a\\|b");
        assert_eq!(guarded("a\\\\|b"), "a\\\\\\|b");
    }

    #[test]
    fn a_closing_backslash_keeps_the_next_pipe() {
        assert_eq!(kept("a\\", true), "a\\\\");
        assert_eq!(kept("a\\\\", true), "a\\\\");
        assert_eq!(kept_at("a\\", 2, true), 3);
        assert_eq!(kept_at("a\\b", 2, true), 2);
        let text = "|a|b|\n|---|---|\n|c|d|\n";
        let cell = find(text, 0..text.len() - 1, Aim::First).expect("a cell");
        let shut = closed(text, cell.range.end);
        let written = [
            &text[..cell.range.start],
            &kept("a\\", shut),
            &text[cell.range.end..],
        ]
        .concat();
        let b = find(&written, 0..written.len() - 1, Aim::At(at(0, 1))).expect("still a table");
        assert_eq!(&written[b.range], "b");
    }

    #[test]
    fn a_backslash_before_padding_stays_single() {
        let text = "| a\\ | b |\n| --- | --- |\n";
        let cell = find(text, 0..text.len() - 1, Aim::First).expect("a cell");
        assert_eq!(&text[cell.range.clone()], "a\\");
        assert!(!closed(text, cell.range.end));
        assert_eq!(kept("a\\", false), "a\\");
    }

    #[test]
    fn typed_pipes_shift_later_bytes() {
        assert_eq!(kept_at("||x", 2, false), 4);
    }
}
