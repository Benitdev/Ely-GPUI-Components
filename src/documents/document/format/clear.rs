//! Clear Format: every inline mark off the touched blocks.

use std::ops::Range;

use markdown::mdast::Node;

use super::{Change, FormatRefusal, after, block, cut, marks, room};
use crate::documents::{markdown::span, tree::inside};

/// The backslashes of escapes in a text node's span.
fn escapes(text: &str, whole: Range<usize>) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut chars = text[whole.clone()].char_indices().peekable();
    while let Some((at, ch)) = chars.next() {
        if ch == '\\'
            && chars
                .peek()
                .is_some_and(|(_, next)| next.is_ascii_punctuation())
        {
            out.push(whole.start + at..whole.start + at + 1);
            chars.next();
        }
    }
    out
}

/// A node's inline marks, as Typora's `.md-meta`.
fn meta(node: &Node, text: &str) -> Vec<Range<usize>> {
    let whole = span(node);
    match node {
        Node::Html(_) => vec![whole],
        Node::Text(_) => escapes(text, whole),
        Node::InlineMath(_) => {
            let inner = inside(text, whole.clone(), '$');
            vec![whole.start..inner.start, inner.end..whole.end]
        }
        Node::FootnoteReference(_) => vec![whole.start..whole.start + 2, whole.end - 1..whole.end],
        _ => marks(node, text).map(Vec::from).unwrap_or_default(),
    }
}

/// Every inline mark in the blocks `selection` touches.
pub(super) fn clear(
    text: &str,
    nodes: &[&Node],
    selection: Range<usize>,
) -> Result<Change, FormatRefusal> {
    let blocks: Vec<Range<usize>> = (nodes.iter())
        .filter(|node| block(node))
        .map(|node| room(node, text))
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
        .flat_map(|node| meta(node, text))
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
