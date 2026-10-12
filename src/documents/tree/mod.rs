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
mod tests;
