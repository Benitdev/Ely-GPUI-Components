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
                if self.commit(cx).is_some() {
                    self.reparse(true, cx);
                }
                if !self.read_now(cx) {
                    return log::warn!("markdown document: a match shown while the text is read");
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

    /// `edits` to the text as one undo step.
    pub fn replace(&mut self, edits: Vec<(Range<usize>, String)>, cx: &mut Context<Self>) {
        self.commit(cx);
        log::info!("markdown document: {} replaced", edits.len());
        self.editor.update(cx, |editor, cx| editor.edit(edits, cx));
    }
}
