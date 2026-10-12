use gpui::{App, Context, Focusable as _, ListOffset, ScrollStrategy, Window, px};

use super::{MarkdownDocument, Placement};
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

    /// The selected Markdown: the source's, or the open block's.
    pub fn selected(&self, cx: &App) -> String {
        let Some(open) = &self.open else {
            let editor = self.editor.read(cx);
            return editor.text()[editor.primary().range()].to_string();
        };
        let input = open.input.read(cx);
        input.text()[input.selection()].to_string()
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

    /// The first line in view; block lines share its height.
    pub fn top_line(&self, cx: &App) -> usize {
        let editor = self.editor.read(cx);
        if let Some(top) = self.top {
            return top.line.min(editor.buffer.lines() - 1);
        }
        match self.mode {
            MarkdownMode::Source | MarkdownMode::Split => editor.top_line().unwrap_or(0),
            MarkdownMode::Visual | MarkdownMode::Preview => {
                let ListOffset {
                    item_ix,
                    offset_in_item,
                } = self.list.logical_scroll_top();
                let Some(block) = self.parsed.blocks.get(item_ix) else {
                    return editor.buffer.lines() - 1;
                };
                let first = editor.buffer.line_of(block.range.start);
                let lines = editor.buffer.line_of(block.range.end) + 1 - first;
                let into = self.list.bounds_for_item(item_ix).map_or(0.0, |bounds| {
                    offset_in_item.as_f32() / bounds.size.height.as_f32().max(1.0)
                });
                first + ((into * lines as f32).round() as usize).min(lines - 1)
            }
        }
    }

    /// Scrolls `line` to the top; the caret stays.
    pub fn set_top_line(&mut self, line: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.asks += 1;
        self.top = Some(Placement {
            line,
            ask: self.asks,
            placing: false,
        });
        self.place_top(window, cx);
    }

    /// A placement in flight goes stale; it is placed again.
    pub(super) fn replace_top(&mut self) {
        if let Some(top) = &mut self.top {
            self.asks += 1;
            (top.ask, top.placing) = (self.asks, false);
        }
    }

    /// Moves the view to the asked top once read.
    pub(super) fn place_top(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Placement {
            line,
            ask,
            placing: false,
        }) = self.top
        else {
            return;
        };
        let editor = self.editor.read(cx);
        let line = line.min(editor.buffer.lines() - 1);
        if matches!(self.mode, MarkdownMode::Source | MarkdownMode::Split) {
            editor
                .scroll
                .scroll_to_item_strict(line + editor.pad, ScrollStrategy::Top);
            self.placing(ask);
            // The editor's offset holds it from the next frame.
            return cx.on_next_frame(window, move |document, _, cx| {
                document.placed(ask, None, cx)
            });
        }
        if !self.read_now(cx) {
            return;
        }
        let editor = self.editor.read(cx);
        let at = editor.buffer.line_range(line).start;
        let item_ix = self.block_at(at);
        let Some(block) = self.parsed.blocks.get(item_ix) else {
            self.top = None;
            return;
        };
        let first = editor.buffer.line_of(block.range.start);
        let lines = editor.buffer.line_of(block.range.end) + 1 - first;
        let into = line.saturating_sub(first) as f32 / lines as f32;
        self.list.scroll_to(ListOffset {
            item_ix,
            offset_in_item: px(0.),
        });
        self.placing(ask);
        cx.notify();
        // A block's height is known once drawn.
        cx.on_next_frame(window, move |document, _, cx| {
            document.placed(ask, Some((item_ix, into)), cx)
        });
    }

    fn placing(&mut self, ask: u64) {
        if let Some(top) = self.top.as_mut().filter(|top| top.ask == ask) {
            top.placing = true;
        }
    }

    /// A frame after placing ask `ask`; stale asks do nothing.
    fn placed(&mut self, ask: u64, within: Option<(usize, f32)>, cx: &mut Context<Self>) {
        if self.top.is_none_or(|top| top.ask != ask) {
            return;
        }
        if let Some((item_ix, into)) = within
            && let Some(bounds) = self.list.bounds_for_item(item_ix)
        {
            self.list.scroll_to(ListOffset {
                item_ix,
                offset_in_item: bounds.size.height * into,
            });
        }
        self.top = None;
        // The view moved: center again.
        self.centered = None;
        self.editor.update(cx, |editor, cx| {
            editor.centered = None;
            cx.notify();
        });
        cx.notify();
    }

    /// The block holding byte `at`, or the one before it.
    pub(super) fn block_at(&self, at: usize) -> usize {
        self.parsed
            .blocks
            .partition_point(|block| block.range.start <= at)
            .saturating_sub(1)
    }

    /// Line `line` at top and focused, caret at start.
    pub fn reveal(&mut self, line: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.set_caret(line, 0, cx);
        self.set_top_line(line, window, cx);
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
