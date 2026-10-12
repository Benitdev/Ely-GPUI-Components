//! Matches shown in any mode; replacements in one step.

use std::ops::Range;

use gpui::{Context, Window};

use super::MarkdownDocument;
use crate::documents::MarkdownMode;

impl MarkdownDocument {
    /// `range` in view: selected, or in its opened block.
    pub fn show_match(&mut self, range: Range<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let line = |document: &Self, cx: &Context<Self>| {
            document.editor.read(cx).text()[..range.start]
                .matches('\n')
                .count()
        };
        match self.mode {
            MarkdownMode::Source | MarkdownMode::Split => {
                self.editor
                    .update(cx, |editor, cx| editor.select(std::iter::once(range), cx));
            }
            MarkdownMode::Preview => self.reveal(line(self, cx), window, cx),
            MarkdownMode::Visual => {
                if self.commit(cx).is_some() || !self.read_now(cx) {
                    self.reparse(true, cx);
                }
                if !self.read_now(cx) {
                    // A long text: shown once read.
                    self.showing = Some(range);
                    return cx.notify();
                }
                let ix = self.block_at(range.start);
                let held = self.parsed.blocks.get(ix).map(|block| block.range.clone());
                let Some(block) =
                    held.filter(|block| block.start <= range.start && range.end <= block.end)
                else {
                    // Outside one block: its line.
                    return self.reveal(line(self, cx), window, cx);
                };
                let start = block.start;
                self.open(Some(ix), false, window, cx);
                self.list.scroll_to_reveal_item(ix);
                if let Some(open) = &self.open {
                    let input = open.input.clone();
                    let inside = range.start - start..range.end - start;
                    input.update(cx, |input, cx| input.select(inside, cx));
                }
            }
        }
    }

    /// A match asked before its text was read, now shown.
    pub(super) fn show_waiting(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.showing.is_none() || !self.read_now(cx) {
            return;
        }
        let range = self.showing.take().expect("checked above");
        let text = self.editor.read(cx).text();
        let fits = range.end <= text.len()
            && text.is_char_boundary(range.start)
            && text.is_char_boundary(range.end);
        if !fits {
            return log::warn!("markdown document: a waiting match outlived its text");
        }
        self.show_match(range, window, cx);
    }

    /// `edits` as one undo step; false in preview.
    pub fn replace(&mut self, edits: Vec<(Range<usize>, String)>, cx: &mut Context<Self>) -> bool {
        if self.mode == MarkdownMode::Preview {
            return false;
        }
        self.commit(cx);
        let text = self.editor.read(cx).text();
        if edits
            .iter()
            .all(|(range, with)| text[range.clone()] == *with)
        {
            return true;
        }
        log::info!("markdown document: {} replaced", edits.len());
        // Its own change: an open block outlives it.
        self.writes += 1;
        self.editor.update(cx, |editor, cx| editor.edit(edits, cx));
        true
    }
}
