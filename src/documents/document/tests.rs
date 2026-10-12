use std::{cell::RefCell, rc::Rc};

use gpui::{Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext, point, px};

use super::{AT_ONCE, DocumentEvent, MarkdownDocument, visual};
use crate::documents::MarkdownMode;

const TEXT: &str = "# Lift\n\nBlend toward white.\n\n- one\n- two\n";

fn open<'a>(
    text: &str,
    mode: MarkdownMode,
    cx: &'a mut TestAppContext,
) -> (Entity<MarkdownDocument>, &'a mut VisualTestContext) {
    cx.update(crate::init_for_tests);
    let text = text.to_string();
    let (document, cx) =
        cx.add_window_view(move |window, cx| MarkdownDocument::new(text, mode, window, cx));
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    (document, cx)
}

/// Presses block `ix` where it is drawn.
fn press(ix: usize, modifiers: Modifiers, cx: &mut VisualTestContext) {
    let bounds = cx
        .debug_bounds(Box::leak(format!("markdown-block-{ix}").into_boxed_str()))
        .expect("the block is drawn");
    let at = point(bounds.left() + px(4.0), bounds.top() + px(4.0));
    cx.simulate_mouse_down(at, MouseButton::Left, modifiers);
    cx.simulate_mouse_up(at, MouseButton::Left, modifiers);
    cx.run_until_parked();
}

fn text(document: &Entity<MarkdownDocument>, cx: &mut VisualTestContext) -> String {
    document.read_with(cx, |document, cx| document.text(cx))
}

#[gpui::test]
fn a_block_typed_in_is_the_text_at_once_and_one_undo_step(cx: &mut TestAppContext) {
    let (document, cx) = open(TEXT, MarkdownMode::Visual, cx);
    let changes = Rc::new(RefCell::new(0));
    let counted = changes.clone();
    cx.update(|_, cx| {
        cx.subscribe(&document, move |_, event: &DocumentEvent, _| {
            if *event == DocumentEvent::Changed {
                *counted.borrow_mut() += 1;
            }
        })
        .detach()
    });
    press(1, Modifiers::none(), cx);
    cx.simulate_input(" Again.");
    assert_eq!(
        text(&document, cx),
        "# Lift\n\nBlend toward white. Again.\n\n- one\n- two\n",
        "typing shows in the text before it is written back"
    );
    assert!(*changes.borrow() > 0, "each key tells the owner");
    cx.simulate_keystrokes("escape");
    let editor = document.read_with(cx, |document, _| document.editor().clone());
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.text().to_string()),
        text(&document, cx),
        "written back"
    );
    cx.dispatch_action(visual::Undo);
    assert_eq!(text(&document, cx), TEXT, "one step undoes the block");
    cx.dispatch_action(visual::Redo);
    assert!(text(&document, cx).contains("Again."));
}

#[gpui::test]
fn the_same_text_and_history_cross_modes(cx: &mut TestAppContext) {
    let (document, cx) = open(TEXT, MarkdownMode::Source, cx);
    let editor = document.read_with(cx, |document, _| document.editor().clone());
    editor.update(cx, |editor, cx| {
        editor.edit([(0..0, "Intro\n\n".to_string())], cx)
    });
    document.update_in(cx, |document, window, cx| {
        document.set_mode(MarkdownMode::Visual, window, cx)
    });
    cx.run_until_parked();
    assert_eq!(text(&document, cx), format!("Intro\n\n{TEXT}"));
    cx.dispatch_action(visual::Undo);
    assert_eq!(
        text(&document, cx),
        TEXT,
        "the source edit undoes in visual"
    );
}

#[gpui::test]
fn a_mode_switch_writes_the_open_block_and_changes_nothing_else(cx: &mut TestAppContext) {
    let (document, cx) = open(TEXT, MarkdownMode::Visual, cx);
    press(0, Modifiers::none(), cx);
    document.update_in(cx, |document, window, cx| {
        document.set_mode(MarkdownMode::Preview, window, cx)
    });
    assert_eq!(
        text(&document, cx),
        TEXT,
        "an unchanged block writes nothing"
    );
    let undone = document.read_with(cx, |document, cx| {
        document.editor().read(cx).history.can_undo()
    });
    assert!(!undone, "no step was recorded");
}

#[gpui::test]
fn preview_and_a_secondary_press_open_no_block(cx: &mut TestAppContext) {
    let (document, cx) = open(TEXT, MarkdownMode::Preview, cx);
    press(1, Modifiers::none(), cx);
    assert!(document.read_with(cx, |document, _| document.open.is_none()));
    document.update_in(cx, |document, window, cx| {
        document.set_mode(MarkdownMode::Visual, window, cx)
    });
    cx.run_until_parked();
    press(1, Modifiers::secondary_key(), cx);
    assert!(
        document.read_with(cx, |document, _| document.open.is_none()),
        "Cmd or Ctrl leaves the press to links"
    );
}

