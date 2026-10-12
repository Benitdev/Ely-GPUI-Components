//! Tables change by rows and columns (MARK-06).

use std::ops::Range;

use gpui::Context;
use markdown::mdast::{self, AlignKind};

use super::MarkdownDocument;
use crate::documents::{
    MarkdownMode,
    markdown::{span, tree, walk},
};

/// What a table edit does at the caret.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableEdit {
    RowAbove,
    RowBelow,
    ColumnLeft,
    ColumnRight,
    DeleteRow,
    DeleteColumn,
}

/// A new table: three columns, a head and two rows.
const NEW: &str = "|   |   |   |\n| --- | --- | --- |\n|   |   |   |\n|   |   |   |";

/// Why a table edit did not happen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableRefusal {
    /// No table at the caret.
    Outside,
    /// The head row and the last column stay.
    Kept,
    /// The caret is in a cell past the head's columns.
    Beyond,
    /// Its lines carry different quote or list marks.
    Mixed,
    /// Preview takes no edits.
    Preview,
}

/// The table around `at` after `edit`: its range, text, caret.
pub(super) fn edit_table(
    text: &str,
    at: usize,
    edit: TableEdit,
) -> Result<(Range<usize>, String, usize), TableRefusal> {
    let root = tree(text);
    let mut nodes = Vec::new();
    walk(&root, &mut nodes);
    let (range, table) = nodes
        .into_iter()
        .find_map(|node| match node {
            mdast::Node::Table(table) if (span(node).start..=span(node).end).contains(&at) => {
                Some((span(node), table))
            }
            _ => None,
        })
        .ok_or(TableRefusal::Outside)?;
    let prefix = &text[text[..range.start].rfind('\n').map_or(0, |found| found + 1)..range.start];
    if text[range.clone()]
        .split('\n')
        .skip(1)
        .any(|line| !line.starts_with(prefix))
    {
        return Err(TableRefusal::Mixed);
    }
    let rows: Vec<&mdast::Node> = table.children.iter().collect();
    let row = rows
        .iter()
        .rposition(|row| span(row).start <= at)
        .unwrap_or(0);
    let cells_of = |row: &mdast::Node| row.children().expect("a row holds cells").clone();
    let column = cells_of(rows[row])
        .iter()
        .rposition(|cell| span(cell).start <= at)
        .unwrap_or(0);
    let width = table.align.len();
    // Cells past the head stay as written.
    let mut grid: Vec<Vec<String>> = rows
        .iter()
        .map(|row| {
            let mut cells: Vec<String> = cells_of(row)
                .iter()
                .map(|cell| written(text, cell))
                .collect();
            if cells.len() < width {
                cells.resize(width, String::new());
            }
            cells
        })
        .collect();
    let mut aligns = table.align.clone();
    let column_edit = matches!(
        edit,
        TableEdit::ColumnLeft | TableEdit::ColumnRight | TableEdit::DeleteColumn
    );
    if column_edit && column >= width {
        return Err(TableRefusal::Beyond);
    }
    let (row, column) = match edit {
        TableEdit::RowAbove | TableEdit::RowBelow => {
            let below = edit == TableEdit::RowBelow || row == 0;
            let at = if below { row + 1 } else { row };
            grid.insert(at, vec![String::new(); width]);
            (at, column.min(width - 1))
        }
        TableEdit::DeleteRow if row > 0 => {
            grid.remove(row);
            (row.min(grid.len() - 1), column.min(width - 1))
        }
        TableEdit::ColumnLeft | TableEdit::ColumnRight => {
            let at = column + usize::from(edit == TableEdit::ColumnRight);
            grid.iter_mut()
                .for_each(|cells| cells.insert(at, String::new()));
            aligns.insert(at, AlignKind::None);
            (row, at)
        }
        TableEdit::DeleteColumn if width > 1 => {
            grid.iter_mut().for_each(|cells| {
                cells.remove(column);
            });
            aligns.remove(column);
            (row, column.min(width - 2))
        }
        _ => return Err(TableRefusal::Kept),
    };
    let (out, caret) = table_text(&grid, &aligns, (row, column), prefix);
    Ok((range, out, caret))
}

/// A cell's source between its pipes.
fn written(text: &str, cell: &mdast::Node) -> String {
    let inside = cell.children().expect("a cell holds inlines");
    match (inside.first(), inside.last()) {
        (Some(first), Some(last)) => text[span(first).start..span(last).end].to_string(),
        _ => String::new(),
    }
}

/// The table's text, each line after the first under `prefix`.
fn table_text(
    grid: &[Vec<String>],
    aligns: &[AlignKind],
    at: (usize, usize),
    prefix: &str,
) -> (String, usize) {
    let mut out = String::new();
    let mut caret = 0;
    for (row, cells) in grid.iter().enumerate() {
        for (column, cell) in cells.iter().enumerate() {
            out.push_str("| ");
            if (row, column) == at {
                caret = out.len();
            }
            out.push_str(cell);
            out.push(' ');
        }
        out.push('|');
        if row == 0 {
            out.push('\n');
            out.push_str(prefix);
            for align in aligns {
                out.push_str(match align {
                    AlignKind::Left => "| :-- ",
                    AlignKind::Right => "| --: ",
                    AlignKind::Center => "| :-: ",
                    AlignKind::None => "| --- ",
                });
            }
            out.push('|');
        }
        if row + 1 < grid.len() {
            out.push('\n');
            out.push_str(prefix);
        }
    }
    (out, caret)
}

