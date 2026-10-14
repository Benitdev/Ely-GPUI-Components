use gpui::{Modifiers, MouseButton, TestAppContext, VisualTestContext, point, px};

use super::{MarkdownMode, open};

const LISTED: &str = "[toc]\n\n# A\n\ntext\n\n## B\n";

/// Presses entry `ix` of the drawn heading list.
fn press_entry(ix: usize, modifiers: Modifiers, cx: &mut VisualTestContext) {
    let bounds = cx
        .debug_bounds(Box::leak(format!("markdown-toc-{ix}").into_boxed_str()))
        .expect("the entry is drawn");
    let at = point(bounds.left() + px(2.0), bounds.top() + px(2.0));
    cx.simulate_mouse_down(at, MouseButton::Left, modifiers);
    cx.simulate_mouse_up(at, MouseButton::Left, modifiers);
    cx.run_until_parked();
}

#[gpui::test]
fn a_press_in_preview_reveals_the_heading(cx: &mut TestAppContext) {
    let (document, cx) = open(LISTED, MarkdownMode::Preview, cx);
    press_entry(1, Modifiers::none(), cx);
    let caret = document.read_with(cx, |document, cx| document.caret(cx));
    assert_eq!(caret, (6, 0));
}

#[gpui::test]
fn visual_reveals_on_a_secondary_press_and_edits_on_a_plain_one(cx: &mut TestAppContext) {
    let (document, cx) = open(LISTED, MarkdownMode::Visual, cx);
    press_entry(1, Modifiers::secondary_key(), cx);
    let (caret, open) = document.read_with(cx, |document, cx| {
        (document.caret(cx), document.open.is_some())
    });
    assert_eq!((caret, open), ((6, 0), false));
    press_entry(0, Modifiers::none(), cx);
    assert!(document.read_with(cx, |document, _| document.open.is_some()));
}
