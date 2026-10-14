//! Components Ely ships say nothing in fixed English.

/// Sources of components the Ely apps draw.
const SHIPPED: &[(&str, &str)] = &[
    ("editor/find.rs", include_str!("../../editor/find.rs")),
    ("editor/hints.rs", include_str!("../../editor/hints.rs")),
    ("forms/input.rs", include_str!("../../forms/input.rs")),
    ("forms/select.rs", include_str!("../../forms/select.rs")),
    (
        "tables/builders.rs",
        include_str!("../../tables/builders.rs"),
    ),
    (
        "navigation/editor_tabs.rs",
        include_str!("../../navigation/editor_tabs.rs"),
    ),
    (
        "settings/appearance.rs",
        include_str!("../../settings/appearance.rs"),
    ),
];

#[test]
fn shipped_components_show_no_fixed_english() {
    let shown = [".tooltip(\"", ".child(\"", "SharedString::from(\""];
    let mut found = Vec::new();
    for (path, source) in SHIPPED {
        let code = source.split("#[cfg(test)]").next().unwrap_or(source);
        for (no, line) in code.lines().enumerate() {
            for call in shown {
                let Some(at) = line.find(call) else { continue };
                let rest = &line[at + call.len()..];
                if rest.starts_with(|ch: char| ch.is_ascii_alphabetic()) {
                    found.push(format!("{path}:{}: {}", no + 1, line.trim()));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "English outside the catalog:\n{}",
        found.join("\n")
    );
}
