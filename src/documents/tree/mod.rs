use std::{collections::HashMap, ops::Range};

use markdown::mdast::{self, AlignKind};

use super::markdown::{span, tree, walk};

mod blocks;
mod html;

pub use blocks::{AlertKind, DocumentHeading};

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
    Underline,
    /// A highlighted run, as `<mark>`.
    Mark,
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
    /// A GitHub alert: a quote under its kind's title.
    Alert {
        kind: AlertKind,
        body: Vec<Node>,
    },
    /// `[toc]`: the file's headings.
    Toc(Vec<DocumentHeading>),
    /// Front matter, its fences cut.
    FrontMatter(String),
}

/// A tree read against its source and its link definitions.
pub(crate) struct Reader<'a> {
    source: &'a str,
    urls: HashMap<String, String>,
    /// Footnotes numbered by their first reference.
    notes: HashMap<String, usize>,
    pub headings: Vec<DocumentHeading>,
}

/// An open element, as HTML names it.
struct Opened {
    name: String,
    start: usize,
    style: Option<Style>,
    url: Option<String>,
}

/// Inline text being built, and the containers open around it.
#[derive(Default)]
struct Run {
    text: Inline,
    open: Vec<Opened>,
    /// How many of each name are open.
    named: HashMap<String, usize>,
    /// Inside an autolink: its text stays as written.
    verbatim: bool,
    /// Filtered tags open: their text keeps shortcodes.
    filtered: usize,
    /// White space owed, and the open elements before it.
    owed: Option<(char, usize)>,
}

impl Run {
    /// Text so far; open styles close here, reopen at zero.
    fn cut(&mut self) -> Inline {
        let mut done = std::mem::take(&mut self.text);
        let end = done.text.len();
        for opened in self.open.iter_mut().rev() {
            close(&mut done, opened, end);
            opened.start = 0;
        }
        done
    }

    fn count(&self, name: &str) -> usize {
        self.named.get(name).copied().unwrap_or(0)
    }

    /// Outside autolinks, `<code>` and filtered tags.
    fn reads_emoji(&self) -> bool {
        !self.verbatim && self.count("code") == 0 && self.filtered == 0
    }

    /// Text as shown: shortcodes read, lines joined.
    fn push_text(&mut self, written: &str) {
        match self.reads_emoji() {
            true => self.push_plain(&html::emoji(written)),
            false => self.push_plain(written),
        }
    }

    fn push_plain(&mut self, shown: &str) {
        let soft = shown.replace("\r\n", " ").replace(['\r', '\n'], " ");
        self.text.text.push_str(&soft);
    }

    /// Counts a filtered tag opened or closed.
    fn filter(&mut self, tag: &html::Tag) {
        self.filtered = match tag.closing {
            true => self.filtered.saturating_sub(1),
            false => self.filtered + 1,
        };
    }

    fn push(&mut self, opened: Opened) {
        *self.named.entry(opened.name.clone()).or_default() += 1;
        self.open.push(opened);
    }

    /// The innermost open link's address.
    fn link(&self) -> Option<String> {
        self.open.iter().rev().find_map(|opened| opened.url.clone())
    }

    /// Opens or closes an element for an inline HTML tag.
    fn html(&mut self, written: &str) {
        let Some(tag) = html::tag(written) else {
            return;
        };
        if tag.name == "br" {
            return self.text.text.push('\n');
        }
        if html::filtered(&tag.name) {
            self.filter(&tag);
            return self.push_plain(&html::text(written));
        }
        let Some(style) = html::inline_style(&tag.name) else {
            return;
        };
        if tag.closing {
            return self.end(&tag.name);
        }
        let url = match style {
            Style::Link => tag.attribute("href").map(str::to_string),
            _ => None,
        };
        let style = (style != Style::Link || url.is_some()).then_some(style);
        self.start(&tag.name, style, url);
    }

    /// Opens `name`; a new `a` ends the open one.
    fn start(&mut self, name: &str, style: Option<Style>, url: Option<String>) {
        if name == "a" {
            self.end("a");
        }
        self.push(Opened {
            name: name.to_string(),
            start: self.text.text.len(),
            style,
            url,
        });
    }

