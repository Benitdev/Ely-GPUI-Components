use std::ops::Range;

use markdown::mdast::Node;

use super::{Inline, Node as Drawn, span};

/// A leaf's written span; code and math inside their marks.
pub(super) fn written(node: &Node, source: &str) -> Option<Range<usize>> {
    let whole = match node {
        Node::Text(_)
        | Node::Break(_)
        | Node::Html(_)
        | Node::FootnoteReference(_)
        | Node::Image(_)
        | Node::ImageReference(_)
        | Node::InlineCode(_)
        | Node::InlineMath(_) => span(node),
        _ => return None,
    };
    let marks = match node {
        Node::InlineCode(_) | Node::InlineMath(_) => {
            let text = &source[whole.clone()];
            text.len() - text.trim_start_matches(['`', '$']).len()
        }
        _ => 0,
    };
    Some(whole.start + marks..whole.end)
}

/// Where a fenced block's `text` starts, if written as drawn.
pub(super) fn code_from(source: &str, node: &Node, text: &str) -> Option<usize> {
    let whole = span(node);
    let written = &source[whole.clone()];
    let body = written.find('\n')? + 1;
    written[body..]
        .starts_with(text)
        .then_some(whole.start + body)
}

impl Drawn {
    /// Its source bytes counted from `start`, its block's.
    pub(crate) fn rebase(&mut self, start: usize) {
        match self {
            Drawn::Paragraph(inline) | Drawn::Heading(_, inline) => {
                for (_, span) in &mut inline.sources {
                    *span = span.start - start..span.end - start;
                }
            }
            Drawn::Quote(body) | Drawn::Alert { body, .. } | Drawn::Footnote { body, .. } => {
                body.iter_mut().for_each(|node| node.rebase(start))
            }
            Drawn::List { items, .. } => items
                .iter_mut()
                .flat_map(|(_, body)| body)
                .for_each(|node| node.rebase(start)),
            Drawn::Table { head, rows, .. } => head
                .iter_mut()
                .chain(rows.iter_mut().flatten())
                .flatten()
                .for_each(|node| node.rebase(start)),
            Drawn::Code { from, .. } => {
                if let Some(from) = from {
                    *from -= start;
                }
            }
            Drawn::Image { at, .. } => *at -= start,
            Drawn::Rule | Drawn::Math(_) | Drawn::Toc(_) | Drawn::FrontMatter(_) => {}
        }
    }
}

impl Inline {
    /// Source byte under drawn byte `at`, if a piece holds it.
    pub(crate) fn source_at(&self, source: &str, at: usize) -> Option<usize> {
        let piece = self.sources.partition_point(|(start, _)| *start <= at);
        let (start, span) = self.sources.get(piece.checked_sub(1)?)?;
        let end = self
            .sources
            .get(piece)
            .map_or(self.text.len(), |(next, _)| *next);
        let drawn = self.text.get(*start..end.max(*start))?;
        let written = source.get(span.clone())?;
        // Text that does not read back lands at its start.
        Some(span.start + align(drawn, written, at - start).unwrap_or(0))
    }
}

/// Where drawn byte `at` was written, past joins and escapes.
fn align(drawn: &str, written: &str, at: usize) -> Option<usize> {
    let (mut shown, mut source) = (drawn.char_indices().peekable(), written.char_indices());
    let mut next = source.next();
    loop {
        let here = next.map_or(written.len(), |(byte, _)| byte);
        let Some(&(byte, shown_char)) = shown.peek() else {
            return Some(here);
        };
        if byte >= at {
            return Some(here);
        }
        let (_, written_char) = next?;
        match (shown_char, written_char) {
            (a, b) if a == b => {
                shown.next();
            }
            (' ', '\r' | '\n') => {
                shown.next();
                next = source.next();
                while let Some((_, ' ' | '\t' | '\r' | '\n')) = next {
                    next = source.next();
                }
                continue;
            }
            (_, ' ' | '\t' | '\r' | '\n' | '\\') => {}
            _ => return None,
        }
        next = source.next();
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Reader, tree};

    /// The source byte under drawn text in the first paragraph.
    fn under(source: &str, drawn: &str) -> usize {
        let root = tree(source);
        let reader = Reader::new(source, &root);
        let paragraph = &root.children().expect("blocks")[0];
        let inline = reader.inline(paragraph.children().expect("inlines"));
        let at = inline.text.find(drawn).expect("drawn");
        inline.source_at(source, at).expect("a piece")
    }

    #[test]
    fn a_word_lands_where_it_was_written() {
        let source = "Say **bold** and `code` here\nnext line";
        assert_eq!(under(source, "bold"), source.find("bold").unwrap());
        assert_eq!(under(source, "code"), source.find("code").unwrap());
        assert_eq!(under(source, "here"), source.find("here").unwrap());
        assert_eq!(under(source, "next"), source.find("next").unwrap());
    }

    #[test]
    fn joins_escapes_and_indents_read_back() {
        let source = "a \\*star\\* x\n   y";
        assert_eq!(under(source, "x"), source.find('x').unwrap());
        assert_eq!(under(source, "y"), source.find('y').unwrap());
    }

    #[test]
    fn text_that_does_not_read_back_lands_at_its_start() {
        let source = "a &amp; b";
        assert_eq!(under(source, "b"), 0, "the reference ends the reading");
        let source = "x :smile: y";
        assert_eq!(under(source, "y"), 0);
    }
}
