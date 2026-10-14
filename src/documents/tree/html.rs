use markdown::{decode_named, decode_numeric};

use super::Style;

/// One HTML tag as written: its lowercase name and attributes.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Tag {
    pub name: String,
    pub closing: bool,
    pub attributes: Vec<(String, String)>,
}

impl Tag {
    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

/// How Typora shows an inline tag; others show text.
pub(crate) fn inline_style(name: &str) -> Option<Style> {
    match name {
        "u" => Some(Style::Underline),
        "b" | "strong" => Some(Style::Strong),
        "i" | "em" => Some(Style::Emphasis),
        "s" | "del" | "strike" => Some(Style::Strike),
        "code" | "kbd" => Some(Style::Code),
        "mark" => Some(Style::Mark),
        "a" => Some(Style::Link),
        _ => None,
    }
}

/// `html` as one tag; comments and the rest are none.
pub(crate) fn tag(html: &str) -> Option<Tag> {
    let inner = html.trim().strip_prefix('<')?.strip_suffix('>')?;
    let (closing, inner) = match inner.strip_prefix('/') {
        Some(rest) => (true, rest),
        None => (false, inner),
    };
    if !inner.starts_with(|ch: char| ch.is_ascii_alphabetic()) {
        return None;
    }
    let named = inner
        .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-'))
        .unwrap_or(inner.len());
    Some(Tag {
        name: inner[..named].to_ascii_lowercase(),
        closing,
        attributes: attributes(&inner[named..]),
    })
}

/// `name="value"`, `name='value'`, `name=value` and bare names.
fn attributes(mut rest: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    loop {
        rest = rest.trim_start();
        // A slash between attributes closes the tag itself.
        if let Some(after) = rest.strip_prefix('/') {
            rest = after;
            continue;
        }
        let named = rest
            .find(|ch: char| ch.is_whitespace() || ch == '=' || ch == '/')
            .unwrap_or(rest.len());
        if named == 0 {
            return out;
        }
        let name = rest[..named].to_ascii_lowercase();
        rest = rest[named..].trim_start();
        let Some(after) = rest.strip_prefix('=') else {
            out.push((name, String::new()));
            continue;
        };
        let after = after.trim_start();
        let (value, left) = match after.chars().next() {
            Some(quote @ ('"' | '\'')) => match after[1..].find(quote) {
                Some(end) => (&after[1..1 + end], &after[2 + end..]),
                None => (&after[1..], ""),
            },
            _ => {
                let end = after.find(char::is_whitespace).unwrap_or(after.len());
                (&after[..end], &after[end..])
            }
        };
        out.push((name, decoded(value)));
        rest = left;
    }
}

/// `text` with its character references decoded.
fn decoded(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        match reference(rest) {
            Some((value, len)) => {
                out.push_str(&value);
                rest = &rest[len..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The reference `text` starts with, as CommonMark reads one.
fn reference(text: &str) -> Option<(String, usize)> {
    let body = text.strip_prefix('&')?;
    let end = body.find(';')?;
    let name = &body[..end];
    let value = match name.strip_prefix('#') {
        Some(number) => {
            let (digits, radix, most) = match number.strip_prefix(['x', 'X']) {
                Some(hex) => (hex, 16, 6),
                None => (number, 10, 7),
            };
            let valid =
                (1..=most).contains(&digits.len()) && digits.chars().all(|ch| ch.is_digit(radix));
            valid.then(|| decode_numeric(digits, radix))?
        }
        None if !name.is_empty() && name.chars().all(|ch| ch.is_ascii_alphanumeric()) => {
            decode_named(name, true)?
        }
        None => return None,
    };
    Some((value, end + 2))
}

/// `value` with each `:shortcode:` written plainly in `source` as emoji.
pub(crate) fn emoji(source: &str, value: &str) -> String {
    let mut codes = literal_codes(source).into_iter().peekable();
    let mut out = String::with_capacity(value.len());
    let (mut colons, mut rest) = (0, value);
    while let Some(at) = rest.find(':') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        match codes.peek() {
            Some((ordinal, code, glyph))
                if *ordinal == colons
                    && rest[1..].starts_with(code)
                    && rest[1 + code.len()..].starts_with(':') =>
            {
                out.push_str(glyph);
                rest = &rest[code.len() + 2..];
                colons += 2;
                codes.next();
            }
            _ => {
                out.push(':');
                rest = &rest[1..];
                colons += 1;
                while codes.next_if(|(ordinal, _, _)| *ordinal < colons).is_some() {}
            }
        }
    }
    out.push_str(rest);
    out
}

/// Plain shortcodes in `source`, with colons counted before each.
fn literal_codes(source: &str) -> Vec<(usize, &str, &'static str)> {
    let (mut out, mut colons, mut at) = (Vec::new(), 0, 0);
    while let Some(ch) = source[at..].chars().next() {
        let (rest, after) = (&source[at..], &source[at + ch.len_utf8()..]);
        if ch == '\\'
            && let Some(next) = after.chars().next().filter(char::is_ascii_punctuation)
        {
            colons += usize::from(next == ':');
            at += 1 + next.len_utf8();
            continue;
        }
        if ch == '&'
            && let Some((text, len)) = reference(rest)
        {
            colons += text.matches(':').count();
            at += len;
            continue;
        }
        if ch == ':' {
            let found = after
                .find(':')
                .map(|end| &after[..end])
                .filter(|code| !code.is_empty() && code.chars().all(shortcode_char))
                .and_then(|code| emojis::get_by_shortcode(code).map(|found| (code, found)));
            if let Some((code, found)) = found {
                out.push((colons, code, found.as_str()));
                colons += 2;
                at += code.len() + 2;
                continue;
            }
            colons += 1;
        }
        at += ch.len_utf8();
    }
    out
}

fn shortcode_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '+' | '-')
}

/// `n` in superscript digits.
pub(crate) fn superscript(n: usize) -> String {
    const DIGITS: [char; 10] = ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];
    n.to_string()
        .bytes()
        .map(|digit| DIGITS[usize::from(digit - b'0')])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tag_reads_its_name_and_attributes() {
        let link = tag("<A HREF=\"https://ely.dev\" title='x' hidden>").expect("a tag");
        assert_eq!((link.name.as_str(), link.closing), ("a", false));
        assert_eq!(link.attribute("href"), Some("https://ely.dev"));
        assert_eq!(link.attribute("title"), Some("x"));
        assert_eq!(link.attribute("hidden"), Some(""));
        assert_eq!(
            tag("</u>").map(|t| (t.name, t.closing)),
            Some(("u".into(), true))
        );
        assert_eq!(tag("<br/>").map(|t| t.name), Some("br".into()));
        assert_eq!(tag("<u-box>").map(|t| t.name), Some("u-box".into()));
        assert_eq!(tag("<!-- note -->"), None);
        assert_eq!(tag("<1>"), None);
    }

    #[test]
    fn attribute_values_keep_slashes_and_decode_references() {
        let bare = tag("<a href=/path/>").expect("a tag");
        assert_eq!(bare.attribute("href"), Some("/path/"));
        let closed = tag("<img src=\"a.png\" />").expect("a tag");
        assert_eq!(closed.attribute("src"), Some("a.png"));
        let query = tag("<a href=\"https://x.test/?q=1&amp;p=2&#38;r=&#x33;&nope;\">");
        let query = query.expect("a tag");
        assert_eq!(
            query.attribute("href"),
            Some("https://x.test/?q=1&p=2&r=3&nope;")
        );
    }

    #[test]
    fn shortcodes_written_plainly_become_emoji() {
        let same = |text: &str| emoji(text, text);
        assert_eq!(same("hi :smile: :+1:"), "hi 😄 👍");
        assert_eq!(same(":nope: at 10:30 and ::"), ":nope: at 10:30 and ::");
        assert_eq!(same("a:smile:b"), "a😄b");
        assert_eq!(same("é :smile: 中\\é"), "é 😄 中\\é");
    }

    #[test]
    fn escaped_or_referenced_colons_stay() {
        assert_eq!(emoji("\\:smile: :smile:", ":smile: :smile:"), ":smile: 😄");
        assert_eq!(emoji("&#58;smile: :tada:", ":smile: :tada:"), ":smile: 🎉");
        assert_eq!(emoji(":sm&#105;le:", ":smile:"), ":smile:");
    }

    #[test]
    fn numbers_go_up() {
        assert_eq!(superscript(1), "¹");
        assert_eq!(superscript(120), "¹²⁰");
    }
}
