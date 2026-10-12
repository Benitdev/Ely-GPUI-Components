use std::{collections::HashMap, ops::Range};

use markdown::mdast::{self, AlignKind};

use super::markdown::{span, tree};

/// How a stretch of inline text prints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Strong,
    Emphasis,
    Strike,
    Code,
    Link,
    Math,
    Footnote,
}

/// Inline content: its text, the styled stretches of it, and the address of each link, in order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Inline {
    pub text: String,
    pub styles: Vec<(Range<usize>, Style)>,
    pub links: Vec<(Range<usize>, String)>,
}

/// A block of a markdown document.
#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Paragraph(Inline),
    Heading(u8, Inline),
    Quote(Vec<Node>),
    Code {
        language: Option<String>,
        text: String,
    },
    /// Items with their blocks, numbered from `start` when ordered, each with its checkbox when a task.
    List {
        start: Option<u64>,
        items: Vec<(Option<bool>, Vec<Node>)>,
    },
    Table {
        aligns: Vec<AlignKind>,
        head: Vec<Inline>,
        rows: Vec<Vec<Inline>>,
    },
    Rule,
    Math(String),
    Image {
        alt: String,
        url: String,
    },
    Footnote {
        label: String,
        body: Vec<Node>,
    },
}

/// A tree read against its source and its link definitions.
struct Reader<'a> {
    source: &'a str,
    urls: HashMap<String, String>,
}

/// `markdown` as blocks.
pub fn parse(markdown: &str) -> Vec<Node> {
    let root = tree(markdown);
    let blocks = root.children().expect("the root holds the blocks");
    let mut urls = HashMap::new();
    for block in blocks {
        if let mdast::Node::Definition(definition) = block {
            urls.entry(definition.identifier.clone())
                .or_insert_with(|| definition.url.clone());
        }
    }
    let reader = Reader {
        source: markdown,
        urls,
    };
    let mut out = Vec::new();
    reader.blocks(blocks, &mut out);
    out
}