#[gpui::test]
fn down_at_a_blocks_last_line_opens_the_next(cx: &mut TestAppContext) {
    let (document, cx) = open(TEXT, MarkdownMode::Visual, cx);
    press(0, Modifiers::none(), cx);
    cx.simulate_keystrokes("down");
    let opened = document.read_with(cx, |document, cx| {
        let open = document.open.as_ref().expect("a block is open");
        document.text(cx)[open.range.clone()].to_string()
    });
    assert_eq!(opened, "Blend toward white.");
}

#[gpui::test]
fn reveal_puts_the_caret_on_the_line(cx: &mut TestAppContext) {
    let (document, cx) = open(TEXT, MarkdownMode::Source, cx);
    document.update_in(cx, |document, window, cx| document.reveal(4, window, cx));
    assert_eq!(
        document.read_with(cx, |document, cx| document.caret(cx)),
        (4, 0)
    );
    cx.simulate_input("x");
    assert!(
        text(&document, cx).contains(
            "
x- one"
        ),
        "keys land on the line"
    );
    let outline: Vec<(String, usize)> = document.read_with(cx, |document, cx| {
        document
            .outline(cx)
            .into_iter()
            .map(|(heading, line)| (heading.title, line))
            .collect()
    });
    assert_eq!(outline, [("Lift".to_string(), 0)]);
}

#[gpui::test]
fn reset_starts_undo_afresh(cx: &mut TestAppContext) {
    let (document, cx) = open(TEXT, MarkdownMode::Visual, cx);
    document.update(cx, |document, cx| document.set_text("changed", cx));
    document.update(cx, |document, cx| document.reset("from disk", cx));
    cx.dispatch_action(visual::Undo);
    assert_eq!(text(&document, cx), "from disk");
}

#[gpui::test]
fn a_long_text_reads_off_the_main_thread(cx: &mut TestAppContext) {
    let long = "A paragraph of prose.\n\n".repeat(AT_ONCE / 20);
    let (document, cx) = open(&long, MarkdownMode::Visual, cx);
    let (read, blocks) = document.read_with(cx, |document, _| {
        (document.read_now(), document.parsed.blocks.len())
    });
    assert!(read, "read once parked");
    assert_eq!(blocks, AT_ONCE / 20);
    document.update(cx, |document, cx| document.set_text("# Short\n", cx));
    assert!(
        document.read_with(cx, |document, _| document.read_now()),
        "a short text reads at once"
    );
}

#[gpui::test]
fn a_press_past_the_end_adds_a_block(cx: &mut TestAppContext) {
    let (document, cx) = open("# Lift", MarkdownMode::Visual, cx);
    let end = cx.debug_bounds("markdown-block-0").expect("drawn").bottom() + px(12.0);
    cx.simulate_mouse_down(point(px(40.0), end), MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    cx.simulate_input("More.");
    assert_eq!(text(&document, cx), "# Lift\n\nMore.");
    cx.simulate_keystrokes("escape");
    let editor = document.read_with(cx, |document, _| document.editor().clone());
    let written = editor.read_with(cx, |editor, _| editor.text().to_string());
    assert_eq!(written, "# Lift\n\nMore.");
}

#[gpui::test]
fn front_matter_opens_as_written(cx: &mut TestAppContext) {
    let (document, cx) = open("---\ntitle: a\n---\n\nBody\n", MarkdownMode::Visual, cx);
    press(0, Modifiers::none(), cx);
    let opened = document.read_with(cx, |document, cx| {
        document
            .open
            .as_ref()
            .expect("open")
            .input
            .read(cx)
            .text()
            .to_string()
    });
    assert_eq!(opened, "---\ntitle: a\n---");
}

#[gpui::test]
fn the_source_colors_markdown(cx: &mut TestAppContext) {
    let (document, cx) = open(TEXT, MarkdownMode::Source, cx);
    let colors = document.read_with(cx, |document, cx| {
        document.editor().read(cx).line_colors("a **b**", cx)
    });
    assert!(
        colors.iter().any(|(range, style)| *range == (2..7)
            && style.font_weight == Some(gpui::FontWeight::SEMIBOLD)),
        "{colors:?}"
    );
}

#[gpui::test]
fn down_past_the_last_block_keeps_it_open(cx: &mut TestAppContext) {
    let (document, cx) = open("# Lift\n\nLast.", MarkdownMode::Visual, cx);
    press(1, Modifiers::none(), cx);
    cx.simulate_keystrokes("down");
    let opened = document.read_with(cx, |document, cx| {
        let open = document.open.as_ref().expect("still open");
        document.text(cx)[open.range.clone()].to_string()
    });
    assert_eq!(opened, "Last.");
}
