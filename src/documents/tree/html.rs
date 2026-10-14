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

/// How Typora shows an inline tag; others show their text.
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

/// `html` read as one tag; comments and the rest are none.
pub(crate) fn tag(html: &str) -> Option<Tag> {
    let inner = html.trim().strip_prefix('<')?.strip_suffix('>')?;
    let (closing, inner) = match inner.strip_prefix('/') {
        Some(rest) => (true, rest),
        None => (false, inner),
    };
    let named = inner
        .find(|ch: char| !ch.is_ascii_alphanumeric())
        .unwrap_or(inner.len());
    if named == 0 || !inner.starts_with(|ch: char| ch.is_ascii_alphabetic()) {
        return None;
    }
    Some(Tag {
        name: inner[..named].to_ascii_lowercase(),
        closing,
        attributes: attributes(inner[named..].trim_end_matches('/')),
    })
}

/// `name="value"`, `name='value'`, `name=value` and bare names.
fn attributes(mut rest: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    loop {
        rest = rest.trim_start();
        let named = rest
            .find(|ch: char| ch.is_whitespace() || ch == '=')
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
        out.push((name, value.to_string()));
        rest = left;
    }
}

/// `:shortcode:` written as its emoji, as Typora shows it.
pub(crate) fn emoji(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find(':') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let code = after
            .find(':')
            .map(|close| &after[..close])
            .filter(|code| !code.is_empty() && code.chars().all(shortcode_char));
        match code.and_then(emojis::get_by_shortcode) {
            Some(found) => {
                out.push_str(found.as_str());
                rest = &after[code.expect("a matched code").len() + 1..];
            }
            None => {
                out.push(':');
                rest = after;
            }
        }
    }
    out.push_str(rest);
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
        assert_eq!(tag("<!-- note -->"), None);
        assert_eq!(tag("<1>"), None);
    }

    #[test]
    fn shortcodes_become_emoji_and_others_stay() {
        assert_eq!(emoji("hi :smile: :+1:"), "hi 😄 👍");
        assert_eq!(emoji(":nope: at 10:30 and ::"), ":nope: at 10:30 and ::");
        assert_eq!(emoji("a:smile:b"), "a😄b");
    }

    #[test]
    fn numbers_go_up() {
        assert_eq!(superscript(1), "¹");
        assert_eq!(superscript(120), "¹²⁰");
    }
}
