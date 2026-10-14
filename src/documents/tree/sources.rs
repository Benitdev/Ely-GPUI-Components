use std::ops::Range;

use markdown::{decode_named, mdast::Node};

use super::{Inline, Node as Drawn, html, span};

/// How a piece's source turns into its text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Escapes {
    Plain,
    /// Backslash escapes and references, as Markdown reads them.
    Markdown,
    /// References, as HTML reads them.
    Html,
}

/// A leaf's written span and how it reads.
pub(super) fn written(node: &Node, source: &str) -> Option<(Range<usize>, Escapes)> {
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
    Some(match node {
        Node::Text(_) => (whole, Escapes::Markdown),
        Node::InlineCode(_) => (inside(source, whole, '`'), Escapes::Plain),
        Node::InlineMath(_) => (inside(source, whole, '$'), Escapes::Plain),
        _ => (whole, Escapes::Plain),
    })
}

/// A code span's text, one end space cut each side.
fn inside(source: &str, whole: Range<usize>, mark: char) -> Range<usize> {
    let written = &source[whole.clone()];
    let open = written.len() - written.trim_start_matches(mark).len();
    let close = written.len() - written.trim_end_matches(mark).len();
    let text = &written[open..written.len().saturating_sub(close).max(open)];
    let blank = |ch: char| matches!(ch, ' ' | '\n' | '\r');
    let cut = text.starts_with(blank) && text.ends_with(blank) && !text.chars().all(blank);
    let width = |crlf: bool| if crlf { 2 } else { 1 };
    let (head, tail) = match cut {
        true => (
            width(text.starts_with("\r\n")),
            width(text.ends_with("\r\n")),
        ),
        false => (0, 0),
    };
    whole.start + open + head..whole.start + open + text.len() - tail
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
        true => lines(written)
            .nth(1)
            .map_or(whole.end, |line| whole.start + line.start),
        false => whole.start,
    }
}

/// `text`'s lines without their endings, split as CommonMark splits.
fn lines(text: &str) -> impl Iterator<Item = Range<usize>> + '_ {
    let mut next = Some(0);
    std::iter::from_fn(move || {
        let start = next?;
        let rest = &text[start..];
        let Some(end) = rest.find(['\n', '\r']) else {
            next = None;
            return Some(start..text.len());
        };
        let width = if rest[end..].starts_with("\r\n") {
            2
        } else {
            1
        };
        next = Some(start + end + width);
        Some(start..start + end)
    })
}

/// Source byte under drawn byte `at`; lines start at `from`.
pub(crate) fn lines_at(source: &str, from: usize, drawn: &str, at: usize) -> Option<usize> {
    LineMap::new(source, from, drawn)?.at(at)
}

/// Each drawn line set against the written line it ends.
pub(super) struct LineMap(Vec<Line>);

struct Line {
    drawn: Range<usize>,
    /// Where its written line starts, and its shared end.
    written: usize,
    common: usize,
    kept: usize,
    /// The char before the shared end, as a cut tab.
    before: usize,
}

impl LineMap {
    pub(super) fn new(source: &str, from: usize, drawn: &str) -> Option<Self> {
        let written = source.get(from..)?;
        let lines = lines(drawn).zip(lines(written)).map(|(line, place)| {
            let (shown, text) = (&drawn[line.clone()], &written[place.clone()]);
            let kept: usize = match text.ends_with(shown) {
                true => shown.len(),
                false => (shown.chars().rev())
                    .zip(text.chars().rev())
                    .take_while(|(a, b)| a == b)
                    .map(|(a, _)| a.len_utf8())
                    .sum(),
            };
            let common = text.len() - kept;
            let before =
                (text[..common].chars().next_back()).map_or(0, |ch| common - ch.len_utf8());
            Line {
                drawn: line,
                written: from + place.start,
                common,
                kept,
                before,
            }
        });
        Some(Self(lines.collect()))
    }

    /// Source byte under drawn byte `at`.
    pub(super) fn at(&self, at: usize) -> Option<usize> {
        let line = self
            .0
            .get(self.0.partition_point(|line| line.drawn.end < at))?;
        let column = at.saturating_sub(line.drawn.start);
        let shared = line.drawn.len() - line.kept;
        Some(
            line.written
                + match column.checked_sub(shared) {
                    Some(into) => line.common + into,
                    None => line.before,
                },
        )
    }
}

