//! Blocks put in and taken out, as Typora's `insertBlock`.

use std::collections::HashSet;

use markdown::mdast::Node;

use super::{
    BlockCommand, Here, container, eol, inside, line_end, line_start, next_line, placed, prefix,
    prev_line, refusal,
};
use crate::documents::{
    document::format::{Change, EditRefusal},
    markdown::span,
};

/// Where `line`'s text starts inside `containers`; a lazy line's own.
fn content(text: &str, line: usize, containers: &[&Node]) -> usize {
    let at = inside(text, line, containers).unwrap_or(line);
    at + text[at..].len() - text[at..].trim_start_matches([' ', '\t']).len()
}

/// Whether `line` holds text past `containers`' prefix.
fn text_on(text: &str, line: usize, containers: &[&Node]) -> bool {
    let at = content(text, line, containers);
    at < line_end(text, line)
}

/// Back from `at` over blanks and prefixes, to `floor`.
fn back(text: &str, mut at: usize, floor: usize, containers: &[&Node]) -> usize {
    loop {
        let line = line_start(text, at).max(floor);
        at = line + text[line..at].trim_end_matches([' ', '\t']).len();
        if line == floor || at > content(text, line, containers) {
            return at;
        }
        at = line_end(text, prev_line(text, line).expect("a line above the floor"));
    }
}

/// On from `at` over blanks and prefixes, to `ceil`.
fn forth(text: &str, mut at: usize, ceil: usize, containers: &[&Node]) -> usize {
    loop {
        let end = line_end(text, at).min(ceil);
        at = end - text[at..end].trim_start_matches([' ', '\t']).len();
        if at < end {
            return at;
        }
        match next_line(text, at) {
            Some(next) if next < ceil => at = content(text, next, containers),
            _ => return ceil,
        }
    }
}

/// The selection's text, line by line, prefixes cut.
fn picked(here: &Here, containers: &[&Node]) -> Vec<String> {
    let text = here.text;
    let (s, e) = (here.selection.start, here.selection.end);
    let mut out = Vec::new();
    let mut at = s;
    while at < e {
        out.push(text[at..line_end(text, at).min(e)].to_string());
        match next_line(text, at).filter(|next| *next < e) {
            Some(next) => at = inside(text, next, containers).unwrap_or(next).min(e),
            None => break,
        }
    }
    out
}

/// The smallest footnote number not taken.
fn free_note(here: &Here) -> usize {
    let used: HashSet<&str> = (here.nodes.iter())
        .filter_map(|node| match node {
            Node::FootnoteDefinition(note) => Some(note.identifier.as_str()),
            Node::FootnoteReference(note) => Some(note.identifier.as_str()),
            _ => None,
        })
        .collect();
    (1..)
        .find(|n: &usize| !used.contains(n.to_string().as_str()))
        .expect("a free number")
}

/// `command`'s lines and the caret's line and byte.
fn lines(
    here: &Here,
    command: BlockCommand,
    body: Vec<String>,
) -> (Vec<String>, Option<(usize, usize)>) {
    let fenced = |fence: String, body: Vec<String>| {
        let mut lines = vec![fence.clone()];
        lines.extend(if body.is_empty() {
            vec![String::new()]
        } else {
            body
        });
        lines.push(fence);
        (lines, Some((1, 0)))
    };
    match command {
        BlockCommand::CodeBlock => {
            let longest = (body.iter())
                .flat_map(|line| line.split(|ch| ch != '`'))
                .map(str::len)
                .max()
                .unwrap_or(0);
            fenced("`".repeat(longest.max(2) + 1), body)
        }
        BlockCommand::MathBlock => fenced("$$".to_string(), body),
        BlockCommand::Toc => (vec!["[toc]".to_string()], Some((0, 5))),
        BlockCommand::Footnote => {
            let line = format!("[^{}]: ", free_note(here));
            let end = line.len();
            (vec![line], Some((0, end)))
        }
        BlockCommand::LinkDefinition => {
            let first = body.first().map_or("", |line| line.trim());
            if first.contains(['/', '\\']) {
                let address: String = first.chars().take(160).collect();
                return (vec![format!("[]: {address}")], Some((0, 1)));
            }
            let label: String = (first.split_whitespace().next().unwrap_or_default())
                .chars()
                .take(20)
                .collect();
            let line = format!("[{label}]: ");
            let caret = if label.is_empty() { 1 } else { line.len() };
            (vec![line], Some((0, caret)))
        }
        _ => (vec!["---".to_string()], None),
    }
}

