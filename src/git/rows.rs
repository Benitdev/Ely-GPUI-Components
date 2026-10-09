//! The rows a diff viewer draws, and their colors.

use std::ops::Range;

use gpui::{App, HighlightStyle, Hsla, SharedString, StyledText};

use super::{
    diff::{DiffLine, LineKind, Stretch, pair_places},
    viewer::DiffLayout,
};
use crate::{
    editor::{code_colors, stack},
    theme::Palette,
};

/// A line's place: its stretch, then its line there.
pub type Place = (usize, usize);

/// A row as the viewer draws it.
#[derive(Clone)]
pub(super) enum Shown {
    /// A hunk's header and its place among the stretches.
    Header(usize, SharedString),
    Line(Place, DiffLine),
    Pair(Option<(Place, DiffLine)>, Option<(Place, DiffLine)>),
    /// A folded stretch: its place and how many lines.
    Fold(usize, usize),
}

pub(super) fn shown(stretches: &[Stretch], layout: DiffLayout, open: &[usize]) -> Vec<Shown> {
    let mut rows = Vec::new();
    for (ix, stretch) in stretches.iter().enumerate() {
        match stretch {
            Stretch::Hunk { header, lines } => {
                rows.push(Shown::Header(ix, header.clone()));
                let at = |line: usize| ((ix, line), lines[line].clone());
                match layout {
                    DiffLayout::Unified => rows.extend((0..lines.len()).map(|line| {
                        let (place, line) = at(line);
                        Shown::Line(place, line)
                    })),
                    DiffLayout::Split => rows.extend(
                        pair_places(lines)
                            .into_iter()
                            .map(|(left, right)| Shown::Pair(left.map(at), right.map(at))),
                    ),
                }
            }
            Stretch::Folded(lines) if open.contains(&ix) => {
                rows.extend(lines.iter().enumerate().map(|(at, line)| {
                    let place = (ix, at);
                    match layout {
                        DiffLayout::Unified => Shown::Line(place, line.clone()),
                        DiffLayout::Split => {
                            Shown::Pair(Some((place, line.clone())), Some((place, line.clone())))
                        }
                    }
                }))
            }
            Stretch::Folded(lines) => rows.push(Shown::Fold(ix, lines.len())),
        }
    }
    rows
}

/// The wash behind a line, and behind its changed words.
pub(super) fn washes(kind: LineKind, colors: &Palette) -> (Option<Hsla>, Hsla) {
    match kind {
        LineKind::Same => (None, colors.hover),
        LineKind::Added => (
            Some(colors.success.opacity(0.08)),
            colors.success.opacity(0.25),
        ),
        LineKind::Removed => (
            Some(colors.danger.opacity(0.08)),
            colors.danger.opacity(0.25),
        ),
    }
}

/// Extensions of prose and plain text.
const PROSE: [&str; 13] = [
    "md", "markdown", "mdown", "mkd", "txt", "text", "rst", "adoc", "asciidoc", "org", "csv",
    "tsv", "log",
];

/// Whether `path` holds prose, read plain rather than as code.
pub(super) fn prose(path: &str) -> bool {
    let name = path.rsplit_once('/').map_or(path, |(_, name)| name);
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => PROSE.contains(&ext.to_ascii_lowercase().as_str()),
        _ => true,
    }
}

/// A line's text, colored as code unless prose, changed words washed.
pub(super) fn code(line: &DiffLine, colors: &Palette, prose: bool, cx: &App) -> StyledText {
    let (_, word) = washes(line.kind, colors);
    let mut styles: Vec<(Range<usize>, HighlightStyle)> = if prose {
        Vec::new()
    } else {
        code_colors(&line.text, cx)
    };
    styles.extend(line.words.iter().map(|range| {
        (
            range.clone(),
            HighlightStyle {
                background_color: Some(word),
                ..HighlightStyle::default()
            },
        )
    }));
    styles.sort_by_key(|(range, _)| range.start);
    StyledText::new(line.text.clone()).with_highlights(stack(styles))
}

/// A line as assistive tech reads it: number, change, text.
pub(super) fn spoken(line: &DiffLine) -> SharedString {
    let (number, sign) = match line.kind {
        LineKind::Same => (line.new, ""),
        LineKind::Added => (line.new, "+ "),
        LineKind::Removed => (line.old, "− "),
    };
    let number = number.map_or_else(String::new, |n| format!("{n} "));
    format!("{number}{sign}{}", line.text).into()
}

#[cfg(test)]
mod tests {
    #[test]
    fn prose_reads_plain_and_code_reads_as_code() {
        for path in [
            "docs/README.md",
            "notes.MARKDOWN",
            "LICENSE",
            ".gitignore",
            "a/b.txt",
            "data.csv",
        ] {
            assert!(super::prose(path), "{path}");
        }
        for path in ["src/main.rs", "web/app.js", "a.md/b.py", "Cargo.toml"] {
            assert!(!super::prose(path), "{path}");
        }
    }

    #[test]
    fn a_line_speaks_its_number_change_and_text() {
        let stretches = super::super::diff::diff("a\nb\n", "a\nc\n", 1);
        let super::Stretch::Hunk { lines, .. } = &stretches[0] else {
            panic!("a hunk")
        };
        let said: Vec<String> = lines
            .iter()
            .map(|line| super::spoken(line).to_string())
            .collect();
        assert_eq!(said, ["1 a", "2 − b", "2 + c"]);
    }
}