impl MarkdownDocument {
    /// `edit` on the table at the caret, or why not.
    pub fn edit_table(
        &mut self,
        edit: TableEdit,
        cx: &mut Context<Self>,
    ) -> Result<(), TableRefusal> {
        if self.mode == MarkdownMode::Preview {
            return Err(TableRefusal::Preview);
        }
        self.commit(cx);
        let at = self.editor.read(cx).primary().range().start;
        let text = self.editor.read(cx).text().to_string();
        let (range, table, caret) = edit_table(&text, at, edit).inspect_err(|why| {
            log::info!("markdown document: table edit {edit:?} refused: {why:?}");
        })?;
        log::info!("markdown document: table edit {edit:?}");
        let caret = range.start + caret;
        self.editor.update(cx, |editor, cx| {
            editor.edit([(range, table)], cx);
            editor.select(std::iter::once(caret..caret), cx);
        });
        Ok(())
    }

    /// A new table after the caret's line, unless in preview.
    pub fn insert_table(&mut self, cx: &mut Context<Self>) -> Result<(), TableRefusal> {
        if self.mode == MarkdownMode::Preview {
            return Err(TableRefusal::Preview);
        }
        self.commit(cx);
        let text = self.editor.read(cx).text().to_string();
        let at = self.editor.read(cx).primary().range().start;
        let end = text[at..].find('\n').map_or(text.len(), |found| at + found);
        let start = text[..at].rfind('\n').map_or(0, |found| found + 1);
        let before = if text[start..end].trim().is_empty() {
            ""
        } else {
            "\n\n"
        };
        let after = if text[end..].starts_with("\n\n") || end == text.len() {
            ""
        } else {
            "\n"
        };
        let written = format!("{before}{NEW}{after}");
        let caret = end + before.len() + 2;
        log::info!("markdown document: a table goes in");
        self.editor.update(cx, |editor, cx| {
            editor.edit([(end..end, written)], cx);
            editor.select(std::iter::once(caret..caret), cx);
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{TableEdit, TableRefusal, edit_table};

    const TABLE: &str = "Intro\n\n| A | B |\n|:--|--:|\n| 1 | 2 |\n| 3 | 4 |\n\nAfter\n";

    fn edited_in(text: &str, at: &str, edit: TableEdit) -> Result<String, TableRefusal> {
        let at = text.find(at).expect("in the table");
        let (range, table, _) = edit_table(text, at, edit)?;
        Ok(format!(
            "{}{table}{}",
            &text[..range.start],
            &text[range.end..]
        ))
    }

    fn edited(at: &str, edit: TableEdit) -> Result<String, TableRefusal> {
        edited_in(TABLE, at, edit)
    }

    #[test]
    fn rows_and_columns_come_and_go() {
        let around = |table: &str| Ok(format!("Intro\n\n{table}\n\nAfter\n"));
        assert_eq!(
            edited("3", TableEdit::RowAbove),
            around("| A | B |\n| :-- | --: |\n| 1 | 2 |\n|  |  |\n| 3 | 4 |")
        );
        assert_eq!(
            edited("A", TableEdit::RowBelow),
            around("| A | B |\n| :-- | --: |\n|  |  |\n| 1 | 2 |\n| 3 | 4 |")
        );
        assert_eq!(
            edited("2", TableEdit::ColumnLeft),
            around("| A |  | B |\n| :-- | --- | --: |\n| 1 |  | 2 |\n| 3 |  | 4 |")
        );
        assert_eq!(
            edited("1", TableEdit::DeleteRow),
            around("| A | B |\n| :-- | --: |\n| 3 | 4 |")
        );
        assert_eq!(
            edited("B", TableEdit::DeleteColumn),
            around("| A |\n| :-- |\n| 1 |\n| 3 |")
        );
    }

    #[test]
    fn the_head_stays_and_outside_is_no_table() {
        assert_eq!(edited("A", TableEdit::DeleteRow), Err(TableRefusal::Kept));
        assert_eq!(
            edit_table(TABLE, 1, TableEdit::RowBelow).err(),
            Some(TableRefusal::Outside)
        );
        let single = "| A |\n| - |\n| 1 |\n";
        assert_eq!(
            edit_table(single, 2, TableEdit::DeleteColumn).err(),
            Some(TableRefusal::Kept)
        );
    }

    #[test]
    fn cells_past_the_head_stay_and_take_no_column_edit() {
        let text = "| A | B |\n| - | - |\n| 1 | 2 | 3 |\n";
        assert_eq!(
            edited_in(text, "3", TableEdit::DeleteColumn),
            Err(TableRefusal::Beyond)
        );
        assert_eq!(
            edited_in(text, "1", TableEdit::RowBelow),
            Ok("| A | B |\n| --- | --- |\n| 1 | 2 | 3 |\n|  |  |\n".into())
        );
    }

    #[test]
    fn a_quoted_table_keeps_its_marks() {
        let text = "> | A | B |\n> | - | - |\n> | 1 | 2 |\n";
        assert_eq!(
            edited_in(text, "1", TableEdit::RowBelow),
            Ok("> | A | B |\n> | --- | --- |\n> | 1 | 2 |\n> |  |  |\n".into())
        );
        let mixed = "- | A |\n  | - |\n  | 1 |\n";
        assert_eq!(
            edited_in(mixed, "1", TableEdit::RowBelow),
            Err(TableRefusal::Mixed)
        );
    }
}
