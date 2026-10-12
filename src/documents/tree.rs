use std::{collections::HashMap, ops::Range};

use markdown::mdast::{self, AlignKind};

use super::markdown::{span, tree, walk};

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

/// Inline text, its styled stretches and link addresses.
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
    /// Items and blocks; ordered from `start`; tasks carry checkboxes.
    List {
        start: Option<u64>,
        items: Vec<(Option<bool>, Vec<Node>)>,
    },
    /// Each cell's blocks: its text, split at pictures.
    Table {
        aligns: Vec<AlignKind>,
        head: Vec<Vec<Node>>,
        rows: Vec<Vec<Vec<Node>>>,
    },
    Rule,
    Math(String),
    /// A picture, and the link around it.
    Image {
        alt: String,
        url: String,
        link: Option<String>,
    },
    Footnote {
        label: String,
        body: Vec<Node>,
    },
}

/// A tree read against its source and its link definitions.
pub(crate) struct Reader<'a> {
    source: &'a str,
    urls: HashMap<String, String>,
}

/// Inline text being built, and the containers open around it.
#[derive(Default)]
struct Run {
    text: Inline,
    open: Vec<(usize, Style, Option<String>)>,
}

impl Run {
    /// Text so far; open styles close here, reopen at zero.
    fn cut(&mut self) -> Inline {
        let mut done = std::mem::take(&mut self.text);
        let end = done.text.len();
        for (start, style, url) in self.open.iter_mut().rev() {
            if *start < end {
                done.styles.push((*start..end, *style));
                if let Some(url) = url {
                    done.links.push((*start..end, url.clone()));
                }
            }
            *start = 0;
        }
        done
    }

    /// The innermost open link's address.
    fn link(&self) -> Option<String> {
        self.open.iter().rev().find_map(|(_, _, url)| url.clone())
    }
}

/// `markdown` as blocks.
pub fn parse(markdown: &str) -> Vec<Node> {
    let root = tree(markdown);
    let mut out = Vec::new();
    Reader::new(markdown, &root).blocks(
        root.children().expect("the root holds the blocks"),
        &mut out,
    );
    out
}

impl<'a> Reader<'a> {
    /// A reader of `root`, parsed from `source`.
    pub(crate) fn new(source: &'a str, root: &mdast::Node) -> Self {
        let mut nodes = Vec::new();
        walk(root, &mut nodes);
        let mut urls = HashMap::new();
        for node in nodes {
            if let mdast::Node::Definition(definition) = node {
                urls.entry(definition.identifier.clone())
                    .or_insert_with(|| definition.url.clone());
            }
        }
        Self { source, urls }
    }

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

    /// A reference's address; the parser checked its definition.
    fn url(&self, identifier: &str) -> String {
        self.urls
            .get(identifier)
            .cloned()
            .expect("a reference names a definition")
    }

