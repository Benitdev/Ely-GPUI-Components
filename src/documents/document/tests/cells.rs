//! A press in a table opens only its cell (R6.1).

use gpui::{Modifiers, MouseButton, TestAppContext, VisualTestContext, point, px};

use super::{open, opened, text};
use crate::documents::MarkdownMode;

const TABLE: &str = "| a | **bold** |\n| --- | --- |\n| c |  |\n";

/// Presses cell `row`, `column` of the first table near its left.
fn press_cell(row: usize, column: usize, cx: &mut VisualTestContext) {
    let name = format!("markdown-cell-0-{row}-{column}");
    let bounds = cx
        .debug_bounds(Box::leak(name.into_boxed_str()))
        .expect("the cell is drawn");
    let at = point(bounds.left() + px(2.0), bounds.top() + px(2.0));
    cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
}

#[gpui::test]
fn a_press_opens_its_cell_and_writes_one_undo_step(cx: &mut TestAppContext) {
    let (document, cx) = open(TABLE, MarkdownMode::Visual, cx);
    press_cell(0, 1, cx);
    assert_eq!(opened(&document, cx), "**bold**");
    assert!(
        cx.debug_bounds("markdown-cell-0-0-0").is_some(),
        "the rest stays drawn"
    );
    press_cell(1, 1, cx);
    assert_eq!(opened(&document, cx), "");
    cx.simulate_input("x|y");
    assert_eq!(
        text(&document, cx),
        "| a | **bold** |\n| --- | --- |\n| c | x\\|y |\n"
    );
    cx.simulate_keystrokes("escape");
    let editor = document.read_with(cx, |document, _| document.editor().clone());
    editor.update(cx, |editor, cx| editor.undo(cx));
    assert_eq!(text(&document, cx), TABLE);
}

#[gpui::test]
fn tab_and_arrows_walk_the_cells(cx: &mut TestAppContext) {
    let (document, cx) = open(TABLE, MarkdownMode::Visual, cx);
    press_cell(0, 0, cx);
    assert_eq!(opened(&document, cx), "a");
    cx.simulate_keystrokes("tab");
    assert_eq!(opened(&document, cx), "**bold**");
    cx.simulate_keystrokes("shift-tab");
    assert_eq!(opened(&document, cx), "a");
    cx.simulate_keystrokes("down");
    assert_eq!(opened(&document, cx), "c");
    cx.simulate_keystrokes("enter");
    assert!(document.read_with(cx, |document, _| document.open.is_none()));
    assert_eq!(text(&document, cx), TABLE);
}
