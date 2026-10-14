use markdown::mdast;

use super::{Node, Reader, Run, html, text_block};
use crate::documents::markdown::span;

/// A heading anywhere in the file.
#[derive(Clone, Debug, PartialEq)]
pub struct DocumentHeading {
    pub depth: u8,
    pub title: String,
    /// Where its source starts, in bytes.
    pub at: usize,
}

/// A GitHub alert's kind, named by its marker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertKind {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
}

impl AlertKind {
    /// The title GitHub shows.
    pub fn title(self) -> &'static str {
        match self {
            Self::Note => "Note",
            Self::Tip => "Tip",
            Self::Important => "Important",
            Self::Warning => "Warning",
            Self::Caution => "Caution",
        }
    }

    /// The kind `line` marks, as `[!NOTE]` in any case.
    fn marked(line: &str) -> Option<Self> {
        let name = line
            .trim_matches([' ', '\t'])
            .strip_prefix("[!")?
            .strip_suffix(']')?;
        [
            Self::Note,
            Self::Tip,
            Self::Important,
            Self::Warning,
            Self::Caution,
        ]
        .into_iter()
        .find(|kind| kind.title().eq_ignore_ascii_case(name))
    }
}

impl Reader<'_> {
    /// A root block; alerts and `[toc]` live only here.
    pub(crate) fn top(&self, node: &mdast::Node, out: &mut Vec<Node>) {
        if let Some(alert) = self.alert(node) {
            return out.push(alert);
        }
        let toc = matches!(node, mdast::Node::Paragraph(_))
            && self.source[span(node)].trim().eq_ignore_ascii_case("[toc]");
        match toc {
            true => out.push(Node::Toc(self.headings.clone())),
            false => self.block(node, out),
        }
    }

    /// A quote whose first line marks it, with more after.
    fn alert(&self, node: &mdast::Node) -> Option<Node> {
        let mdast::Node::Blockquote(quote) = node else {
            return None;
        };
        let [mdast::Node::Paragraph(paragraph), rest @ ..] = &quote.children[..] else {
            return None;
        };
        let (kind, after) = marker(&paragraph.children)?;
        if after.is_empty() && rest.is_empty() {
            return None;
        }
        let mut body = Vec::new();
        if !after.is_empty() {
            let kept = mdast::Paragraph {
                children: after,
                position: paragraph.position.clone(),
            };
            self.block(&mdast::Node::Paragraph(kept), &mut body);
        }
        self.blocks(rest, &mut body);
        Some(Node::Alert { kind, body })
    }

    /// An HTML block's text, as a browser shows it.
    pub(super) fn html_block(&self, written: &str, out: &mut Vec<Node>) {
        let mut run = Run::default();
        let mut rest = written;
        while let Some(at) = rest.find('<') {
            run.words(&rest[..at]);
            rest = &rest[at..];
            if let Some(comment) = rest.strip_prefix("<!--") {
                rest = comment.split_once("-->").map_or("", |(_, after)| after);
                continue;
            }
            let opens =
                rest[1..].starts_with(|ch: char| ch.is_ascii_alphabetic() || "/!?".contains(ch));
            if !opens {
                run.words("<");
                rest = &rest[1..];
                continue;
            }
            // An unclosed tag drops the rest, as HTML5 does.
            let Some(end) = html::tag_end(rest) else {
                rest = "";
                break;
            };
            let (written, after) = rest.split_at(end);
            rest = after;
            match html::tag(written) {
                Some(tag) if html::filtered(&tag.name) => {
                    run.filter(&tag);
                    run.spaced(&html::text(written));
                }
                // A block closes what opened inside it.
                Some(tag) if html::breaks(&tag.name) => {
                    run.line();
                    match tag.closing {
                        true => run.end(&tag.name),
                        false => {
                            run.implied(&tag.name);
                            if tag.name != "hr" {
                                run.start(&tag.name, None, None);
                            }
                        }
                    }
                }
                // A line's end drops its space.
                Some(tag) if tag.closing || tag.name == "br" => {
                    run.owed = run.owed.filter(|_| tag.name != "br");
                    run.html(written);
                }
                Some(_) => {
                    run.pay();
                    run.html(written);
                }
                None => {}
            }
        }
        run.words(rest);
        text_block(run.cut(), out);
    }
}

