use std::ops::Range;

use gpui::{AppContext as _, Context, Focusable as _, Window};

use super::{DocumentEvent, MarkdownDocument, Open};
use crate::{
    documents::{
        MarkdownMode,
        markdown::markdown_highlights,
        modes::{at_edge, joint},
    },
    forms::{InputEvent, TextInput},
};

/// Where byte `at` lies after `edit`, if it survives.
fn moved(at: usize, edit: &Option<(Range<usize>, usize)>) -> Option<usize> {
    match edit {
        None => Some(at),
        Some((replaced, _)) if at < replaced.start => Some(at),
        Some((replaced, written)) if at >= replaced.end => Some(at - replaced.len() + written),
        Some(_) => None,
    }
}

impl MarkdownDocument {
    /// An empty visual document takes typing at once.
    pub(super) fn focused(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let empty = self.editor.read(cx).text().trim().is_empty();
        if self.mode == MarkdownMode::Visual && self.open.is_none() && empty {
            self.open(None, false, window, cx);
        }
    }

    /// Opens block `ix` as shown, or one past the end.
    pub(super) fn open(
        &mut self,
        ix: Option<usize>,
        at_start: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pressed = match ix.map(|ix| self.parsed.blocks.get(ix)) {
            Some(None) => return log::warn!("markdown document: a press on a block read away"),
            pressed => pressed.flatten().map(|block| block.range.start),
        };
        let written = self.commit(cx);
        if written.is_some() {
            self.reparse(true, cx);
        }
        if !self.read_now(cx) {
            return log::warn!("markdown document: pressed while the text is read again");
        }
        let text = self.editor.read(cx).text();
        let (range, append) = match pressed {
            None => (text.len()..text.len(), true),
            Some(start) => {
                let found = moved(start, &written).and_then(|start| {
                    self.parsed
                        .blocks
                        .iter()
                        .find(|block| block.range.start == start)
                });
                let Some(block) = found else {
                    return log::warn!("markdown document: the pressed block changed as one wrote");
                };
                (block.range.clone(), false)
            }
        };
        let source = text[range.clone()].to_string();
        let input = cx.new(|cx| {
            TextInput::new(window, cx)
                .multi_line(1, usize::MAX)
                .highlighter(markdown_highlights)
        });
        input.update(cx, |input, cx| {
            input.set_text(source, cx);
            if at_start {
                input.select(0..0, cx);
            }
        });
        window.focus(&input.focus_handle(cx), cx);
        let subscriptions =
            vec![
                cx.subscribe(&input, |document, _, event: &InputEvent, cx| match event {
                    InputEvent::Blur => {
                        document.commit(cx);
                    }
                    InputEvent::Changed => cx.emit(DocumentEvent::Changed),
                    _ => {}
                }),
            ];
        log::info!("markdown document: block {range:?} opens");
        self.open = Some(Open {
            range,
            input,
            append,
            _subscriptions: subscriptions,
        });
        cx.notify();
    }

    /// Writes the open block as one step; the edit made.
    pub(super) fn commit(&mut self, cx: &mut Context<Self>) -> Option<(Range<usize>, usize)> {
        let open = self.open.take()?;
        cx.notify();
        let typed = open.input.read(cx).text().to_string();
        let caret = open.input.read(cx).cursor();
        let text = self.editor.read(cx).text();
        let (lead, written) = match open.append {
            true if typed.is_empty() => return None,
            true => (joint(text).len(), format!("{}{typed}", joint(text))),
            false => (0, typed),
        };
        let at = open.range.start + lead + caret;
        let edit = (text[open.range.clone()] != written).then(|| {
            log::info!("markdown document: block {:?} written back", open.range);
            (open.range.clone(), written.len())
        });
        if edit.is_some() {
            self.writes += 1;
            let start = open.range.start;
            let landed = self.editor.update(cx, |editor, cx| {
                editor.edit([(open.range, written.clone())], cx);
                editor.text().get(start..start + written.len()) == Some(written.as_str())
            });
            if !landed {
                self.writes -= 1;
                log::error!("markdown document: the editor refused a write-back");
                return None;
            }
        }
        // The one caret follows the block's.
        self.editor
            .update(cx, |editor, cx| editor.select(std::iter::once(at..at), cx));
        edit
    }

    /// Up past the open block's top, or down past it.
    pub(super) fn step(&mut self, down: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(open) = &self.open else {
            return false;
        };
        if open.append || !at_edge(open.input.read(cx), down) {
            return false;
        }
        let here = self.block_at(open.range.start);
        let next = match down {
            true => Some(here + 1).filter(|next| *next < self.parsed.blocks.len()),
            false => here.checked_sub(1),
        };
        let Some(next) = next else {
            return false;
        };
        self.open(Some(next), down, window, cx);
        true
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_place_moves_with_an_edit_before_it() {
        let edit = Some((2..6, 9));
        assert_eq!(super::moved(10, &edit), Some(15));
        assert_eq!(super::moved(1, &edit), Some(1), "before stays");
        assert_eq!(super::moved(3, &edit), None, "inside is gone");
        assert_eq!(super::moved(7, &None), Some(7));
    }
}