    pub(crate) fn block(&self, node: &mdast::Node, out: &mut Vec<Node>) {
        match node {
            mdast::Node::Paragraph(paragraph) => self.paragraph(node, &paragraph.children, out),
            mdast::Node::Heading(heading) => {
                let (mut run, mut after) = (Run::default(), Vec::new());
                for child in &heading.children {
                    self.inline_into(child, &mut run, &mut Images::After(&mut after));
                }
                out.push(Node::Heading(heading.depth, run.cut()));
                out.extend(after);
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
                        .map(|cell| {
                            let mut blocks = Vec::new();
                            let inlines = cell.children().expect("a cell holds inlines");
                            self.paragraph(cell, inlines, &mut blocks);
                            blocks
                        })
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

    /// Text split at its images; lone `$$` math is block.
    fn paragraph(&self, node: &mdast::Node, children: &[mdast::Node], out: &mut Vec<Node>) {
        if let [mdast::Node::InlineMath(math)] = children
            && self.source[span(node)].starts_with("$$")
        {
            return out.push(Node::Math(math.value.clone()));
        }
        let mut run = Run::default();
        for child in children {
            self.inline_into(child, &mut run, &mut Images::Split(out));
        }
        text_block(run.cut(), out);
    }

    /// Inline text where images draw as their alt text.
    pub(crate) fn inline(&self, nodes: &[mdast::Node]) -> Inline {
        let mut run = Run::default();
        for node in nodes {
            self.inline_into(node, &mut run, &mut Images::Alt);
        }
        run.cut()
    }

    fn inline_into(&self, node: &mdast::Node, run: &mut Run, images: &mut Images) {
        let text = &mut run.text;
        let (children, style, url) = match node {
            mdast::Node::Text(words) => {
                let soft = words.value.replace("\r\n", " ").replace(['\r', '\n'], " ");
                return text.text.push_str(&soft);
            }
            mdast::Node::Break(_) => return text.text.push('\n'),
            mdast::Node::InlineCode(code) => return styled(text, &code.value, Style::Code),
            mdast::Node::InlineMath(math) => return styled(text, &math.value, Style::Math),
            mdast::Node::FootnoteReference(note) => {
                let label = note.label.as_ref().unwrap_or(&note.identifier);
                return styled(text, &format!("[{label}]"), Style::Footnote);
            }
            mdast::Node::Image(_) | mdast::Node::ImageReference(_) => {
                let (alt, url) = match node {
                    mdast::Node::Image(image) => (image.alt.clone(), image.url.clone()),
                    mdast::Node::ImageReference(image) => {
                        (image.alt.clone(), self.url(&image.identifier))
                    }
                    _ => unreachable!("matched as an image"),
                };
                let link = run.link();
                return match images {
                    Images::Alt => run.text.text.push_str(&alt),
                    Images::Split(out) => {
                        text_block(run.cut(), out);
                        out.push(Node::Image { alt, url, link })
                    }
                    Images::After(out) => out.push(Node::Image { alt, url, link }),
                };
            }
            mdast::Node::Html(_) => return,
            mdast::Node::Strong(strong) => (&strong.children, Style::Strong, None),
            mdast::Node::Emphasis(emphasis) => (&emphasis.children, Style::Emphasis, None),
            mdast::Node::Delete(delete) => (&delete.children, Style::Strike, None),
            mdast::Node::Link(link) => (&link.children, Style::Link, Some(link.url.clone())),
            mdast::Node::LinkReference(link) => (
                &link.children,
                Style::Link,
                Some(self.url(&link.identifier)),
            ),
            other => unreachable!("{other:?} is no inline outside MDX"),
        };
        run.open.push((run.text.text.len(), style, url));
        for child in children {
            self.inline_into(child, run, images);
        }
        let (start, style, url) = run.open.pop().expect("the container opened above");
        let end = run.text.text.len();
        if start < end {
            run.text.styles.push((start..end, style));
            if let Some(url) = url {
                run.text.links.push((start..end, url));
            }
        }
    }
}

/// Where an inline image goes.
enum Images<'a> {
    /// Its alt text stays in the line.
    Alt,
    /// It splits the text into blocks.
    Split(&'a mut Vec<Node>),
    /// It follows the text, as a heading's.
    After(&'a mut Vec<Node>),
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
                url: "outer.png".into(),
                link: None
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
                url: "a.png".into(),
                link: None
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
        for (source, shown) in [
            ("one\ntwo  \nthree", "one two\nthree"),
            ("one\r\ntwo  \r\nthree", "one two\nthree"),
            ("one\rtwo  \rthree", "one two\nthree"),
        ] {
            let nodes = parse(source);
            assert!(
                matches!(&nodes[0], Node::Paragraph(text) if text.text == shown),
                "{source:?} reads {nodes:?}"
            );
        }
    }

    #[test]
    fn definitions_nested_in_quotes_and_lists_resolve() {
        for source in [
            "[site] ![pic]\n\n> [site]: https://ely.dev\n> [pic]: a.png\n",
            "[site] ![pic]\n\n- [site]: https://ely.dev\n- [pic]: a.png\n",
        ] {
            let nodes = parse(source);
            let Node::Paragraph(text) = &nodes[0] else {
                panic!("a paragraph, not {:?}", nodes[0])
            };
            assert_eq!(text.links, [(0..4, "https://ely.dev".to_string())]);
            assert_eq!(
                nodes[1],
                Node::Image {
                    alt: "pic".into(),
                    url: "a.png".into(),
                    link: None
                }
            );
        }
    }

    #[test]
    fn an_image_inside_styles_and_links_stays_an_image() {
        let nodes = parse("*a ![x](x.png) b* [![y](y.png)](https://ely.dev)");
        let Node::Paragraph(before) = &nodes[0] else {
            panic!("a paragraph, not {:?}", nodes[0])
        };
        assert_eq!(
            (before.text.as_str(), &before.styles[..]),
            ("a ", &[(0..2, Style::Emphasis)][..])
        );
        assert_eq!(
            nodes[1],
            Node::Image {
                alt: "x".into(),
                url: "x.png".into(),
                link: None
            }
        );
        let Node::Paragraph(after) = &nodes[2] else {
            panic!("a paragraph, not {:?}", nodes[2])
        };
        assert_eq!(
            (after.text.as_str(), &after.styles[..]),
            (" b ", &[(0..2, Style::Emphasis)][..])
        );
        assert_eq!(
            nodes[3],
            Node::Image {
                alt: "y".into(),
                url: "y.png".into(),
                link: Some("https://ely.dev".into())
            }
        );
        assert_eq!(nodes.len(), 4, "{nodes:?}");
    }

    #[test]
    fn a_heading_keeps_its_images_after_it() {
        let nodes = parse("# Lift ![x](x.png)");
        assert!(matches!(&nodes[0], Node::Heading(1, text) if text.text == "Lift "));
        assert!(matches!(&nodes[1], Node::Image { url, .. } if url == "x.png"));
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
        let text = |cell: &[Node]| match cell {
            [Node::Paragraph(line)] => line.text.clone(),
            other => panic!("one line, not {other:?}"),
        };
        assert_eq!(
            head.iter().map(|cell| text(cell)).collect::<Vec<_>>(),
            ["Name", "Lift"]
        );
        assert_eq!(text(&rows[0][1]), "0.5");
    }

    #[test]
    fn a_picture_in_a_cell_stays_a_picture() {
        let nodes = parse("| Picture |\n| --- |\n| ![shot](assets/shot.png) |\n");
        let Node::Table { rows, .. } = &nodes[0] else {
            panic!("a table, not {:?}", nodes[0])
        };
        assert!(
            matches!(&rows[0][0][..], [Node::Image { alt, url, .. }] if alt == "shot" && url == "assets/shot.png")
        );
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
