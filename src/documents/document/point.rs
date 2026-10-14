use std::rc::Rc;

use gpui::{Context, MouseButton, Window};
use markdown::mdast;

use super::{DocumentEvent, MarkdownDocument, edit::Landing, edit::moved};
use crate::documents::{
    render::{OnPoint, Place, Pointed},
    tree::{alert, is_toc},
};

/// A top-level block's kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetBlock {
    Paragraph,
    Heading(u8),
    Quote,
    Alert,
    List,
    Code,
    Math,
    Table,
    Rule,
    Html,
    Footnote,
    Definition,
    FrontMatter,
    Toc,
}

impl TargetBlock {
    /// `node`'s kind, read from `source`.
    pub(super) fn of(node: &mdast::Node, source: &str) -> Self {
        match node {
            mdast::Node::Paragraph(_) if is_toc(node, source) => Self::Toc,
            mdast::Node::Paragraph(_) => Self::Paragraph,
            mdast::Node::Heading(heading) => Self::Heading(heading.depth),
            mdast::Node::Blockquote(quote) if alert(quote).is_some() => Self::Alert,
            mdast::Node::Blockquote(_) => Self::Quote,
            mdast::Node::List(_) => Self::List,
            mdast::Node::Code(_) => Self::Code,
            mdast::Node::Math(_) => Self::Math,
            mdast::Node::Table(_) => Self::Table,
            mdast::Node::ThematicBreak(_) => Self::Rule,
            mdast::Node::Html(_) => Self::Html,
            mdast::Node::FootnoteDefinition(_) => Self::Footnote,
            mdast::Node::Definition(_) => Self::Definition,
            mdast::Node::Yaml(_) | mdast::Node::Toml(_) => Self::FrontMatter,
            other => unreachable!("{other:?} is no block outside MDX"),
        }
    }
}

/// What a secondary press landed on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentTarget {
    /// The source byte the caret went to.
    pub at: usize,
    /// None past the last block.
    pub block: Option<TargetBlock>,
    pub link: Option<String>,
    pub image: Option<String>,
    pub in_table: bool,
    pub in_code: bool,
}

impl MarkdownDocument {
    /// Presses on block `ix`'s drawing, read as it was drawn.
    pub(super) fn on_point(&self, ix: usize, cx: &mut Context<Self>) -> OnPoint {
        let (document, parsed) = (cx.entity().downgrade(), self.parsed.clone());
        let start = parsed.blocks[ix].range.start;
        Rc::new(move |pointed, button, window, cx| {
            // The drawing counts from its block's start.
            let at = match &pointed.place {
                Place::Text(inline, drawn) => inline.source_at(&parsed.text[start..], *drawn),
                Place::At(at) => Some(*at),
                Place::Block => None,
            }
            .map(|at| start + at);
            let ix = Some(ix);
            document
                .update(cx, |document, cx| match button {
                    MouseButton::Left => {
                        document.open(ix, at.map_or(Landing::End, Landing::At), window, cx)
                    }
                    MouseButton::Right => document.point(ix, at, pointed, window, cx),
                    _ => {}
                })
                .expect("a block's drawing lives in its document");
        })
    }

    /// Caret to `at`, else block `ix`; the owner hears.
    pub(super) fn point(
        &mut self,
        ix: Option<usize>,
        at: Option<usize>,
        pointed: Pointed,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let block = match ix.map(|ix| self.parsed.blocks.get(ix)) {
            Some(None) => return log::warn!("markdown document: a press on a block read away"),
            block => block.flatten(),
        };
        let kind = block.map(|block| block.kind);
        let at = at.or(block.map(|block| block.range.start));
        let written = self.commit(cx);
        if written.is_some() {
            self.reparse(true, cx);
        }
        if !self.read_now(cx) {
            return log::warn!("markdown document: pressed while the text is read again");
        }
        let at = match at {
            None => self.editor.read(cx).text().len(),
            Some(at) => match moved(at, &written) {
                Some(at) => at,
                None => return log::warn!("markdown document: the pressed text was written over"),
            },
        };
        self.editor
            .update(cx, |editor, cx| editor.select(std::iter::once(at..at), cx));
        window.focus(&self.focus, cx);
        log::info!("markdown document: a secondary press puts the caret at byte {at}");
        cx.emit(DocumentEvent::Pointed(DocumentTarget {
            at,
            block: kind,
            link: pointed.link,
            image: pointed.image,
            in_table: pointed.table || kind == Some(TargetBlock::Table),
            in_code: pointed.code || kind == Some(TargetBlock::Code),
        }));
        cx.notify();
    }
}
