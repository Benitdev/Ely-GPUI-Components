//! Lists go on as typed; lines shift in and out.

use std::ops::Range;

use gpui::{Context, Window};
use markdown::mdast;

use super::MarkdownDocument;
use crate::documents::markdown::{span, tree, walk};

/// Spaces one indent step adds.
const STEP: usize = 4;

/// What Enter makes of a list line.
#[derive(Debug, PartialEq)]
pub(super) enum Next {
    /// A new item: this follows the line break.
    Item(String),
    /// An empty item ends the list: the line empties.
    End,
}

/// The item after `line`; none outside a list.
pub(super) fn next(line: &str) -> Option<Next> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    let rest = &line[indent..];
    let ruled = rest.chars().filter(|ch| "-*_".contains(*ch)).count() >= 3
        && rest.chars().all(|ch| "-*_ ".contains(ch));
    if ruled {
        return None;
    }
    let (mark, after) = bullet(rest).or_else(|| ordered(rest))?;
    let body = &rest[after..];
    let (task, body) = match body.get(..4) {
        Some("[ ] " | "[x] " | "[X] ") => ("[ ] ", &body[4..]),
        _ => ("", body),
    };
    if body.trim().is_empty() {
        return Some(Next::End);
    }
    Some(Next::Item(format!("{}{mark}{task}", " ".repeat(indent))))
}

/// Whether `at` sits in a list item, not code.
fn listed(source: &str, at: usize) -> bool {
    let root = tree(source);
    let mut nodes = Vec::new();
    walk(&root, &mut nodes);
    let held: Vec<&mdast::Node> = nodes
        .into_iter()
        .filter(|node| span(node).start <= at && at <= span(node).end)
        .collect();
    let item = held
        .iter()
        .any(|node| matches!(node, mdast::Node::ListItem(_)));
    item && !held
        .iter()
        .any(|node| matches!(node, mdast::Node::Code(_) | mdast::Node::Math(_)))
}

/// A bullet marker, and where its text starts.
fn bullet(rest: &str) -> Option<(String, usize)> {
    let mark = rest.chars().next().filter(|ch| "-*+".contains(*ch))?;
    rest[1..].starts_with(' ').then(|| (format!("{mark} "), 2))
}

/// A number marker, counted on; where text starts.
fn ordered(rest: &str) -> Option<(String, usize)> {
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    if !(1..=9).contains(&digits) {
        return None;
    }
    let close = rest[digits..]
        .chars()
        .next()
        .filter(|ch| ".)".contains(*ch))?;
    if !rest[digits + 1..].starts_with(' ') {
        return None;
    }
    let number: u64 = rest[..digits].parse().expect("ascii digits");
    Some((format!("{}{close} ", number + 1), digits + 2))
}

/// Lines `range` touches, a step in or out.
pub(super) fn shift(text: &str, range: Range<usize>, deeper: bool) -> (String, Range<usize>) {
    let first = text[..range.start].rfind('\n').map_or(0, |at| at + 1);
    let last = text[range.end..]
        .find('\n')
        .map_or(text.len(), |at| range.end + at);
    // Each change: where, and bytes added or removed.
    let mut changes: Vec<(usize, isize)> = Vec::new();
    let mut at = first;
    loop {
        let line = &text[at..text[at..].find('\n').map_or(text.len(), |end| at + end)];
        let spaces = line.len() - line.trim_start_matches(' ').len();
        match deeper {
            true => changes.push((at, STEP as isize)),
            false if spaces > 0 => changes.push((at, -(spaces.min(STEP) as isize))),
            false => {}
        }
        match text[at..].find('\n') {
            Some(end) if at + end < last => at += end + 1,
            _ => break,
        }
    }
    let mut out = String::with_capacity(text.len() + changes.len() * STEP);
    let mut from = 0;
    for (at, change) in &changes {
        out.push_str(&text[from..*at]);
        from = *at;
        match *change > 0 {
            true => out.push_str(&" ".repeat(STEP)),
            false => from += change.unsigned_abs(),
        }
    }
    out.push_str(&text[from..]);
    let moved = |pos: usize| {
        let mut shifted = pos as isize;
        for (at, change) in &changes {
            match *change > 0 {
                true if *at <= pos => shifted += change,
                false if *at < pos => shifted -= (pos - at).min(change.unsigned_abs()) as isize,
                _ => {}
            }
        }
        shifted as usize
    };
    (out, moved(range.start)..moved(range.end))
}