impl Drawn {
    /// Its source bytes counted from `start`, its block's.
    pub(crate) fn rebase(&mut self, start: usize) {
        match self {
            Drawn::Paragraph(inline) | Drawn::Heading(_, inline) => {
                for (_, span, _) in &mut inline.sources {
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
        let piece = self.sources.partition_point(|(start, ..)| *start <= at);
        let (start, span, escapes) = self.sources.get(piece.checked_sub(1)?)?;
        let end = self
            .sources
            .get(piece)
            .map_or(self.text.len(), |(next, ..)| *next);
        let drawn = self.text.get(*start..end.max(*start))?;
        let written = source.get(span.clone())?;
        // Unreadable text lands at its start.
        Some(span.start + align(drawn, written, *escapes, at - start).unwrap_or(0))
    }
}

/// A source char as read, and where it was written.
struct Read {
    ch: char,
    at: usize,
    /// Written as itself, not escaped or referenced.
    raw: bool,
    /// A container mark or indent at a line's start.
    lead: bool,
}

impl Read {
    fn ends_line(&self) -> bool {
        self.raw && matches!(self.ch, '\n' | '\r')
    }
}

/// `written`'s chars, escapes and references read.
fn read(written: &str, escapes: Escapes) -> Vec<Read> {
    let mut out = Vec::with_capacity(written.len());
    let (mut at, mut line_start) = (0, false);
    while let Some(ch) = written[at..].chars().next() {
        let rest = &written[at..];
        let decoded = match (escapes, ch) {
            (Escapes::Markdown, '\\') => (rest[1..].chars().next())
                .filter(char::is_ascii_punctuation)
                .map(|punct| (punct.to_string(), 2, at + 1)),
            (Escapes::Markdown, '&') => reference(rest).map(|(value, used)| (value, used, at)),
            (Escapes::Html, '&') => {
                html::reference(rest, false).map(|(value, used)| (value, used, at))
            }
            _ => None,
        };
        if let Some((value, used, place)) = decoded {
            out.extend(value.chars().map(|ch| Read {
                ch,
                at: place,
                raw: false,
                lead: false,
            }));
            (at, line_start) = (at + used, false);
            continue;
        }
        let ends = matches!(ch, '\n' | '\r');
        let lead = line_start && !ends && matches!(ch, ' ' | '\t' | '>');
        out.push(Read {
            ch,
            at,
            raw: true,
            lead,
        });
        (at, line_start) = (at + ch.len_utf8(), ends || lead);
    }
    out
}

/// A reference at `text`'s start, as Markdown reads it.
fn reference(text: &str) -> Option<(String, usize)> {
    let body = text.strip_prefix('&')?;
    let (end, _) = body.char_indices().take(34).find(|(_, ch)| *ch == ';')?;
    let name = &body[..end];
    let code = |digits: &str, radix: u32, most: usize| {
        let fits =
            (1..=most).contains(&digits.len()) && digits.chars().all(|ch| ch.is_digit(radix));
        let code = u32::from_str_radix(digits, radix).ok().filter(|_| fits)?;
        let ch = char::from_u32(code).filter(|ch| *ch != '\0');
        Some(ch.unwrap_or(char::REPLACEMENT_CHARACTER).to_string())
    };
    let value = match name.strip_prefix('#') {
        Some(hex) if hex.starts_with(['x', 'X']) => code(&hex[1..], 16, 6)?,
        Some(digits) => code(digits, 10, 7)?,
        None if name.chars().all(|ch| ch.is_ascii_alphanumeric()) => decode_named(name, true)?,
        None => return None,
    };
    Some((value, end + 2))
}

/// Where drawn byte `at` was written, matched from the end.
fn align(drawn: &str, written: &str, escapes: Escapes, at: usize) -> Option<usize> {
    if at >= drawn.len() {
        return Some(written.len());
    }
    let source = read(written, escapes);
    let mut j = source.len();
    let mut shown = drawn.char_indices().rev().peekable();
    while let Some((byte, d)) = shown.next() {
        let found = loop {
            let s = source[..j].last()?;
            // A join: a line's end and the marks after it.
            if d.is_whitespace() && (s.ends_line() || s.lead) {
                while source[..j].last().is_some_and(|s| s.lead) {
                    j -= 1;
                }
                j -= 1;
                let crlf = source[j].ch == '\n' && j > 0;
                if crlf && source[j - 1].ends_line() && source[j - 1].ch == '\r' {
                    j -= 1;
                }
                if d == '\n' && shown.peek().is_some_and(|(_, ch)| *ch == '\r') {
                    let (cr, _) = shown.next().expect("peeked");
                    if cr == at {
                        return Some(source[j].at);
                    }
                }
                break source[j].at;
            }
            if d == s.ch {
                j -= 1;
                break s.at;
            }
            // White space, line ends and marks the text dropped.
            if s.raw && (matches!(s.ch, ' ' | '\t') || s.ends_line() || s.lead) {
                j -= 1;
                continue;
            }
            return None;
        };
        if byte == at {
            return Some(found);
        }
    }
    None
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
    fn references_read_back_and_emoji_end_the_reading() {
        let source = "a &amp; b";
        assert_eq!(under(source, "b"), source.find('b').unwrap());
        let source = "ab :smile: y";
        assert_eq!(under(source, "y"), source.find('y').unwrap());
        assert_eq!(under(source, "b"), 0, "before the emoji: its start");
    }
}
