use std::ops::Range;

use markdown::mdast;

use crate::documents::{
    markdown::{file_tree, span, walk},
    tree::{Node, Reader},
};

/// A top-level block: its source, and what it draws.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Block {
    pub range: Range<usize>,
    /// None shows the source as written: definitions, HTML, front matter.
    pub nodes: Option<Vec<Node>>,
}

/// A heading anywhere in the file.
#[derive(Clone, Debug, PartialEq)]
pub struct DocumentHeading {
    pub depth: u8,
    pub title: String,
    /// Where its source starts, in bytes.
    pub at: usize,
}

/// A file read as blocks and headings.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Parsed {
    /// The text read, to compare blocks across reads.
    pub text: String,
    pub blocks: Vec<Block>,
    pub headings: Vec<DocumentHeading>,
}

/// `text` read whole; every byte is in or between blocks.
pub(crate) fn parsed(text: &str) -> Parsed {
    let root = file_tree(text);
    let reader = Reader::new(text, &root);
    let blocks = root
        .children()
        .expect("the root holds the blocks")
        .iter()
        .map(|node| {
            let range = span(node);
            let kept = text[range.clone()].trim_end_matches(['\n', '\r']).len();
            let raw = matches!(
                node,
                mdast::Node::Definition(_)
                    | mdast::Node::Html(_)
                    | mdast::Node::Yaml(_)
                    | mdast::Node::Toml(_)
            );
            let nodes = (!raw).then(|| {
                let mut out = Vec::new();
                reader.block(node, &mut out);
                out
            });
            Block {
                range: range.start..range.start + kept,
                nodes,
            }
        })
        .collect();
    let mut nodes = Vec::new();
    walk(&root, &mut nodes);
    let headings = nodes
        .into_iter()
        .filter_map(|node| match node {
            mdast::Node::Heading(heading) => Some(DocumentHeading {
                depth: heading.depth,
                title: reader.inline(&heading.children).text,
                at: span(node).start,
            }),
            _ => None,
        })
        .collect();
    Parsed {
        text: text.to_string(),
        blocks,
        headings,
    }
}

/// Blocks alike at both ends: `(head, tail)` counts to keep.
pub(crate) fn alike(old: &Parsed, new: &Parsed) -> (usize, usize) {
    let same = |a: &Block, b: &Block| {
        a.nodes == b.nodes && old.text[a.range.clone()] == new.text[b.range.clone()]
    };
    let head = old
        .blocks
        .iter()
        .zip(&new.blocks)
        .take_while(|(a, b)| same(a, b))
        .count();
    let room = old.blocks.len().min(new.blocks.len()) - head;
    let tail = old
        .blocks
        .iter()
        .rev()
        .zip(new.blocks.iter().rev())
        .take(room)
        .take_while(|(a, b)| same(a, b))
        .count();
    (head, tail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_block_is_shown_raw_ones_as_written() {
        let text = "---\ntitle: a\n---\n\n# Lift\n\n[site]: https://ely.dev\n\n<div>x</div>\n\n> ## Quoted\n";
        let found = parsed(text);
        let shown: Vec<(&str, bool)> = found
            .blocks
            .iter()
            .map(|block| (&text[block.range.clone()], block.nodes.is_some()))
            .collect();
        assert_eq!(
            shown,
            [
                ("---\ntitle: a\n---", false),
                ("# Lift", true),
                ("[site]: https://ely.dev", false),
                ("<div>x</div>", false),
                ("> ## Quoted", true),
            ]
        );
        let headings: Vec<(u8, &str, usize)> = found
            .headings
            .iter()
            .map(|heading| (heading.depth, heading.title.as_str(), heading.at))
            .collect();
        assert_eq!(headings, [(1, "Lift", 18), (2, "Quoted", 67)]);
    }

    #[test]
    fn an_edit_keeps_the_blocks_around_it() {
        let old = parsed("# A\n\nb\n\nc\n\nd\n");
        let new = parsed("# A\n\nb and more\n\nc\n\nd\n");
        assert_eq!(alike(&old, &new), (1, 2));
        assert_eq!(alike(&old, &old), (4, 0), "all kept from the head");
        let shorter = parsed("# A\n");
        assert_eq!(alike(&old, &shorter), (1, 0));
    }

    #[test]
    fn a_reference_resolves_across_blocks() {
        let found = parsed("[site]\n\n[site]: https://ely.dev\n");
        let Some(nodes) = &found.blocks[0].nodes else {
            panic!("the paragraph draws")
        };
        let Node::Paragraph(text) = &nodes[0] else {
            panic!("a paragraph, not {nodes:?}")
        };
        assert_eq!(text.links, [(0..4, "https://ely.dev".to_string())]);
    }
}