/// `lines` joined inside `lead`, and the caret's byte in them.
fn joined(
    lines: &[String],
    caret: Option<(usize, usize)>,
    nl: &str,
    lead: &str,
) -> (String, Option<usize>) {
    let mut out = String::new();
    let mut at = None;
    for (ix, line) in lines.iter().enumerate() {
        if ix > 0 {
            out.push_str(nl);
            out.push_str(lead);
        }
        if let Some((row, byte)) = caret
            && row == ix
        {
            at = Some(out.len() + byte);
        }
        out.push_str(line);
    }
    (out, at)
}

/// Whether a paragraph or ATX heading splits at the caret.
fn splits(text: &str, node: &Node) -> bool {
    match node {
        Node::Paragraph(_) => true,
        Node::Heading(_) => text[span(node)].trim_start().starts_with('#'),
        _ => false,
    }
}

/// Where the new block's text goes, ending the edit.
struct Room<'a> {
    nl: &'static str,
    lead: String,
    blank: String,
    containers: Vec<&'a Node>,
}

impl Room<'_> {
    /// A blank line, then a new line's prefix.
    fn gap(&self) -> String {
        format!("{}{}{}{}", self.nl, self.blank, self.nl, self.lead)
    }
}

pub(super) fn run(here: &Here, command: BlockCommand) -> Result<Change, EditRefusal> {
    let text = here.text;
    let leaf = here.leaf();
    let containers = here.containers(here.path.len() - usize::from(leaf.is_some()));
    let lead = prefix(text, &containers);
    let room = Room {
        nl: eol(text),
        blank: lead.trim_end().to_string(),
        lead,
        containers,
    };
    let (lines, caret) = lines(here, command, picked(here, &room.containers));
    let (block, inner) = joined(&lines, caret, room.nl, &room.lead);
    let rule = inner.is_none();
    match leaf {
        Some(node) if splits(text, node) => {
            if here.selection.end > span(node).end {
                return Err(EditRefusal::Across);
            }
            Ok(split(here, node, &room, &block, inner))
        }
        Some(node) if !here.selection.is_empty() => Err(refusal(node)),
        Some(node) => {
            let at = line_end(text, span(node).end);
            let next =
                next_line(text, at).is_some_and(|next| text_on(text, next, &room.containers));
            let gap = room.gap();
            let tail = match rule {
                true => gap.clone(),
                false => String::new(),
            };
            let sep = if next {
                format!("{}{}", room.nl, room.blank)
            } else {
                String::new()
            };
            let caret = at + gap.len() + inner.unwrap_or(block.len() + tail.len());
            Ok(placed(
                vec![(at..at, format!("{gap}{block}{tail}{sep}"))],
                caret,
            ))
        }
        None if here.blank() && here.selection.is_empty() => {
            let line = line_start(text, here.selection.start);
            let at = inside(text, line, &room.containers).expect("a blank line holds its prefix");
            let end = line_end(text, line);
            let prev =
                prev_line(text, line).is_some_and(|prev| text_on(text, prev, &room.containers));
            let next =
                next_line(text, line).is_some_and(|next| text_on(text, next, &room.containers));
            let head = if prev {
                format!("{}{}", room.nl, room.lead)
            } else {
                String::new()
            };
            let tail = if rule { room.gap() } else { String::new() };
            let sep = if next {
                format!("{}{}", room.nl, room.blank)
            } else {
                String::new()
            };
            let caret = at + head.len() + inner.unwrap_or(block.len() + tail.len());
            Ok(placed(
                vec![(at..end, format!("{head}{block}{tail}{sep}"))],
                caret,
            ))
        }
        None => Err(EditRefusal::Outside),
    }
}

