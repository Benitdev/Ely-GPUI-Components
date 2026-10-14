//! Inline formats toggle at the selection, as Typora's (R4.6).

use std::ops::Range;

use gpui::Context;
use markdown::mdast::Node;

use super::MarkdownDocument;
use crate::documents::{
    MarkdownMode,
    markdown::{file_tree, span, walk},
    modes::joint,
    tree::inside,
};

/// An inline format of Typora's Format menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InlineFormat {
    Strong,
    Emphasis,
    Code,
    Strike,
    Underline,
    Link,
    Image,
    /// Every inline mark in the touched blocks.
    Clear,
}

/// Why a format did not happen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatRefusal {
    /// Preview takes no edits.
    Preview,
    Code,
    Math,
    Html,
    FrontMatter,
    /// No paragraph, heading or cell at the caret.
    Outside,
    /// The selection crosses a block or another mark.
    Across,
}

/// The edits and the selection after them.
pub(super) type Change = (Vec<(Range<usize>, String)>, Range<usize>);

/// Typora's word stops: punctuation and space.
fn stop(ch: char) -> bool {
    ch.is_whitespace() || "()[]^&_-+*#@!~`\\/{}'\":;<>,.?%".contains(ch)
}

/// The text a block holds: its children, past heading marks.
fn content(node: &Node) -> Range<usize> {
    let whole = span(node);
    match node.children().map(|kids| (kids.first(), kids.last())) {
        Some((Some(first), Some(last))) => span(first).start..span(last).end,
        _ => whole.end..whole.end,
    }
}

fn block(node: &Node) -> bool {
    matches!(
        node,
        Node::Paragraph(_) | Node::Heading(_) | Node::TableCell(_)
    )
}

/// A styled node's two marks, kept text between.
fn marks(node: &Node, text: &str) -> Option<[Range<usize>; 2]> {
    let whole = span(node);
    let inner = match node {
        Node::Strong(_) | Node::Emphasis(_) | Node::Delete(_) => content(node),
        Node::Link(_) | Node::LinkReference(_) => content(node),
        Node::InlineCode(_) => inside(text, whole.clone(), '`'),
        Node::Image(_) | Node::ImageReference(_) => {
            // Typora keeps the alt up to its first `]`.
            let end = whole.start + text[whole.clone()].find(']')?;
            whole.start + 2..end
        }
        _ => return None,
    };
    Some([whole.start..inner.start, inner.end..whole.end])
}

fn styled(format: InlineFormat, node: &Node) -> bool {
    match format {
        InlineFormat::Strong => matches!(node, Node::Strong(_)),
        InlineFormat::Emphasis => matches!(node, Node::Emphasis(_)),
        InlineFormat::Code => matches!(node, Node::InlineCode(_)),
        InlineFormat::Strike => matches!(node, Node::Delete(_)),
        InlineFormat::Link => matches!(node, Node::Link(_) | Node::LinkReference(_)),
        InlineFormat::Image => matches!(node, Node::Image(_) | Node::ImageReference(_)),
        InlineFormat::Underline | InlineFormat::Clear => false,
    }
}

/// `text[range]` without `holes`, sorted and inside it.
fn cut(text: &str, range: Range<usize>, holes: &[Range<usize>]) -> String {
    let mut out = String::new();
    let mut at = range.start;
    for hole in holes {
        out.push_str(&text[at..hole.start]);
        at = hole.end;
    }
    out.push_str(&text[at..range.end]);
    out
}

/// Byte `at` once `holes` are cut; inside one, its start.
fn after(at: usize, holes: &[Range<usize>]) -> usize {
    let gone: usize = holes
        .iter()
        .map(|hole| at.clamp(hole.start, hole.end) - hole.start)
        .sum();
    at - gone
}

