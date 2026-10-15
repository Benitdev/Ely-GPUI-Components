//! A wide character takes its laid-out width, not one column.

use gpui::{Bounds, EntityInputHandler, TestAppContext, VisualTestContext};

use super::editor;

#[gpui::test]
fn a_wide_character_moves_the_caret_by_its_width(cx: &mut TestAppContext) {
    // The test text system draws "😄" two ems wide, "a" one.
    let (editor, cx) = editor("😄a", cx);
    cx.run_until_parked();
    let x = |utf16: usize, cx: &mut VisualTestContext| {
        editor.update_in(cx, |editor, window, cx| {
            let bounds = editor.bounds_for_range(utf16..utf16, Bounds::default(), window, cx);
            bounds.expect("a drawn line").origin.x
        })
    };
    let (start, wide, narrow) = (x(0, cx), x(2, cx), x(3, cx));
    let off = (wide - start) - (narrow - wide) * 2.0;
    assert!(off.abs() < gpui::px(0.01), "{start:?} {wide:?} {narrow:?}");
    let at = editor.update_in(cx, |editor, window, cx| {
        let left = editor.metrics.left;
        editor.offset_in(0, left + (wide - start) * 0.9, window, cx)
    });
    assert_eq!(at, Some("😄".len()), "a press near its end lands after it");
}