    /// Ends the innermost `name`, as HTML5 repairs nesting.
    fn end(&mut self, name: &str) {
        if self.count(name) == 0 {
            return;
        }
        let at = self
            .open
            .iter()
            .rposition(|opened| opened.name == name)
            .expect("a counted name is open");
        let end = self.text.text.len();
        if let Some((_, from)) = &mut self.owed {
            *from = (*from).min(at);
        }
        let closed = self.open.split_off(at);
        for opened in &closed {
            close(&mut self.text, opened, end);
            *self.named.get_mut(&opened.name).expect("counted") -= 1;
        }
        for opened in closed.into_iter().skip(1) {
            if html::reopens(&opened.name) {
                self.push(Opened {
                    start: end,
                    ..opened
                });
            }
        }
    }
}

/// Records `opened` as styled up to `end`.
fn close(text: &mut Inline, opened: &Opened, end: usize) {
    if opened.start >= end {
        return;
    }
    if let Some(style) = opened.style {
        text.styles.push((opened.start..end, style));
    }
    if let Some(url) = &opened.url {
        text.links.push((opened.start..end, url.clone()));
    }
}

/// `markdown` as blocks.
pub fn parse(markdown: &str) -> Vec<Node> {
    let root = tree(markdown);
    let (reader, mut out) = (Reader::new(markdown, &root), Vec::new());
    for node in root.children().expect("the root holds the blocks") {
        reader.top(node, &mut out);
    }
    out
}

impl<'a> Reader<'a> {
    /// A reader of `root`, parsed from `source`.
    pub(crate) fn new(source: &'a str, root: &mdast::Node) -> Self {
        let mut nodes = Vec::new();
        walk(root, &mut nodes);
        let (mut urls, mut notes, mut headings) = (HashMap::new(), HashMap::new(), Vec::new());
        for node in nodes {
            match node {
                mdast::Node::Definition(definition) => {
                    urls.entry(definition.identifier.clone())
                        .or_insert_with(|| definition.url.clone());
                }
                mdast::Node::FootnoteReference(note) => {
                    let next = notes.len() + 1;
                    notes.entry(note.identifier.clone()).or_insert(next);
                }
                mdast::Node::Heading(heading) => headings.push((heading, span(node).start)),
                _ => {}
            }
        }
        let mut reader = Self {
            source,
            urls,
            notes,
            headings: Vec::new(),
        };
        reader.headings = headings
            .into_iter()
            .map(|(heading, at)| DocumentHeading {
                depth: heading.depth,
                title: reader.inline(&heading.children).text,
                at,
            })
            .collect();
        reader
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
                label: match self.notes.get(&note.identifier) {
                    Some(number) => number.to_string(),
                    None => note
                        .label
                        .clone()
                        .unwrap_or_else(|| note.identifier.clone()),
                },
                body: self.nested(&note.children),
            }),
            mdast::Node::Html(html) => self.html_block(&html.value, out),
            mdast::Node::Yaml(yaml) => out.push(Node::FrontMatter(yaml.value.clone())),
            mdast::Node::Toml(toml) => out.push(Node::FrontMatter(toml.value.clone())),
            mdast::Node::Definition(_) => {}
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
        let (children, (name, style), url) = match node {
            mdast::Node::Text(words) => return run.push_text(&words.value),
            mdast::Node::Break(_) => return text.text.push('\n'),
            mdast::Node::InlineCode(code) => return styled(text, &code.value, Style::Code),
            mdast::Node::InlineMath(math) => return styled(text, &math.value, Style::Math),
            mdast::Node::FootnoteReference(note) => {
                let number = self.notes[&note.identifier];
                return styled(text, &html::superscript(number), Style::Footnote);
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
            mdast::Node::Html(written) => return run.html(&written.value),
            mdast::Node::Strong(strong) => (&strong.children, ("strong", Style::Strong), None),
            mdast::Node::Emphasis(emphasis) => (&emphasis.children, ("em", Style::Emphasis), None),
            mdast::Node::Delete(delete) => (&delete.children, ("del", Style::Strike), None),
            mdast::Node::Link(link) => (&link.children, ("a", Style::Link), Some(link.url.clone())),
            mdast::Node::LinkReference(link) => (
                &link.children,
                ("a", Style::Link),
                Some(self.url(&link.identifier)),
            ),
            other => unreachable!("{other:?} is no inline outside MDX"),
        };
        // A container is the element GitHub writes.
        run.start(name, Some(style), url);
        let autolink =
            matches!(node, mdast::Node::Link(_)) && !self.source[span(node)].starts_with('[');
        let outer = run.verbatim;
        run.verbatim |= autolink;
        for child in children {
            self.inline_into(child, run, images);
        }
        run.verbatim = outer;
        run.end(name);
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