/// `block` put where the caret splits `node`.
fn split(here: &Here, node: &Node, room: &Room, block: &str, inner: Option<usize>) -> Change {
    let text = here.text;
    let whole = span(node);
    let kids = node.children().map(Vec::as_slice).unwrap_or_default();
    let (marks, body) = match node {
        Node::Heading(heading) => (
            "#".repeat(usize::from(heading.depth)) + " ",
            kids.first().map_or(whole.end, |kid| span(kid).start)
                ..kids.last().map_or(whole.end, |kid| span(kid).end),
        ),
        _ => (String::new(), whole.clone()),
    };
    let containers = &room.containers;
    let mut from = back(text, here.selection.start, body.start, containers);
    let after = forth(text, here.selection.end, body.end, containers);
    let before = from > body.start;
    let follows = after < body.end;
    if !before {
        from = whole.start;
    }
    let to = if follows { after } else { whole.end };
    let gap = room.gap();
    let mut written = String::new();
    if before {
        written.push_str(&gap);
    }
    let start = written.len();
    written.push_str(block);
    let caret = match inner {
        Some(inner) => start + inner,
        None => {
            written.push_str(&gap);
            written.len() + if follows { marks.len() } else { 0 }
        }
    };
    if follows {
        if inner.is_some() {
            written.push_str(&gap);
        }
        written.push_str(&marks);
    }
    let mut edits = Vec::new();
    let mut shift = 0;
    if !before && here.joins_before(&whole, None) {
        let line = line_start(text, whole.start);
        let blank = format!("{}{}", room.blank, room.nl);
        shift = blank.len();
        edits.push((line..line, blank));
    }
    edits.push((from..to, written));
    placed(edits, from + shift + caret)
}

/// An empty line before or after the caret's block.
pub(super) fn paragraph(here: &Here, before: bool) -> Result<Change, EditRefusal> {
    let text = here.text;
    let leaf = here.leaf().ok_or(EditRefusal::Outside)?;
    let whole = span(leaf);
    let containers = here.containers(here.path.len() - 1);
    let lead = prefix(text, &containers);
    let room = Room {
        nl: eol(text),
        blank: lead.trim_end().to_string(),
        lead,
        containers,
    };
    let gap = room.gap();
    if before {
        let mut edits = Vec::new();
        let mut shift = 0;
        if here.joins_before(&whole, None) {
            let line = line_start(text, whole.start);
            let blank = format!("{}{}", room.blank, room.nl);
            shift = blank.len();
            edits.push((line..line, blank));
        }
        edits.push((whole.start..whole.start, gap));
        return Ok(placed(edits, whole.start + shift));
    }
    let at = line_end(text, whole.end);
    let next = next_line(text, at).is_some_and(|next| text_on(text, next, &room.containers));
    let sep = if next {
        format!("{}{}", room.nl, room.blank)
    } else {
        String::new()
    };
    Ok(placed(
        vec![(at..at, format!("{gap}{sep}"))],
        at + gap.len(),
    ))
}

/// Front matter at the top, unless there is some.
pub(super) fn front_matter(here: &Here) -> Result<Change, EditRefusal> {
    let first = here.root.children().and_then(|kids| kids.first());
    if matches!(first, Some(Node::Yaml(_) | Node::Toml(_))) {
        return Err(EditRefusal::Present);
    }
    let nl = eol(here.text);
    let tail = match here.text.is_empty() || here.text.starts_with(['\n', '\r']) {
        true => "",
        false => nl,
    };
    Ok(placed(
        vec![(0..0, format!("---{nl}{nl}---{nl}{tail}"))],
        3 + nl.len(),
    ))
}

/// The caret's block gone; a lone paragraph takes its holder.
pub(super) fn delete(here: &Here) -> Result<Change, EditRefusal> {
    let text = here.text;
    let leaf = here.leaf().ok_or(EditRefusal::Outside)?;
    let mut ix = here.path.len() - 1;
    if matches!(leaf, Node::Paragraph(_))
        && let Some(parent) = here.path.get(ix - 1)
        && container(parent)
        && parent.children().is_some_and(|kids| kids.len() == 1)
    {
        ix -= 1;
    }
    let whole = span(here.path[ix]);
    let outer = here.containers(ix);
    let line = line_start(text, whole.start);
    if !text[line..whole.start]
        .chars()
        .all(|ch| matches!(ch, ' ' | '\t' | '>'))
    {
        return Ok(placed(vec![(whole.clone(), String::new())], whole.start));
    }
    let mut end = next_line(text, whole.end).unwrap_or(text.len());
    let blank_above = prev_line(text, line).is_none_or(|prev| !text_on(text, prev, &outer));
    if blank_above && end < text.len() && !text_on(text, end, &outer) {
        end = next_line(text, end).unwrap_or(text.len());
    }
    Ok(placed(vec![(line..end, String::new())], line))
}
