//! An open block keeps its place and size (R7.5).

use gpui::{Bounds, Modifiers, Pixels, TestAppContext, VisualTestContext, rems};

use super::{open, press};
use crate::documents::MarkdownMode;

/// Block 0's bounds closed, then open.
fn closed_then_open<'a>(
    source: &str,
    cx: &'a mut TestAppContext,
) -> (Bounds<Pixels>, Bounds<Pixels>, &'a mut VisualTestContext) {
    let (_, cx) = open(source, MarkdownMode::Visual, cx);
    let closed = cx.debug_bounds("markdown-block-0").expect("drawn");
    press(0, Modifiers::none(), cx);
    let open = cx.debug_bounds("markdown-open").expect("the block opens");
    (closed, open, cx)
}

/// Within a pixel: a field rounds its line height.
fn near(a: Pixels, b: Pixels) -> bool {
    (a - b).abs() < gpui::px(1.0)
}

#[gpui::test]
fn an_open_paragraph_keeps_its_place_and_height(cx: &mut TestAppContext) {
    let (closed, open, _) = closed_then_open("Plain words here.\n", cx);
    assert!(
        near(closed.top(), open.top()) && near(closed.left(), open.left()),
        "{closed:?} {open:?}"
    );
    assert!(
        near(closed.size.height, open.size.height),
        "{closed:?} {open:?}"
    );
}

#[gpui::test]
fn an_open_heading_keeps_its_size(cx: &mut TestAppContext) {
    let (closed, open, _) = closed_then_open("## Plain heading\n", cx);
    assert!(
        near(closed.top(), open.top()) && near(closed.left(), open.left()),
        "{closed:?} {open:?}"
    );
    assert!(
        near(closed.size.height, open.size.height),
        "{closed:?} {open:?}"
    );
}

#[gpui::test]
fn an_open_code_block_keeps_its_panel(cx: &mut TestAppContext) {
    let (closed, open, cx) = closed_then_open("```\nlet x = 1;\n```\n", cx);
    assert!(
        near(closed.top(), open.top()) && near(closed.left(), open.left()),
        "{closed:?} {open:?}"
    );
    // The panel sits inside the frame's border and padding.
    let (x, y) = cx.update(|window, _| {
        let rem = window.rem_size();
        (rems(0.75).to_pixels(rem), rems(0.25).to_pixels(rem))
    });
    let panel = cx.debug_bounds("markdown-open-text").expect("drawn");
    let border = gpui::px(1.0);
    assert!(
        near(panel.left(), closed.left() + border + x),
        "{panel:?} {closed:?}"
    );
    assert!(
        near(panel.top(), closed.top() + border + y),
        "{panel:?} {closed:?}"
    );
}
