//! A writing view: how text shows, not what it says.

use gpui::{Context, Rems, SharedString};

use super::MarkdownDocument;
use crate::documents::MarkdownMode;

/// How the document shows its text (MARK-09).
#[derive(Clone, Debug, PartialEq)]
pub struct DocumentLook {
    /// All but the current block or line fade.
    pub focus: bool,
    /// The current block or line stays mid-view.
    pub typewriter: bool,
    /// The widest the text column grows.
    pub measure: Option<Rems>,
    /// The prose font; none keeps the theme's.
    pub font: Option<SharedString>,
    /// Text size against the theme's.
    pub scale: f32,
}

impl Default for DocumentLook {
    fn default() -> Self {
        Self {
            focus: false,
            typewriter: false,
            measure: None,
            font: None,
            scale: 1.0,
        }
    }
}

impl MarkdownDocument {
    pub fn look(&self) -> &DocumentLook {
        &self.look
    }

    pub fn set_look(&mut self, look: DocumentLook, cx: &mut Context<Self>) {
        log::info!("markdown document: look {look:?}");
        self.editor.update(cx, |editor, cx| {
            editor.set_scale(look.scale, cx);
            editor.set_focus_line(look.focus, cx);
            editor.set_typewriter(look.typewriter, cx);
        });
        self.look = look;
        self.centered = None;
        cx.notify();
    }

    /// The reader's block: the open one, or the caret's.
    pub(super) fn current_block(&self, cx: &gpui::App) -> usize {
        match &self.open {
            Some(open) if open.append => self.parsed.blocks.len(),
            Some(open) => self.block_at(open.range.start),
            None => self.block_at(self.editor.read(cx).primary().head),
        }
    }

    /// The current block mid-view once it or its text moves.
    pub(super) fn center_current(&mut self, cx: &gpui::App) {
        if !self.look.typewriter || self.mode != MarkdownMode::Visual {
            return;
        }
        let ix = self.current_block(cx);
        let Some(block) = self.list.bounds_for_item(ix) else {
            return;
        };
        // A new layout moves it too: its height counts.
        let height = f32::from(block.size.height).round() as u64;
        let moved = (ix, self.edits, self.typed, height);
        if self.centered == Some(moved) {
            return;
        }
        let view = self.list.viewport_bounds();
        self.list.scroll_by(block.center().y - view.center().y);
        self.centered = Some(moved);
    }
}