impl Reader<'_> {
    fn blocks(&self, nodes: &[mdast::Node], out: &mut Vec<Node>) {
        for node in nodes {
            self.block(node, out);
        }
    }

    fn nested(&self, nodes: &[mdast::Node]) -> Vec<Node> {
        let mut out = Vec::new();
        self.blocks(nodes, &mut out);
        out
    }

    /// The address a reference names: the parser makes references to definitions alone.
    fn url(&self, identifier: &str) -> String {
        self.urls
            .get(identifier)
            .cloned()
            .expect("a reference names a definition")
    }

    fn block(&self, node: &mdast::Node, out: &mut Vec<Node>) {
        match node {
            mdast::Node::Paragraph(paragraph) => self.paragraph(node, &paragraph.children, out),
            mdast::Node::Heading(heading) => {
                out.push(Node::Heading(heading.depth, self.inline(&heading.children)))
            }
            mdast::Node::Blockquote(quote) => out.push(Node::Quote(self.nested(&quote.children))),
            mdast::Node::Code(code) => out.push(Node::Code {
                language: code.lang.clone(),
                text: code.value.clone(),
            }),
            mdast::Node::Math(math) => out.push(Node::Math(math.value.clone())),
            mdast::Node::List(list) => {
                let items = list
                    .children
                    .iter()
                    .map(|item| match item {
                        mdast::Node::ListItem(item) => (item.checked, self.nested(&item.children)),
                        other => unreachable!("a list holds items, not {other:?}"),
                    })
                    .collect();
                let start = list.ordered.then(|| u64::from(list.start.unwrap_or(1)));
                out.push(Node::List { start, items });
            }
            mdast::Node::Table(table) => {
                let mut rows = table.children.iter().map(|row| match row {
                    mdast::Node::TableRow(row) => row
                        .children
                        .iter()
                        .map(|cell| self.inline(cell.children().expect("a cell holds inlines")))
                        .collect::<Vec<_>>(),
                    other => unreachable!("a table holds rows, not {other:?}"),
                });
                let head = rows.next().expect("a table has its head row");
                out.push(Node::Table {
                    aligns: table.align.clone(),
                    head,
                    rows: rows.collect(),
                });
            }
            mdast::Node::ThematicBreak(_) => out.push(Node::Rule),
            mdast::Node::FootnoteDefinition(note) => out.push(Node::Footnote {
                label: note
                    .label
                    .clone()
                    .unwrap_or_else(|| note.identifier.clone()),
                body: self.nested(&note.children),
            }),
            mdast::Node::Html(_) | mdast::Node::Definition(_) => {}
            other => unreachable!("{other:?} is no block outside MDX"),
        }
    }

    /// A paragraph's text split around its images; `$$` math written alone stands as a block.
    fn paragraph(&self, node: &mdast::Node, children: &[mdast::Node], out: &mut Vec<Node>) {
        if let [mdast::Node::InlineMath(math)] = children
            && self.source[span(node)].starts_with("$$")
        {
            return out.push(Node::Math(math.value.clone()));
        }
        let mut run = Inline::default();
        for child in children {
            let image = match child {
                mdast::Node::Image(image) => Some((image.alt.clone(), image.url.clone())),
                mdast::Node::ImageReference(image) => {
                    Some((image.alt.clone(), self.url(&image.identifier)))
                }
                _ => None,
            };
            match image {
                Some((alt, url)) => {
                    text_block(std::mem::take(&mut run), out);
                    out.push(Node::Image { alt, url });
                }
                None => self.inline_into(child, &mut run),
            }
        }
        text_block(run, out);
    }

    fn inline(&self, nodes: &[mdast::Node]) -> Inline {
        let mut text = Inline::default();
        for node in nodes {
            self.inline_into(node, &mut text);
        }
        text
    }

    /// `nodes` into `text`; their stretch of it, styled.
    fn styled_children(&self, nodes: &[mdast::Node], text: &mut Inline) -> Range<usize> {
        let start = text.text.len();
        for node in nodes {
            self.inline_into(node, text);
        }
        start..text.text.len()
    }

    fn inline_into(&self, node: &mdast::Node, text: &mut Inline) {
        let (children, style) = match node {
            mdast::Node::Text(words) => return text.text.push_str(&words.value.replace('\n', " ")),
            mdast::Node::Break(_) => return text.text.push('\n'),
            mdast::Node::InlineCode(code) => return styled(text, &code.value, Style::Code),
            mdast::Node::InlineMath(math) => return styled(text, &math.value, Style::Math),
            mdast::Node::FootnoteReference(note) => {
                let label = note.label.as_ref().unwrap_or(&note.identifier);
                return styled(text, &format!("[{label}]"), Style::Footnote);
            }
            mdast::Node::Image(image) => return text.text.push_str(&image.alt),
            mdast::Node::ImageReference(image) => return text.text.push_str(&image.alt),
            mdast::Node::Html(_) => return,
            mdast::Node::Strong(strong) => (&strong.children, Style::Strong),
            mdast::Node::Emphasis(emphasis) => (&emphasis.children, Style::Emphasis),
            mdast::Node::Delete(delete) => (&delete.children, Style::Strike),
            mdast::Node::Link(link) => (&link.children, Style::Link),
            mdast::Node::LinkReference(link) => (&link.children, Style::Link),
            other => unreachable!("{other:?} is no inline outside MDX"),
        };
        let range = self.styled_children(children, text);
        text.styles.push((range.clone(), style));
        match node {
            mdast::Node::Link(link) => text.links.push((range, link.url.clone())),
            mdast::Node::LinkReference(link) => {
                text.links.push((range, self.url(&link.identifier)))
            }
            _ => {}
        }
    }
}

/// Text left between images, unless it is blank.
fn text_block(text: Inline, out: &mut Vec<Node>) {
    if !text.text.trim().is_empty() {
        out.push(Node::Paragraph(text));
    }
}

