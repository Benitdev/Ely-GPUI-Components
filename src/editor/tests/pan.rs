//! Code wider than the view pans sideways under a still gutter.

use gpui::{
    Bounds, EntityInputHandler, Modifiers, Pixels, ScrollDelta, ScrollWheelEvent, TestAppContext,
    TouchPhase, VisualTestContext, point, px,
};

use super::editor;

fn long() -> String {
    format!("{}\nshort\n", "x".repeat(600))
}

fn wheel(dx: f32, dy: f32, editor: &gpui::Entity<super::CodeEditor>, cx: &mut VisualTestContext) {
    let position = editor.update(cx, |editor, _| {
        // The code's box, wherever the text sits in it.
        point(
            editor.metrics.left + editor.pan.x + px(40.0),
            editor.metrics.top + editor.metrics.line / 2.0,
        )
    });
    cx.simulate_event(ScrollWheelEvent {
        position,
        delta: ScrollDelta::Pixels(point(px(dx), px(dy))),
        modifiers: Modifiers::default(),
        touch_phase: TouchPhase::Moved,
    });
    cx.run_until_parked();
}

fn pan(editor: &gpui::Entity<super::CodeEditor>, cx: &mut VisualTestContext) -> Pixels {
    editor.update(cx, |editor, _| editor.pan.x)
}

#[gpui::test]
fn a_sideways_wheel_pans_and_a_press_lands_under_the_pointer(cx: &mut TestAppContext) {
    let (editor, cx) = editor(&long(), cx);
    cx.run_until_parked();
    wheel(-120.0, 0.0, &editor, cx);
    let panned = pan(&editor, cx);
    assert_eq!(panned, px(120.0), "the code moved with the fingers");
    let (at, advance) = editor.update_in(cx, |editor, window, cx| {
        let left = editor.metrics.left;
        let at = editor.offset_in(0, left + panned + editor.metrics.advance * 0.4, window, cx);
        (at, editor.metrics.advance)
    });
    let column = at.expect("a drawn line");
    let expected = (panned / advance).round() as usize;
    assert!(column.abs_diff(expected) <= 1, "{column} near {expected}");
    wheel(400.0, 0.0, &editor, cx);
    assert_eq!(pan(&editor, cx), px(0.0), "the pan stops at the start");
}

#[gpui::test]
fn an_upright_wheel_leaves_the_pan(cx: &mut TestAppContext) {
    let (editor, cx) = editor(&long(), cx);
    cx.run_until_parked();
    wheel(-5.0, -40.0, &editor, cx);
    assert_eq!(pan(&editor, cx), px(0.0));
}

#[gpui::test]
fn the_pan_stops_at_the_widest_line(cx: &mut TestAppContext) {
    let (editor, cx) = editor(&long(), cx);
    cx.run_until_parked();
    wheel(-100_000.0, 0.0, &editor, cx);
    let (x, reach) = editor.update(cx, |editor, _| {
        let reach = editor.metrics.advance * 602.0 - editor.pan.width;
        (editor.pan.x, reach)
    });
    assert!((x - reach).abs() < px(0.5), "{x:?} at {reach:?}");
}

#[gpui::test]
fn a_caret_moved_off_the_view_pans_it_back(cx: &mut TestAppContext) {
    let (editor, cx) = editor(&long(), cx);
    cx.run_until_parked();
    editor.update(cx, |editor, cx| {
        let end = 600..600;
        editor.select([end], cx);
    });
    cx.run_until_parked();
    let (caret, right) = editor.update_in(cx, |editor, window, cx| {
        let bounds = editor.bounds_for_range(600..600, Bounds::default(), window, cx);
        let caret = bounds.expect("a drawn line").origin.x;
        let box_left = editor.metrics.left - editor.metrics.advance + editor.pan.x;
        let right = box_left + editor.pan.width;
        (caret, right)
    });
    assert!(pan(&editor, cx) > px(0.0));
    assert!(caret < right, "{caret:?} shows before {right:?}");
    editor.update(cx, |editor, cx| {
        let start = 0..0;
        editor.select([start], cx);
    });
    cx.run_until_parked();
    assert_eq!(pan(&editor, cx), px(0.0), "back at the line's start");
}