impl Run {
    /// HTML text: references decoded, white space collapsed.
    fn words(&mut self, written: &str) {
        let decoded = html::text(written);
        match self.reads_emoji() {
            true => self.spaced(&html::emoji(&decoded)),
            false => self.spaced(&decoded),
        }
    }

    /// Text with its white space collapsed.
    fn spaced(&mut self, shown: &str) {
        for ch in shown.chars() {
            if !html::spacing(ch) {
                self.pay();
                self.text.text.push(ch);
            } else if self.owed.is_none() && !self.text.text.ends_with('\n') {
                let open = self.open.len();
                self.owed = (!self.text.text.is_empty()).then_some((' ', open));
            }
        }
    }

    /// A line owed where a block element starts or ends.
    fn line(&mut self) {
        let from = self.owed.map_or(self.open.len(), |(_, from)| from);
        let text = &self.text.text;
        self.owed = (!(text.is_empty() || text.ends_with('\n'))).then_some(('\n', from));
    }

    /// Writes the white space owed, before a word or tag.
    fn pay(&mut self) {
        let Some((owed, from)) = self.owed.take() else {
            return;
        };
        self.text.text.push(owed);
        let start = self.text.text.len();
        // What opened after it starts after it.
        for opened in &mut self.open[from..] {
            opened.start = start;
        }
    }

    /// Ends what a block's start tag closes, as HTML5 does.
    fn implied(&mut self, name: &str) {
        let items: &[&str] = match name {
            "li" => &["li"],
            "dd" | "dt" => &["dd", "dt"],
            _ => &[],
        };
        // The search stops at a special element.
        let stops = |name: &str| html::breaks(name) && !matches!(name, "address" | "div" | "p");
        let found = items
            .iter()
            .any(|item| self.count(item) > 0)
            .then(|| {
                let mut names = self.open.iter().rev().map(|opened| opened.name.as_str());
                names.find(|name| items.contains(name) || stops(name))
            })
            .flatten()
            .filter(|name| items.contains(name))
            .map(str::to_string);
        if let Some(item) = found {
            self.end(&item);
        }
        // A table start closes it, so it is in scope.
        if name != "tr" && self.count("p") > 0 {
            self.end("p");
        }
    }
}

