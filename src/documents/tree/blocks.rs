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
        let name = line.trim().strip_prefix("[!")?.strip_suffix(']')?;
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
        let [first @ mdast::Node::Paragraph(paragraph), rest @ ..] = &quote.children[..] else {
            return None;
        };
        let line = self.source[span(first)].lines().next().unwrap_or_default();
        let kind = AlertKind::marked(line)?;
        let after = after_first_line(&paragraph.children);
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

    /// An HTML block's text; comments, scripts and styles drop.
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
                Some(tag) if !tag.closing && matches!(tag.name.as_str(), "script" | "style") => {
                    let closing = format!("</{}", tag.name);
                    let skipped = rest.to_ascii_lowercase().find(&closing);
                    let left = skipped.map_or("", |at| &rest[at..]);
                    rest = html::tag_end(left).map_or("", |end| &left[end..]);
                }
                Some(tag) if html::breaks(&tag.name) => run.line(),
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
        let shown = match self.in_code() {
            true => decoded,
            false => html::emoji(&decoded),
        };
        for ch in shown.chars() {
            if !html::spacing(ch) {
                self.pay();
                self.text.text.push(ch);
            } else if self.owed.is_none() && !self.text.text.ends_with('\n') {
                self.owed = (!self.text.text.is_empty()).then_some(' ');
            }
        }
    }

    /// A line owed where a block element starts or ends.
    fn line(&mut self) {
        let text = &self.text.text;
        self.owed = (!(text.is_empty() || text.ends_with('\n'))).then_some('\n');
    }

    /// Writes the white space owed, before a word or tag.
    fn pay(&mut self) {
        if let Some(owed) = self.owed.take() {
            self.text.text.push(owed);
        }
    }
}

/// Inlines past the first line ending.
fn after_first_line(children: &[mdast::Node]) -> Vec<mdast::Node> {
    let mut found = None;
    for (at, child) in children.iter().enumerate() {
        match child {
            mdast::Node::Text(text) => {
                if let Some((_, rest)) = text.value.split_once('\n') {
                    let mut kept = text.clone();
                    kept.value = rest.to_string();
                    found = Some((at + 1, (!rest.is_empty()).then_some(kept)));
                    break;
                }
            }
            mdast::Node::Break(_) => {
                found = Some((at + 1, None));
                break;
            }
            _ => {}
        }
    }
    let Some((next, head)) = found else {
        return Vec::new();
    };
    let head = head.map(mdast::Node::Text);
    head.into_iter()
        .chain(children[next..].iter().cloned())
        .collect()
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
        let hidden = "<div>\n<script>alert(':smile:')</script>\n<!-- note -->\nshown<style>p{}</style>\n</div>";
        assert_eq!(html(hidden).expect("text").text, "shown");
        assert_eq!(html("<style>\np { color: red }\n</style>"), None);
        assert_eq!(html("<div>\na < b <i").expect("text").text, "a < b");
    }

    #[test]
    fn hostile_html_reads_in_linear_time() {
        let started = web_time::Instant::now();
        for source in [
            "<".repeat(1 << 17),
            "<a".repeat(1 << 16),
            "<!--".repeat(1 << 16),
        ] {
            let block = format!("<div>\n{source}");
            parse(&block);
        }
        let spent = started.elapsed();
        assert!(spent.as_secs() < 2, "took {spent:?}");
    }
}
