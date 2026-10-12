use gpui::{Modifiers, TestAppContext};

use super::{MarkdownMode, open, opened, press, text};

#[gpui::test]
fn enter_in_source_goes_on_with_the_list(cx: &mut TestAppContext) {
    let (document, cx) = open("- a", MarkdownMode::Source, cx);
    document.update(cx, |document, cx| document.set_caret(0, 3, cx));
    let editor = document.read_with(cx, |document, _| document.editor().clone());
    let focus = editor.read_with(cx, gpui::Focusable::focus_handle);
    cx.update(|window, cx| window.focus(&focus, cx));
    cx.simulate_keystrokes("enter");
    cx.simulate_input("b");
    assert_eq!(text(&document, cx), "- a\n- b");
    cx.simulate_keystrokes("enter enter");
    assert_eq!(
        text(&document, cx),
        "- a\n- b\n",
        "an empty item ends the list"
    );
}

#[gpui::test]
fn enter_in_a_block_goes_on_with_the_list(cx: &mut TestAppContext) {
    let (document, cx) = open("1. a\n", MarkdownMode::Visual, cx);
    press(0, Modifiers::none(), cx);
    cx.simulate_keystrokes("enter");
    cx.simulate_input("b");
    assert_eq!(opened(&document, cx), "1. a\n2. b");
}

#[gpui::test]
fn tab_shifts_a_block_line_in_and_out(cx: &mut TestAppContext) {
    let (document, cx) = open("- a\n- b\n", MarkdownMode::Visual, cx);
    press(0, Modifiers::none(), cx);
    cx.simulate_keystrokes("tab");
    assert_eq!(opened(&document, cx), "- a\n    - b");
    cx.simulate_keystrokes("shift-tab");
    assert_eq!(opened(&document, cx), "- a\n- b");
}

#[gpui::test]
fn a_table_edit_is_one_undo_step_at_the_caret(cx: &mut TestAppContext) {
    let (document, cx) = open("| A |\n| - |\n| 1 |\n", MarkdownMode::Source, cx);
    document.update(cx, |document, cx| document.set_caret(2, 2, cx));
    let edited = document.update(cx, |document, cx| {
        document.edit_table(crate::documents::TableEdit::RowBelow, cx)
    });
    assert!(edited);
    assert_eq!(text(&document, cx), "| A |\n| --- |\n| 1 |\n|  |\n");
    let editor = document.read_with(cx, |document, _| document.editor().clone());
    editor.update(cx, |editor, cx| editor.undo(cx));
    assert_eq!(text(&document, cx), "| A |\n| - |\n| 1 |\n");
}

#[gpui::test]
fn a_new_table_goes_after_the_caret_line(cx: &mut TestAppContext) {
    let (document, cx) = open("Intro\nNext\n", MarkdownMode::Visual, cx);
    document.update(cx, |document, cx| document.insert_table(cx));
    assert_eq!(
        text(&document, cx),
        "Intro\n\n|   |   |   |\n| --- | --- | --- |\n|   |   |   |\n|   |   |   |\n\nNext\n"
    );
}

#[gpui::test]
fn a_match_in_visual_opens_its_block_selected(cx: &mut TestAppContext) {
    let (document, cx) = open("# A\n\nfind me here\n", MarkdownMode::Visual, cx);
    let at = "# A\n\nfind me here\n".find("me").expect("the word");
    document.update_in(cx, |document, window, cx| {
        document.show_match(at..at + 2, window, cx)
    });
    let selected = document.read_with(cx, |document, cx| {
        let input = document
            .open
            .as_ref()
            .expect("a block opens")
            .input
            .read(cx);
        input.text()[input.selection()].to_string()
    });
    assert_eq!(selected, "me");
}

#[gpui::test]
fn every_replacement_is_one_undo_step(cx: &mut TestAppContext) {
    let (document, cx) = open("a b a", MarkdownMode::Visual, cx);
    document.update(cx, |document, cx| {
        document.replace(vec![(0..1, "x".into()), (4..5, "x".into())], cx)
    });
    assert_eq!(text(&document, cx), "x b x");
    let editor = document.read_with(cx, |document, _| document.editor().clone());
    editor.update(cx, |editor, cx| editor.undo(cx));
    assert_eq!(text(&document, cx), "a b a");
}
