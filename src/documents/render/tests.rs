use gpui::{Context, IntoElement, Render, TestAppContext, Window, div, prelude::*, px};

use super::MarkdownRenderer;
use crate::theme::Theme;

/// `source` drawn in a 400-point column.
struct Page(&'static str);

impl Render for Page {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(400.0))
            .child(MarkdownRenderer::new("page", self.0))
    }
}

#[gpui::test]
fn a_rule_draws_a_line_two_pixels_high(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let (_, cx) = cx.add_window_view(|_, _| Page("above\n\n---\n\nbelow\n"));
    cx.run_until_parked();
    let rule = cx.debug_bounds("markdown-rule").expect("the rule is drawn");
    assert_eq!(rule.size.height, px(2.0), "{rule:?}");
    assert!(rule.size.width > px(300.0), "{rule:?}");
}

#[gpui::test]
fn bullets_turn_with_depth(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let (_, cx) = cx.add_window_view(|_, _| Page("- a\n  - b\n    1. c\n       - d\n"));
    cx.run_until_parked();
    for depth in [0, 1, 3] {
        let selector = Box::leak(format!("markdown-bullet-{depth}").into_boxed_str());
        assert!(cx.debug_bounds(selector).is_some(), "depth {depth}");
    }
    assert!(
        cx.debug_bounds("markdown-bullet-2").is_none(),
        "an ordered list"
    );
}

#[gpui::test]
fn alerts_lists_of_headings_and_front_matter_draw(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let source = "> [!IMPORTANT]\n> read\n\n[toc]\n\n# One\n\n## Two\n";
    let (_, cx) = cx.add_window_view(|_, _| Page(source));
    cx.run_until_parked();
    let (one, two) = (
        cx.debug_bounds("markdown-toc-0").expect("drawn"),
        cx.debug_bounds("markdown-toc-1").expect("drawn"),
    );
    assert!(two.top() > one.top(), "{one:?} {two:?}");
}

#[gpui::test]
fn each_heading_level_stands_above_the_next(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let source = "# a

## a

### a

#### a

##### a
";
    let (_, cx) = cx.add_window_view(|_, _| Page(source));
    cx.run_until_parked();
    let high = |ordinal: usize| {
        let selector = Box::leak(format!("markdown-heading-{ordinal}").into_boxed_str());
        cx.debug_bounds(selector).expect("drawn").size.height
    };
    let heights: Vec<_> = (0..5).map(high).collect();
    // Level 5 is the body's size; each above it is taller.
    assert!(
        heights.windows(2).all(|pair| pair[0] > pair[1]),
        "{heights:?}"
    );
}