/// Refuses an end inside code, math, HTML or front matter.
fn verbatim(
    nodes: &[&Node],
    selection: &Range<usize>,
    format: InlineFormat,
) -> Result<(), FormatRefusal> {
    let blocks: Vec<Range<usize>> = nodes
        .iter()
        .filter(|node| block(node))
        .map(|node| span(node))
        .collect();
    for node in nodes {
        let whole = span(node);
        let touches = |edge: bool| {
            [selection.start, selection.end]
                .iter()
                .any(|&at| match edge {
                    true => whole.start <= at && at <= whole.end,
                    false => whole.start < at && at < whole.end,
                })
        };
        let inline = blocks
            .iter()
            .any(|block| block.start <= whole.start && whole.end <= block.end);
        // Clear takes a block's inline code and tags too.
        let held = format != InlineFormat::Clear && touches(false);
        let why = match node {
            Node::Code(_) if touches(true) => FormatRefusal::Code,
            Node::Math(_) if touches(true) => FormatRefusal::Math,
            Node::Yaml(_) | Node::Toml(_) if touches(true) => FormatRefusal::FrontMatter,
            Node::InlineCode(_) if format != InlineFormat::Code && held => FormatRefusal::Code,
            Node::InlineMath(_) if held => FormatRefusal::Math,
            Node::Html(_) if (inline && held) || (!inline && touches(true)) => FormatRefusal::Html,
            _ => continue,
        };
        return Err(why);
    }
    Ok(())
}

/// The `<u>` and `</u>` around `s..e` in `within`.
fn underline(
    nodes: &[&Node],
    within: &Range<usize>,
    s: usize,
    e: usize,
) -> Option<[Range<usize>; 2]> {
    let tag = |node: &&&Node, name: &str| match node {
        Node::Html(html) => html.value.trim().eq_ignore_ascii_case(name),
        _ => false,
    };
    let tags: Vec<(&Node, Range<usize>)> = nodes
        .iter()
        .filter(|node| matches!(node, Node::Html(_)) && within.contains(&span(node).start))
        .map(|node| (*node, span(node)))
        .collect();
    let open = tags.iter().rfind(|(_, at)| at.end <= s)?;
    let close = tags.iter().find(|(_, at)| at.start >= e)?;
    let between = |from: usize, to: usize, name: &str| {
        tags.iter()
            .any(|(node, at)| at.start >= from && at.end <= to && tag(&node, name))
    };
    let paired = tag(&&open.0, "<u>") && tag(&&close.0, "</u>");
    (paired && !between(open.1.end, s, "</u>") && !between(e, close.1.start, "<u>"))
        .then(|| [open.1.clone(), close.1.clone()])
}

