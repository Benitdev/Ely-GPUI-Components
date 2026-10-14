use std::{iter::Peekable, ops::Range, str::CharIndices};

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

/// Where a block's text lines start: past a fence line.
pub(super) fn body(source: &str, node: &Node) -> usize {
    let whole = span(node);
    let written = &source[whole.clone()];
    let fenced = matches!(node, Node::Yaml(_) | Node::Toml(_))
        || ["```", "~~~"]
            .iter()
            .any(|fence| written.starts_with(fence));
    match fenced {
        true => written
            .find('\n')
            .map_or(whole.end, |end| whole.start + end + 1),
        false => whole.start,
    }
}

/// Source byte under drawn byte `at`; lines start at `from`.
pub(crate) fn lines_at(source: &str, from: usize, drawn: &str, at: usize) -> Option<usize> {
    let line = drawn[..at].matches('\n').count();
    let column = at - drawn[..at].rfind('\n').map_or(0, |end| end + 1);
    let shown = drawn.split('\n').nth(line)?;
    let mut start = from;
    for _ in 0..line {
        start += source.get(start..)?.find('\n')? + 1;
    }
    let written = source[start..].split('\n').next()?.trim_end_matches('\r');
    // Each drawn line ends its written one.
    written
        .ends_with(shown)
        .then(|| start + written.len() - shown.len() + column)
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
            Drawn::Code { from, .. } | Drawn::FrontMatter { from, .. } => *from -= start,
            Drawn::Image { at, .. } => *at -= start,
            Drawn::Rule | Drawn::Math(_) | Drawn::Toc(_) => {}
        }
    }
}

impl Inline {
    /// Source byte under drawn byte `at`, in some piece.
    pub(crate) fn source_at(&self, source: &str, at: usize) -> Option<usize> {
        let piece = self.sources.partition_point(|(start, _)| *start <= at);
        let (start, span) = self.sources.get(piece.checked_sub(1)?)?;
        let end = self
            .sources
            .get(piece)
            .map_or(self.text.len(), |(next, _)| *next);
        let drawn = self.text.get(*start..end.max(*start))?;
        let written = source.get(span.clone())?;
        // Unreadable text lands at its start.
        Some(span.start + align(drawn, written, at - start).unwrap_or(0))
    }
}

/// Where drawn byte `at` was written, past marks.
fn align(drawn: &str, written: &str, at: usize) -> Option<usize> {
    let mut reading = Reading::new(written, 0);
    for (byte, shown) in drawn.char_indices() {
        let found = reading.take(shown)?;
        if byte >= at {
            return Some(found);
        }
    }
    Some(reading.end)
}

/// Finds drawn chars, front to back, in written text.
struct Reading<'a> {
    written: Peekable<CharIndices<'a>>,
    base: usize,
    /// Past the last char found.
    end: usize,
}

impl<'a> Reading<'a> {
    fn new(written: &'a str, base: usize) -> Self {
        Self {
            written: written.char_indices().peekable(),
            base,
            end: base,
        }
    }

    /// Where `shown` was written; true if a char holds it.
    fn at(&mut self, shown: char) -> Option<(usize, bool)> {
        loop {
            let Some(&(byte, written)) = self.written.peek() else {
                return shown.is_whitespace().then_some((self.end, false));
            };
            let byte = self.base + byte;
            match (shown, written) {
                (a, b) if a == b => return Some((byte, true)),
                (a, b) if a.is_whitespace() => return Some((byte, b.is_whitespace())),
                // Joins, indents, escapes and quote marks.
                (_, ' ' | '\t' | '\r' | '\n' | '\\' | '>') => {
                    self.written.next();
                }
                _ => return None,
            }
        }
    }

    fn take(&mut self, shown: char) -> Option<usize> {
        let (byte, held) = self.at(shown)?;
        if held {
            let (_, written) = self.written.next().expect("a held char");
            self.end = byte + written.len_utf8();
        }
        Some(byte)
    }
}

/// A node's value bytes mapped to source, front to back.
pub(super) struct Follow<'a> {
    value: &'a str,
    done: usize,
    reading: Option<Reading<'a>>,
}

impl<'a> Follow<'a> {
    pub(super) fn new(value: &'a str, source: &'a str, span: Range<usize>) -> Self {
        let reading = Reading::new(&source[span.clone()], span.start);
        Self {
            value,
            done: 0,
            reading: Some(reading),
        }
    }

    /// Where value byte `at` was written.
    pub(super) fn to(&mut self, at: usize) -> Option<usize> {
        let reading = self.reading.as_mut()?;
        let lost = self.value[self.done..at]
            .chars()
            .any(|shown| reading.take(shown).is_none());
        self.done = at;
        if lost {
            log::warn!("markdown: a block's text lost its source");
            self.reading = None;
            return None;
        }
        let next = self.value[at..].chars().next()?;
        self.reading.as_mut()?.at(next).map(|(byte, _)| byte)
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
