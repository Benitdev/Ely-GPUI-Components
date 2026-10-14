//! A command's result read back: its text and formats kept.

use std::ops::Range;

use markdown::mdast::Node;

use crate::documents::{
    document::format::EditRefusal,
    markdown::{file_tree, span, walk},
};

/// Refuses a result that reads as something else.
pub(super) fn misread(why: &str) -> EditRefusal {
    log::warn!("block command: {why}");
    EditRefusal::Misread
}

/// `text` with sorted `edits` written, parsed.
pub(super) fn reread(text: &str, edits: &[(Range<usize>, String)]) -> Node {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for (range, with) in edits {
        out.push_str(&text[at..range.start]);
        out.push_str(with);
        at = range.end;
    }
    out.push_str(&text[at..]);
    file_tree(&out)
}

/// Each shown character under `node`, with its formats.
pub(super) fn styled(node: &Node) -> Vec<(char, u16)> {
    let mut out = Vec::new();
    read(node, 0, &mut out);
    out
}

fn read(node: &Node, formats: u16, out: &mut Vec<(char, u16)>) {
    let (own, value) = match node {
        Node::Text(text) => (0, Some(text.value.as_str())),
        Node::Strong(_) => (1, None),
        Node::Emphasis(_) => (2, None),
        Node::Delete(_) => (4, None),
        Node::Link(_) | Node::LinkReference(_) => (8, None),
        Node::InlineCode(code) => (16, Some(code.value.as_str())),
        Node::InlineMath(math) => (32, Some(math.value.as_str())),
        Node::Html(html) => (64, Some(html.value.as_str())),
        Node::Image(image) => (128, Some(image.alt.as_str())),
        Node::ImageReference(image) => (128, Some(image.alt.as_str())),
        Node::FootnoteReference(note) => (256, Some(note.identifier.as_str())),
        Node::Code(code) => (512, Some(code.value.as_str())),
        Node::Math(math) => (1024, Some(math.value.as_str())),
        _ => (0, None),
    };
    let formats = formats | own;
    match value {
        Some(value) => out.extend(
            (value.chars())
                .filter(|ch| !ch.is_whitespace())
                .map(|ch| (ch, formats)),
        ),
        None => {
            for kid in node.children().map(Vec::as_slice).unwrap_or_default() {
                read(kid, formats, out);
            }
        }
    }
}

/// The level (0 a paragraph) and characters at `at`.
pub(super) fn block_at(root: &Node, at: usize) -> Option<(u8, Vec<(char, u16)>)> {
    let mut nodes = Vec::new();
    walk(root, &mut nodes);
    nodes.into_iter().find_map(|node| {
        let level = match node {
            Node::Paragraph(_) => 0,
            Node::Heading(heading) => heading.depth,
            _ => return None,
        };
        let whole = span(node);
        (whole.start <= at && at <= whole.end).then(|| (level, styled(node)))
    })
}