/// `format` toggled at `selection` in `text`, or why not.
pub(super) fn toggle(
    text: &str,
    selection: Range<usize>,
    format: InlineFormat,
) -> Result<Change, FormatRefusal> {
    let root = file_tree(text);
    let mut nodes = Vec::new();
    walk(&root, &mut nodes);
    verbatim(&nodes, &selection, format)?;
    if format == InlineFormat::Clear {
        return clear(text, &nodes, selection);
    }
    let picked = &text[selection.clone()];
    let s = selection.start + (picked.len() - picked.trim_start().len());
    let e = (selection.end - (picked.len() - picked.trim_end().len())).max(s);
    let holds = |whole: &Range<usize>| match s == e {
        true => whole.start < s && s < whole.end,
        false => whole.start <= s && e <= whole.end,
    };
    let unwrap = |range: Range<usize>, holes: [Range<usize>; 2]| -> Change {
        let written = cut(text, range.clone(), &holes);
        (vec![(range, written)], after(s, &holes)..after(e, &holes))
    };
    let found = (nodes.iter())
        .filter(|node| styled(format, node) && holds(&span(node)))
        .min_by_key(|node| span(node).len());
    if let Some(node) = found {
        return Ok(unwrap(
            span(node),
            marks(node, text).expect("a styled node has marks"),
        ));
    }
    let within = (nodes.iter())
        .filter(|node| block(node))
        .map(|node| content(node))
        .filter(|range| range.start <= s && e <= range.end)
        .min_by_key(|range| range.len());
    let Some(within) = within else {
        let line = text[..s]
            .rsplit('\n')
            .next()
            .unwrap_or_default()
            .to_string()
            + text[s..].split('\n').next().unwrap_or_default();
        return match s == e && line.trim().is_empty() {
            true => Ok(pair(s, format)),
            false => Err(FormatRefusal::Outside),
        };
    };
    if format == InlineFormat::Underline
        && let Some(tags) = underline(&nodes, &within, s, e)
    {
        return Ok(unwrap(tags[0].start..tags[1].end, tags));
    }
    if s != e {
        return wrap(text, &nodes, &within, s, e, format);
    }
    let prev = text[within.start..s].chars().next_back();
    let next = text[s..within.end].chars().next();
    if prev.is_none_or(stop) && next.is_none_or(stop) {
        crossed(text, &nodes, &within, s, s, format)?;
        return Ok(pair(s, format));
    }
    let word = (nodes.iter())
        .filter(|node| matches!(node, Node::Text(_)))
        .map(|node| span(node))
        .find(|run| run.start <= s && s <= run.end)
        .map(|run| {
            let start = (text[run.start..s].char_indices().rev())
                .find(|(_, ch)| stop(*ch))
                .map_or(run.start, |(at, ch)| run.start + at + ch.len_utf8());
            let end = s + text[s..run.end].find(stop).unwrap_or(run.end - s);
            start..end
        })
        .filter(|word| !word.is_empty());
    match word {
        Some(word) => wrap(text, &nodes, &within, word.start, word.end, format),
        None => {
            crossed(text, &nodes, &within, s, s, format)?;
            Ok(pair(s, format))
        }
    }
}

/// The marks `format` writes around `inner`.
fn written(format: InlineFormat, inner: &str) -> (String, String) {
    let (open, close) = match format {
        InlineFormat::Strong => ("**", "**"),
        InlineFormat::Emphasis => ("*", "*"),
        InlineFormat::Strike => ("~~", "~~"),
        InlineFormat::Underline => ("<u>", "</u>"),
        InlineFormat::Link => ("[", "]()"),
        InlineFormat::Image => ("![", "]()"),
        InlineFormat::Code => {
            let longest = inner.split(|ch| ch != '`').map(str::len).max().unwrap_or(0);
            let fence = "`".repeat(longest + 1);
            let pad = if inner.starts_with('`') || inner.ends_with('`') {
                " "
            } else {
                ""
            };
            return (format!("{fence}{pad}"), format!("{pad}{fence}"));
        }
        InlineFormat::Clear => unreachable!("clear writes no marks"),
    };
    (open.into(), close.into())
}

/// An empty pair at `at`, the caret inside it.
fn pair(at: usize, format: InlineFormat) -> Change {
    let (open, close) = written(format, "");
    let caret = at + open.len();
    (vec![(at..at, format!("{open}{close}"))], caret..caret)
}

/// Refuses `s..e` cutting a node or landing in its marks.
fn crossed(
    text: &str,
    nodes: &[&Node],
    within: &Range<usize>,
    s: usize,
    e: usize,
    format: InlineFormat,
) -> Result<(), FormatRefusal> {
    let merges = matches!(
        format,
        InlineFormat::Strong | InlineFormat::Emphasis | InlineFormat::Strike
    );
    for node in nodes {
        let whole = span(node);
        let merged = merges && styled(format, node) && s <= whole.start && whole.end <= e;
        if matches!(node, Node::Text(_))
            || merged
            || whole.start < within.start
            || whole.end > within.end
        {
            continue;
        }
        let holes = match node {
            Node::InlineMath(_) | Node::Html(_) | Node::Break(_) | Node::FootnoteReference(_) => {
                vec![whole.clone()]
            }
            _ => match marks(node, text) {
                Some(marks) => marks.to_vec(),
                None => continue,
            },
        };
        let holds = whole.start <= s && e <= whole.end;
        let cuts =
            whole.start < e && s < whole.end && !holds && !(s <= whole.start && whole.end <= e);
        let in_mark = |at: usize| holes.iter().any(|hole| hole.start < at && at < hole.end);
        if cuts || in_mark(s) || in_mark(e) {
            return Err(FormatRefusal::Across);
        }
    }
    Ok(())
}

