//! The rows a diff viewer draws, and their colors.

use std::ops::Range;

use gpui::{App, HighlightStyle, Hsla, SharedString, StyledText};

use super::{
    diff::{DiffLine, LineKind, Stretch, pair_places},
    viewer::DiffLayout,
};
use crate::{
    editor::{code_colors_with, stack},
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

/// Line comment marks by extension; others take `//`.
const COMMENTS: [(&str, &[&str]); 4] = [
    (
        "#",
        &[
            "py",
            "rb",
            "sh",
            "bash",
            "zsh",
            "fish",
            "toml",
            "yaml",
            "yml",
            "pl",
            "r",
            "conf",
            "nix",
            "ex",
            "exs",
            "cmake",
            "mk",
            "ps1",
            "tcl",
            "jl",
            "cr",
            "nim",
            "gitignore",
        ],
    ),
    ("--", &["sql", "lua", "hs", "elm", "ada"]),
    (
        ";",
        &["ini", "clj", "cljs", "lisp", "el", "scm", "asm", "s"],
    ),
    ("%", &["tex", "sty", "erl", "m"]),
];

/// How `path` reads: none for prose, else its comment mark.
pub(super) fn syntax(path: &str) -> Option<&'static str> {
    let name = path.rsplit_once('/').map_or(path, |(_, name)| name);
    let ext = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => ext.to_ascii_lowercase(),
        Some(("", dotfile)) => dotfile.to_ascii_lowercase(),
        _ if matches!(name, "Makefile" | "Dockerfile" | "CMakeLists.txt") => return Some("#"),
        _ => return None,
    };
    if PROSE.contains(&ext.as_str()) {
        return None;
    }
    let found = COMMENTS
        .iter()
        .find(|(_, exts)| exts.contains(&ext.as_str()));
    Some(found.map_or("//", |(mark, _)| *mark))
}

/// A line's syntax colors as the file at `path` reads.
pub(super) fn colors_of(text: &str, path: &str, cx: &App) -> Vec<(Range<usize>, HighlightStyle)> {
    syntax(path).map_or_else(Vec::new, |comment| code_colors_with(text, comment, cx))
}

/// A line colored as its file reads, changed words washed.
pub(super) fn code(line: &DiffLine, colors: &Palette, path: &str, cx: &App) -> StyledText {
    let (_, word) = washes(line.kind, colors);
    let mut styles = colors_of(&line.text, path, cx);
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
    #[gpui::test]
    fn each_file_colors_as_its_kind_reads(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            crate::init_for_tests(cx);
            let syntax = crate::theme::ActiveTheme::theme(cx).colors.syntax.clone();
            let color = |text: &str, path: &str, cx: &gpui::App| {
                super::colors_of(text, path, cx)
                    .into_iter()
                    .map(|(range, style)| (range, style.color))
                    .collect::<Vec<_>>()
            };
            for path in [
                "docs/README.md",
                "notes.MARKDOWN",
                "LICENSE",
                "a/b.txt",
                "data.csv",
            ] {
                assert!(
                    color("# return 42", path, cx).is_empty(),
                    "{path} reads plain"
                );
            }
            for path in [
                "script.py",
                "config.toml",
                "run.sh",
                ".gitignore",
                "Makefile",
            ] {
                assert_eq!(
                    color("# return 42", path, cx),
                    [(0..11, Some(syntax.comment))],
                    "{path}"
                );
            }
            assert_eq!(
                color("-- return 42", "q.sql", cx),
                [(0..12, Some(syntax.comment))]
            );
            let rust = color("return 42 // done", "src/main.rs", cx);
            assert_eq!(rust[0], (0..6, Some(syntax.keyword)), "{rust:?}");
            assert_eq!(rust.last(), Some(&(10..17, Some(syntax.comment))));
        });
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
