use gpui::{App, Context, Focusable as _, ListOffset, ScrollStrategy, Window, px};

use super::MarkdownDocument;
use crate::documents::{MarkdownMode, modes::joint};

/// Line and column, from zero, of byte `at` in `text`.
fn point(text: &str, at: usize) -> (usize, usize) {
    let before = &text[..at];
    let line_start = before.rfind('\n').map_or(0, |newline| newline + 1);
    (
        before.matches('\n').count(),
        before[line_start..].chars().count(),
    )
}

impl MarkdownDocument {
    /// The caret's line and column from zero, every mode.
    pub fn caret(&self, cx: &App) -> (usize, usize) {
        let editor = self.editor.read(cx);
        let Some(open) = &self.open else {
            return point(editor.text(), editor.primary().head);
        };
        let input = open.input.read(cx);
        let joined = match open.append && !input.text().is_empty() {
            true => joint(editor.text()).len(),
            false => 0,
        };
        point(&self.text(cx), open.range.start + joined + input.cursor())
    }

    /// The caret to `line` and `column`, clamped to text.
    pub fn set_caret(&mut self, line: usize, column: usize, cx: &mut Context<Self>) {
        self.commit(cx);
        let editor = self.editor.read(cx);
        let range = editor
            .buffer
            .line_range(line.min(editor.buffer.lines() - 1));
        let text = &editor.text()[range.clone()];
        let within = text
            .char_indices()
            .nth(column)
            .map_or(text.len(), |(at, _)| at);
        let at = range.start + within;
        self.editor
            .update(cx, |editor, cx| editor.select(std::iter::once(at..at), cx));
        cx.notify();
    }

    /// The first line in view.
    pub fn top_line(&self, cx: &App) -> usize {
        let editor = self.editor.read(cx);
        match self.mode {
            MarkdownMode::Source | MarkdownMode::Split => editor.top_line().unwrap_or(0),
            MarkdownMode::Visual | MarkdownMode::Preview => {
                let ix = self.list.logical_scroll_top().item_ix;
                let at = self
                    .parsed
                    .blocks
                    .get(ix)
                    .map_or(editor.text().len(), |block| block.range.start);
                editor.buffer.line_of(at.min(editor.text().len()))
            }
        }
    }

    /// Scrolls `line` to the top; the caret stays.
    pub fn set_top_line(&mut self, line: usize, cx: &mut Context<Self>) {
        let editor = self.editor.read(cx);
        let line = line.min(editor.buffer.lines() - 1);
        let (scroll, at) = (editor.scroll.clone(), editor.buffer.line_range(line).start);
        match self.mode {
            MarkdownMode::Source | MarkdownMode::Split => {
                scroll.scroll_to_item_strict(line, ScrollStrategy::Top)
            }
            MarkdownMode::Visual | MarkdownMode::Preview => {
                let item_ix = self.block_at(at);
                self.list.scroll_to(ListOffset {
                    item_ix,
                    offset_in_item: px(0.),
                });
            }
        }
        cx.notify();
    }

    /// The block holding byte `at`, or the one before it.
    pub(super) fn block_at(&self, at: usize) -> usize {
        self.parsed
            .blocks
            .partition_point(|block| block.range.start <= at)
            .saturating_sub(1)
    }

    /// Line `line` in view and focus, caret at start.
    pub fn reveal(&mut self, line: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.set_caret(line, 0, cx);
        let at = self.editor.read(cx).primary().head;
        match self.mode {
            MarkdownMode::Source | MarkdownMode::Split => {}
            MarkdownMode::Visual | MarkdownMode::Preview => {
                self.list.scroll_to_reveal_item(self.block_at(at))
            }
        }
        let focus = self.focus_handle(cx);
        window.focus(&focus, cx);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn points_count_lines_and_characters() {
        assert_eq!(super::point("ab\nçd\n", 6), (1, 2));
        assert_eq!(super::point("", 0), (0, 0));
    }
}
