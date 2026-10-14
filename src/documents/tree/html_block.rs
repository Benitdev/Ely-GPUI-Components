use std::ops::Range;

use super::{Inline, Run, html, sources::Follow};

/// An HTML block's text, as a browser shows it.
pub(super) fn html_block_text(written: &str, source: &str, span: Range<usize>) -> Inline {
    let (mut run, mut follow) = (Run::default(), Follow::new(written, source, span.clone()));
    let mut rest = written;
    // Each text run keeps where it was written.
    let mut words = |run: &mut Run, rest: &str, text: &str| {
        if let Some(at) = follow.to(written.len() - rest.len()) {
            run.text.sources.push((run.text.text.len(), at..span.end));
        }
        run.words(text);
    };
    while let Some(at) = rest.find('<') {
        words(&mut run, rest, &rest[..at]);
        rest = &rest[at..];
        if let Some(comment) = rest.strip_prefix("<!--") {
            rest = comment.split_once("-->").map_or("", |(_, after)| after);
            run.shortcodes.split();
            continue;
        }
        let opens =
            rest[1..].starts_with(|ch: char| ch.is_ascii_alphabetic() || "/!?".contains(ch));
        if !opens {
            words(&mut run, rest, "<");
            rest = &rest[1..];
            continue;
        }
        // An unclosed tag drops the rest, as HTML5 does.
        let Some(end) = html::tag_end(rest) else {
            rest = "";
            break;
        };
        let (tag_text, after) = rest.split_at(end);
        match html::tag(tag_text) {
            Some(tag) if html::filtered(&tag.name) => words(&mut run, rest, tag_text),
            // A block closes what opened inside it.
            Some(tag) if html::breaks(&tag.name) => {
                run.line();
                match tag.closing {
                    true => run.end(&tag.name),
                    false => {
                        run.implied(&tag.name);
                        match tag.name == "hr" {
                            true => run.shortcodes.split(),
                            false => run.start(&tag.name, None, None),
                        }
                    }
                }
            }
            // It ends a `p` but lays no block.
            Some(tag) if !tag.closing && html::closes_p(&tag.name) => {
                run.implied(&tag.name);
                run.html(tag_text);
            }
            // A line's end drops its space.
            Some(tag) if tag.closing || tag.name == "br" => {
                run.owed = run.owed.filter(|_| tag.name != "br");
                run.html(tag_text);
            }
            Some(_) => {
                run.pay();
                run.html(tag_text);
            }
            None => run.shortcodes.split(),
        }
        rest = after;
    }
    words(&mut run, rest, rest);
    run.cut()
}

impl Run {
    /// HTML text: references decoded, white space collapsed.
    fn words(&mut self, written: &str) {
        let shown = self.shown(&html::text(written));
        self.spaced(&shown);
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
        if html::closes_p(name) && self.count("p") > 0 {
            self.line();
            self.end("p");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::documents::tree::{Inline, Node, Style, parse};

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
        let lines: Vec<_> = (split.into_iter())
            .map(|node| match node {
                Node::Paragraph(line) => line.text,
                other => panic!("a paragraph, not {other:?}"),
            })
            .collect();
        assert_eq!(lines, ["<script> first", "second </script>"]);
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
    fn filtered_tags_are_text_and_names_end_as_html5_ends_them() {
        // GitHub's answers, `.work/R4.3-fix3/github-*.log`.
        for (source, shown) in [
            (
                "<div><script>:smile:</script> :smile:</div>",
                "<script>:smile:</script> :smile:",
            ),
            (
                "<div><script><b>:smile:</b></script> :smile:</div>",
                "<script>😄</script> 😄",
            ),
            (
                "<div><script> :smile:</script> :smile:</div>",
                "<script> 😄</script> 😄",
            ),
            (
                "<div><script title=\" :smile:\"> x:smile:</div>",
                "<script title=\" 😄\"> x😄",
            ),
            (
                "<div><iframe title=\":smile:\"> :smile:</div>",
                "<iframe title=\":smile:\"> :smile:",
            ),
            ("<div>\n:smile: x:smile:\n</div>", "😄 x😄"),
            ("<div>\t:smile: x:smile:</div>", "😄 x😄"),
            ("<div>a\n :smile: x:smile:</div>", "a 😄 x😄"),
            ("<div>y\n:smile: x:smile:</div>", "y :smile: x:smile:"),
            ("<div>:smile:</div>", "😄"),
        ] {
            assert_eq!(html(source).expect("text").text, shown, "{source:?}");
        }
        let named = "<div>a<script:custom>x</script:custom>b<script!>y</script!>c</div>";
        assert_eq!(html(named).expect("text").text, "axbyc");
    }

    #[test]
    fn every_html5_p_closer_ends_a_p() {
        for name in ["fieldset", "menu"] {
            let line = html(&format!("<div><p><mark>x<{name}>y</{name}>z</div>")).expect("text");
            let mark = [(0..1, Style::Mark)];
            assert_eq!((line.text.as_str(), &line.styles[..]), ("x\nyz", &mark[..]));
        }
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