/// `s..e` in `format`, same marks inside taken in.
fn wrap(
    text: &str,
    nodes: &[&Node],
    within: &Range<usize>,
    mut s: usize,
    mut e: usize,
    format: InlineFormat,
) -> Result<Change, FormatRefusal> {
    if matches!(
        format,
        InlineFormat::Strong | InlineFormat::Emphasis | InlineFormat::Strike
    ) {
        for node in nodes.iter().filter(|node| styled(format, node)) {
            let whole = span(node);
            if whole.start < e
                && s < whole.end
                && within.start <= whole.start
                && whole.end <= within.end
            {
                (s, e) = (s.min(whole.start), e.max(whole.end));
            }
        }
    }
    crossed(text, nodes, within, s, e, format)?;
    let mut holes: Vec<Range<usize>> = (nodes.iter())
        .filter(|node| styled(format, node) && s <= span(node).start && span(node).end <= e)
        .flat_map(|node| marks(node, text).expect("a styled node has marks"))
        .filter(|hole| !hole.is_empty())
        .collect();
    holes.sort_by_key(|hole| hole.start);
    let inner = cut(text, s..e, &holes);
    let (open, close) = written(format, &inner);
    let at = s + open.len();
    let selection = match format {
        InlineFormat::Link | InlineFormat::Image => at + inner.len() + 2..at + inner.len() + 2,
        _ => at..at + inner.len(),
    };
    Ok((vec![(s..e, format!("{open}{inner}{close}"))], selection))
}

/// Every inline mark in the blocks `selection` touches.
fn clear(text: &str, nodes: &[&Node], selection: Range<usize>) -> Result<Change, FormatRefusal> {
    let blocks: Vec<Range<usize>> = (nodes.iter())
        .filter(|node| block(node))
        .map(|node| content(node))
        .filter(|range| range.start <= selection.end && selection.start <= range.end)
        .collect();
    if blocks.is_empty() {
        return Err(FormatRefusal::Outside);
    }
    let held = |hole: &Range<usize>| {
        blocks
            .iter()
            .any(|block| block.start <= hole.start && hole.end <= block.end)
    };
    let mut holes: Vec<Range<usize>> = (nodes.iter())
        .flat_map(|node| match node {
            Node::Html(_) => vec![span(node)],
            _ => marks(node, text).map(Vec::from).unwrap_or_default(),
        })
        .filter(|hole| !hole.is_empty() && held(hole))
        .collect();
    holes.sort_by_key(|hole| hole.start);
    let edits = (blocks.iter())
        .filter_map(|block| {
            let inside: Vec<Range<usize>> = (holes.iter())
                .filter(|hole| block.start <= hole.start && hole.end <= block.end)
                .cloned()
                .collect();
            (!inside.is_empty()).then(|| (block.clone(), cut(text, block.clone(), &inside)))
        })
        .collect();
    Ok((
        edits,
        after(selection.start, &holes)..after(selection.end, &holes),
    ))
}

impl MarkdownDocument {
    /// `format` at the selection, Typora's way, or why not.
    pub fn format(
        &mut self,
        format: InlineFormat,
        cx: &mut Context<Self>,
    ) -> Result<(), FormatRefusal> {
        if self.mode == MarkdownMode::Preview {
            return Err(FormatRefusal::Preview);
        }
        let text = self.editor.read(cx).text();
        let (full, offset, selection) = match &self.open {
            Some(open) => {
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
            None => (text.to_string(), 0, self.editor.read(cx).primary().range()),
        };
        let (edits, selected) = toggle(&full, selection, format).inspect_err(|why| {
            log::info!("markdown document: format {format:?} refused: {why:?}");
        })?;
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