impl MarkdownDocument {
    /// Enter on a list line in source; false leaves it.
    pub(super) fn source_newline(&mut self, cx: &mut Context<Self>) -> bool {
        let editor = self.editor.read(cx);
        let caret = editor.primary().range();
        if editor.selections().len() != 1 || !caret.is_empty() {
            return false;
        }
        if !self.read_now(cx) {
            self.reparse(true, cx);
        }
        if !self.read_now(cx) {
            log::info!("markdown document: Enter while a long text is read");
            return false;
        }
        let editor = self.editor.read(cx);
        let text = editor.text();
        let ix = self.block_at(caret.start);
        let block = self.parsed.blocks.get(ix).map(|block| block.range.clone());
        let Some(block) =
            block.filter(|block| block.start <= caret.start && caret.start <= block.end)
        else {
            return false;
        };
        if !listed(&text[block.clone()], caret.start - block.start) {
            return false;
        }
        let start = text[..caret.start].rfind('\n').map_or(0, |at| at + 1);
        let end = text[caret.start..]
            .find('\n')
            .map_or(text.len(), |at| caret.start + at);
        let (edit, after) = match next(&text[start..end]) {
            Some(Next::Item(prefix)) => {
                let typed = format!("\n{prefix}");
                let after = caret.start + typed.len();
                ((caret.clone(), typed), after)
            }
            Some(Next::End) if caret.start == end => ((start..end, String::new()), start),
            _ => return false,
        };
        log::info!("markdown document: a list goes on in source");
        self.editor.update(cx, |editor, cx| {
            editor.edit([edit], cx);
            editor.select(std::iter::once(after..after), cx);
        });
        true
    }

    /// Enter on a block list line; false leaves it.
    pub(super) fn block_newline(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(open) = &self.open else {
            return false;
        };
        let input = open.input.clone();
        let (text, caret) = {
            let input = input.read(cx);
            (input.text().to_string(), input.selection())
        };
        if !caret.is_empty() || !listed(&text, caret.start) {
            return false;
        }
        let start = text[..caret.start].rfind('\n').map_or(0, |at| at + 1);
        let end = text[caret.start..]
            .find('\n')
            .map_or(text.len(), |at| caret.start + at);
        let (range, typed) = match next(&text[start..end]) {
            Some(Next::Item(prefix)) => (caret, format!("\n{prefix}")),
            Some(Next::End) if caret.start == end => (start..end, String::new()),
            _ => return false,
        };
        log::info!("markdown document: a list goes on in a block");
        input.update(cx, |input, cx| {
            input.select(range, cx);
            input.insert(&typed, cx);
        });
        true
    }

    /// Tab or Shift-Tab in the open block.
    pub(super) fn block_shift(&mut self, deeper: bool, _: &mut Window, cx: &mut Context<Self>) {
        let Some(open) = &self.open else {
            return;
        };
        let input = open.input.clone();
        let (text, selection) = {
            let input = input.read(cx);
            (input.text().to_string(), input.selection())
        };
        let (shifted, selection) = shift(&text, selection, deeper);
        input.update(cx, |input, cx| {
            input.set_text(shifted, cx);
            input.select(selection, cx);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{Next, next, shift};

    #[test]
    fn items_go_on_with_their_marker() {
        let item = |line| match next(line) {
            Some(Next::Item(prefix)) => prefix,
            other => panic!("{line:?} made {other:?}"),
        };
        assert_eq!(item("- a"), "- ");
        assert_eq!(item("  * a"), "  * ");
        assert_eq!(item("9. a"), "10. ");
        assert_eq!(item("3) a"), "4) ");
        assert_eq!(item("- [x] done"), "- [ ] ");
        assert_eq!(next("- "), Some(Next::End));
        assert_eq!(next("1. [ ] "), Some(Next::End));
        for plain in ["text", "-a", "* * *", "---", "1.a", ""] {
            assert_eq!(next(plain), None, "{plain:?}");
        }
    }

    #[test]
    fn lines_shift_in_and_out_with_the_selection() {
        assert_eq!(shift("a\nb", 0..0, true), ("    a\nb".into(), 4..4));
        assert_eq!(shift("a\nb", 1..3, true), ("    a\n    b".into(), 5..11));
        assert_eq!(shift("      a", 7..7, false), ("  a".into(), 3..3));
        assert_eq!(shift("  a\nb", 1..1, false), ("a\nb".into(), 0..0));
    }
}
