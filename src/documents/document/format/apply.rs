//! The document's format command.

use std::ops::Range;

use gpui::{AppContext as _, Context};

use super::{Change, FormatRefusal, InlineFormat, toggle};
use crate::documents::{
    MarkdownMode,
    document::{DocumentEvent, LARGEST_READ, MarkdownDocument},
    modes::joint,
};

/// The text formatted, the open block's offset, the selection.
type Snapshot = (String, usize, Range<usize>);

impl MarkdownDocument {
    /// `format` at the selection, read apart; refusals by event.
    pub fn format(
        &mut self,
        format: InlineFormat,
        cx: &mut Context<Self>,
    ) -> Result<(), FormatRefusal> {
        if self.mode == MarkdownMode::Preview {
            return Err(FormatRefusal::Preview);
        }
        let (full, offset, selection) = self.snapshot(cx);
        if full.len() > LARGEST_READ {
            log::info!(
                "markdown document: format {format:?} refused: {} bytes",
                full.len()
            );
            return Err(FormatRefusal::TooLong);
        }
        if self.formatting {
            log::info!("markdown document: format {format:?} refused: one runs");
            return Err(FormatRefusal::Busy);
        }
        log::info!(
            "markdown document: format {format:?} reads {} bytes apart",
            full.len()
        );
        let taken = (full.clone(), offset, selection.clone());
        self.formatting = true;
        cx.spawn(async move |document, cx| {
            let change = cx
                .background_spawn(async move { toggle(&full, selection, format) })
                .await;
            document
                .update(cx, |document, cx| {
                    document.formatting = false;
                    let done = if document.mode == MarkdownMode::Preview {
                        Err(FormatRefusal::Preview)
                    } else if document.snapshot(cx) != taken {
                        Err(FormatRefusal::Changed)
                    } else {
                        change.and_then(|change| {
                            document.apply(format, change, &taken.0, taken.1, cx)
                        })
                    };
                    if let Err(why) = done {
                        log::info!("markdown document: format {format:?} refused: {why:?}");
                        cx.emit(DocumentEvent::FormatRefused(why));
                    }
                })
                .ok();
        })
        .detach();
        Ok(())
    }

    /// The text a format reads: the open block's in place.
    fn snapshot(&self, cx: &Context<Self>) -> Snapshot {
        let text = self.editor.read(cx).text();
        let Some(open) = &self.open else {
            return (text.to_string(), 0, self.editor.read(cx).primary().range());
        };
        let input = open.input.read(cx);
        let (before, after) = match open.append {
            true => (format!("{text}{}", joint(text)), ""),
            false => (
                text[..open.range.start].to_string(),
                &text[open.range.end..],
            ),
        };
        let picked = input.selection();
        let offset = before.len();
        let full = format!("{before}{}{after}", input.text());
        (full, offset, offset + picked.start..offset + picked.end)
    }

    /// Writes `change`, into the open block if one is.
    fn apply(
        &mut self,
        format: InlineFormat,
        (edits, selected): Change,
        full: &str,
        offset: usize,
        cx: &mut Context<Self>,
    ) -> Result<(), FormatRefusal> {
        log::info!("markdown document: format {format:?}");
        let Some(open) = &self.open else {
            self.editor.update(cx, |editor, cx| {
                editor.edit(edits, cx);
                editor.select(std::iter::once(selected), cx);
            });
            return Ok(());
        };
        let end = offset + open.input.read(cx).text().len();
        if !edits
            .iter()
            .all(|(range, _)| offset <= range.start && range.end <= end)
        {
            log::warn!("markdown document: format {format:?} reaches past the open block");
            return Err(FormatRefusal::Across);
        }
        let (Some(from), Some(to)) = (
            edits.iter().map(|(range, _)| range.start).min(),
            edits.iter().map(|(range, _)| range.end).max(),
        ) else {
            return Ok(());
        };
        let mut written = String::new();
        let mut at = from;
        for (range, with) in &edits {
            written.push_str(&full[at..range.start]);
            written.push_str(with);
            at = range.end;
        }
        written.push_str(&full[at..to]);
        open.input.update(cx, |input, cx| {
            input.select(from - offset..to - offset, cx);
            input.insert(&written, cx);
            input.select(selected.start - offset..selected.end - offset, cx);
        });
        Ok(())
    }
}