/// A plain first line naming a kind, and what follows.
fn marker(children: &[mdast::Node]) -> Option<(AlertKind, Vec<mdast::Node>)> {
    let mut line = String::new();
    for (at, child) in children.iter().enumerate() {
        let head = match child {
            mdast::Node::Text(text) => match text.value.find(['\n', '\r']) {
                None => {
                    line.push_str(&text.value);
                    continue;
                }
                Some(end) => {
                    line.push_str(&text.value[..end]);
                    let after = &text.value[end..];
                    let after = after.strip_prefix("\r\n").unwrap_or(&after[1..]);
                    let mut kept = text.clone();
                    kept.value = after.to_string();
                    (!after.is_empty()).then_some(mdast::Node::Text(kept))
                }
            },
            mdast::Node::Break(_) => None,
            _ => return None,
        };
        let after = head.into_iter().chain(children[at + 1..].iter().cloned());
        return Some((AlertKind::marked(&line)?, after.collect()));
    }
    Some((AlertKind::marked(&line)?, Vec::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::tree::{Inline, Style, parse};

    fn alert(source: &str) -> Option<(AlertKind, Vec<Node>)> {
        match parse(source).into_iter().next() {
            Some(Node::Alert { kind, body }) => Some((kind, body)),
            _ => None,
        }
    }

    fn text(nodes: &[Node]) -> Vec<String> {
        nodes
            .iter()
            .map(|node| match node {
                Node::Paragraph(line) => line.text.clone(),
                Node::Heading(_, line) => format!("# {}", line.text),
                other => format!("{other:?}"),
            })
            .collect()
    }

    #[test]
    fn alerts_read_as_github_reads_them() {
        let note = alert("> [!NOTE]\n> one").expect("an alert");
        assert_eq!(
            (note.0, text(&note.1)),
            (AlertKind::Note, vec!["one".into()])
        );
        let cases = [
            ("> [!tip]\n> two", Some(AlertKind::Tip)),
            ("> [!NOTE]   \n> trailing", Some(AlertKind::Note)),
            (">   [!TIP]\n> leading", Some(AlertKind::Tip)),
            ("> [!Warning]\n>\n> after a blank", Some(AlertKind::Warning)),
            ("> [!WARNING] same line", None),
            ("> [!IMPORTANT]", None),
            ("> [!FOO]\n> five", None),
            ("> text\n> [!NOTE]", None),
            ("> \\[!NOTE]\n> escaped", Some(AlertKind::Note)),
            ("> [!NOT&#69;]\n> referenced", Some(AlertKind::Note)),
            ("> [!NOTE]  \n> hard line", Some(AlertKind::Note)),
            ("> [!NOTE]\rcarriage", Some(AlertKind::Note)),
            ("> [!NOTE]\nlazy", Some(AlertKind::Note)),
            ("> [!NOTE]\u{a0}\n> no-break space", None),
            ("> **[!TIP]**\n> strong", None),
            ("> <span>[!WARNING]</span>\n> tag", None),
            ("[!NOTE]: https://example.com\n\n> [!NOTE]\n> a link", None),
        ];
        for (source, kind) in cases {
            assert_eq!(alert(source).map(|found| found.0), kind, "{source:?}");
        }
        let nested = parse("- > [!CAUTION]\n  > in a list");
        assert!(matches!(&nested[0], Node::List { .. }), "{nested:?}");
        let blocks = alert("> [!NOTE]\n> # head\n> - item").expect("an alert");
        assert!(matches!(
            &blocks.1[..],
            [Node::Heading(1, _), Node::List { .. }]
        ));
        let styled = alert("> [!CAUTION]\n> a **b**\n> c").expect("an alert");
        let Node::Paragraph(line) = &styled.1[0] else {
            panic!("a paragraph, not {:?}", styled.1)
        };
        let bold = 2..3;
        assert_eq!(
            (line.text.as_str(), &line.styles[..]),
            ("a b c", &[(bold, Style::Strong)][..])
        );
    }

    #[test]
    fn a_toc_paragraph_lists_every_heading() {
        let nodes = parse("[toc]\n\n# A\n\n> ## B :smile:\n");
        let Node::Toc(headings) = &nodes[0] else {
            panic!("a toc, not {nodes:?}")
        };
        let listed: Vec<(u8, &str, usize)> = headings
            .iter()
            .map(|heading| (heading.depth, heading.title.as_str(), heading.at))
            .collect();
        assert_eq!(listed, [(1, "A", 7), (2, "B 😄", 14)]);
        assert!(matches!(&parse("  [TOC]  ")[0], Node::Toc(_)));
        assert!(matches!(&parse("[toc] now")[0], Node::Paragraph(_)));
        assert!(matches!(&parse("> [toc]")[0], Node::Quote(_)));
    }

    fn html(source: &str) -> Option<Inline> {
        match parse(source).into_iter().next() {
            Some(Node::Paragraph(line)) => Some(line),
            None => None,
            other => panic!("a paragraph, not {other:?}"),
        }
    }

    #[test]
    fn an_html_block_shows_its_text_as_a_browser_does() {
        let centered = html(
            "<div align=\"center\">\n  <b>Bold</b>   and\n  :smile: &amp; <code>:smile:</code>\n</div>",
        )
        .expect("text");
        assert_eq!(centered.text, "Bold and 😄 & :smile:");
        let (bold, code) = (0..4, 16..23);
        assert_eq!(
            centered.styles,
            [(bold, Style::Strong), (code, Style::Code)]
        );
        let parts = html("<p>one <b>two </b></p>\n<p>three &lt;b&gt;</p>").expect("text");
        assert_eq!(parts.text, "one two\nthree <b>");
        assert_eq!(
            html("<div>\na <br> b</br>c</div>").expect("text").text,
            "a\nb\nc"
        );
        let filtered = "<div>\n<script>x</script>\n<!-- note -->\nshown<style>p{}</style>\n</div>";
        assert_eq!(
            html(filtered).expect("text").text,
            "<script>x</script> shown<style>p{}</style>"
        );
        let crossed = "<div>before<script>a</scriptx>b</script>after</div>";
        assert_eq!(
            html(crossed).expect("text").text,
            "before<script>ab</script>after"
        );
        let split = parse("<div>\n<script>\nfirst\n\nsecond\n</script>\n</div>");
        assert_eq!(text(&split), ["<script> first", "second </script>"]);
        let inline = html("a <script>b</script> c").expect("text");
        assert_eq!(inline.text, "a <script>b</script> c");
        assert_eq!(html("<div>\na < b <i").expect("text").text, "a < b");
    }

    #[test]
    fn a_closing_block_ends_what_opened_inside_it() {
        let marked = html("<div><mark>x</div>y").expect("text");
        assert_eq!(
            (marked.text.as_str(), &marked.styles[..]),
            ("x\ny", &[(0..1, Style::Mark)][..])
        );
        let bold = html("<div><b>x</div>y").expect("text");
        assert_eq!(bold.styles, [(0..1, Style::Strong), (2..3, Style::Strong)]);
        let kept = html("<div><mark><p>x</p>y</mark></div>").expect("text");
        assert_eq!(kept.styles, [(0..3, Style::Mark)]);
    }

    #[test]
    fn a_block_start_ends_what_html5_ends() {
        let mark = [(0..1, Style::Mark)];
        for source in [
            "<ul><li><mark>x<li>y</ul>z",
            "<div><p><mark>x<div>y</div>z</div>",
            "<dl><dt><mark>x<dd>y</dl>z",
        ] {
            let line = html(source).expect("text");
            assert_eq!(
                (line.text.as_str(), &line.styles[..]),
                ("x\ny\nz", &mark[..])
            );
        }
        let kbd = html("<div><p><kbd>x<p>y</div>z").expect("text");
        assert_eq!(kbd.styles, [(0..1, Style::Code)]);
        let nested = html("<ul><li><mark>a<ul><li>b</li></ul></li></ul>").expect("text");
        assert_eq!(
            nested.styles,
            [(0..3, Style::Mark)],
            "a list stops the search"
        );
    }

    #[test]
    fn filtered_tags_keep_shortcodes_and_names_end_as_html5_ends_them() {
        let block = html("<script title=\":smile:\">:smile:</script>").expect("text");
        assert_eq!(block.text, "<script title=\":smile:\">:smile:</script>");
        assert_eq!(html("<div>:smile:</div>").expect("text").text, "😄");
        let attribute = html("a <script title=\":smile:\">x</script> c").expect("text");
        assert_eq!(attribute.text, "a <script title=\":smile:\">x</script> c");
        let body = html("a <script>:smile:</script> :smile:").expect("text");
        assert_eq!(body.text, "a <script>:smile:</script> 😄");
        let named = "<div>a<script:custom>x</script:custom>b<script!>y</script!>c</div>";
        assert_eq!(html(named).expect("text").text, "axbyc");
    }

    #[test]
    fn hostile_html_reads_in_linear_time() {
        let started = web_time::Instant::now();
        for source in [
            "<".repeat(1 << 17),
            "<a".repeat(1 << 16),
            "<!--".repeat(1 << 16),
            "<b>".repeat(1 << 16),
            "<b>".repeat(1 << 15) + &"</i>".repeat(1 << 15),
            "<script></script>".repeat(1 << 13),
            "<div>a ".repeat(1 << 16),
            "<b> ".repeat(1 << 16),
            "<li><b>x ".repeat(1 << 15),
            "<p><i>x ".repeat(1 << 15),
        ] {
            let block = format!("<div>\n{source}");
            parse(&block);
        }
        let spent = started.elapsed();
        assert!(spent.as_secs() < 2, "took {spent:?}");
    }
}