fn styled(inline: &mut Inline, text: &str, style: Style) {
    let start = inline.text.len();
    inline.text.push_str(text);
    inline.styles.push((start..inline.text.len(), style));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_nest_as_written() {
        let nodes = parse(
            "# Lift\n\nBlend **toward** [white](https://ely.dev).\n\n- [x] clamp\n- [ ] gamma\n\n> quoted\n\n```rust\nfn main() {}\n```\n\n---\n",
        );
        assert!(matches!(&nodes[0], Node::Heading(1, text) if text.text == "Lift"));
        let Node::Paragraph(line) = &nodes[1] else {
            panic!("a paragraph, not {:?}", nodes[1])
        };
        assert_eq!(line.text, "Blend toward white.");
        assert_eq!(line.styles, [(6..12, Style::Strong), (13..18, Style::Link)]);
        assert_eq!(line.links, [(13..18, "https://ely.dev".to_string())]);
        let Node::List { start: None, items } = &nodes[2] else {
            panic!("a list, not {:?}", nodes[2])
        };
        assert_eq!(
            items.iter().map(|(task, _)| *task).collect::<Vec<_>>(),
            [Some(true), Some(false)]
        );
        assert!(matches!(&items[0].1[0], Node::Paragraph(text) if text.text == "clamp"));
        assert!(matches!(&nodes[3], Node::Quote(inner) if inner.len() == 1));
        assert_eq!(
            nodes[4],
            Node::Code {
                language: Some("rust".into()),
                text: "fn main() {}".into()
            }
        );
        assert_eq!(nodes[5], Node::Rule);
    }

    #[test]
    fn an_image_inside_an_image_is_the_outer_one() {
        // markdown-rs drops the inner image from the alt.
        assert_eq!(
            parse("![outer ![inner](inner.png)](outer.png)"),
            [Node::Image {
                alt: "outer ".into(),
                url: "outer.png".into()
            }]
        );
    }

    #[test]
    fn an_image_splits_its_paragraph_and_references_resolve() {
        let nodes =
            parse("Before ![a][pic] after [site].\n\n[pic]: a.png\n[site]: https://ely.dev\n");
        assert!(matches!(&nodes[0], Node::Paragraph(text) if text.text == "Before "));
        assert_eq!(
            nodes[1],
            Node::Image {
                alt: "a".into(),
                url: "a.png".into()
            }
        );
        let Node::Paragraph(after) = &nodes[2] else {
            panic!("a paragraph, not {:?}", nodes[2])
        };
        assert_eq!(after.text, " after site.");
        assert_eq!(after.links, [(7..11, "https://ely.dev".to_string())]);
        assert_eq!(nodes.len(), 3, "definitions draw nothing: {nodes:?}");
    }

    #[test]
    fn a_soft_break_reads_as_a_space_and_a_hard_one_as_a_line() {
        let nodes = parse("one\ntwo  \nthree");
        assert!(matches!(&nodes[0], Node::Paragraph(text) if text.text == "one two\nthree"));
    }

    #[test]
    fn display_math_stands_alone() {
        assert_eq!(parse("$$c = 1$$"), [Node::Math("c = 1".into())]);
        assert_eq!(parse("$$\nc = 1\n$$"), [Node::Math("c = 1".into())]);
        let Node::Paragraph(inline) = &parse("so $c$ holds")[0] else {
            panic!("inline math stays in its paragraph")
        };
        assert_eq!(inline.styles, [(3..4, Style::Math)]);
    }

    #[test]
    fn tables_keep_their_cells_and_alignment() {
        let nodes = parse("| Name | Lift |\n|:--|--:|\n| accent | 0.5 |\n");
        let Node::Table { aligns, head, rows } = &nodes[0] else {
            panic!("a table, not {:?}", nodes[0])
        };
        assert_eq!(aligns, &[AlignKind::Left, AlignKind::Right]);
        assert_eq!(
            head.iter()
                .map(|cell| cell.text.as_str())
                .collect::<Vec<_>>(),
            ["Name", "Lift"]
        );
        assert_eq!(rows[0][1].text, "0.5");
    }

    #[test]
    fn footnotes_number_their_references_and_hold_their_body() {
        let nodes = parse("Lift[^a].\n\n[^a]: Toward white.\n");
        let Node::Paragraph(line) = &nodes[0] else {
            panic!("a paragraph, not {:?}", nodes[0])
        };
        assert_eq!(line.text, "Lift[a].");
        assert_eq!(line.styles, [(4..7, Style::Footnote)]);
        assert!(
            matches!(&nodes[1], Node::Footnote { label, body } if label == "a" && body.len() == 1)
        );
    }
}
