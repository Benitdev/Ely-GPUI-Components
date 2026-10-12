use gpui::{TestAppContext, px, rems};

use super::{MarkdownMode, frames, open};
use crate::documents::DocumentLook;

/// Sixty short paragraphs, each its own block.
fn long() -> String {
    (0..60).map(|n| format!("Paragraph {n}.\n\n")).collect()
}

#[gpui::test]
fn the_measure_narrows_the_column(cx: &mut TestAppContext) {
    let (document, cx) = open("A paragraph.\n", MarkdownMode::Visual, cx);
    let look = DocumentLook {
        measure: Some(rems(20.)),
        ..DocumentLook::default()
    };
    document.update(cx, |document, cx| document.set_look(look, cx));
    frames(cx);
    let block = cx.debug_bounds("markdown-block-0").expect("drawn");
    assert!(block.size.width <= px(320.0), "{:?}", block.size);
}

#[gpui::test]
fn typewriter_keeps_the_open_block_mid_view(cx: &mut TestAppContext) {
    let (document, cx) = open(&long(), MarkdownMode::Visual, cx);
    let look = DocumentLook {
        typewriter: true,
        ..DocumentLook::default()
    };
    document.update_in(cx, |document, window, cx| {
        document.set_look(look, cx);
        document.open(Some(40), false, window, cx);
        document.list.scroll_to_reveal_item(40);
    });
    frames(cx);
    let (block, view) = document.read_with(cx, |document, _| {
        (
            document.list.bounds_for_item(40),
            document.list.viewport_bounds(),
        )
    });
    let block = block.expect("block 40 is laid out");
    let off = (block.center().y - view.center().y).abs();
    assert!(off < px(4.0), "{off:?} from the middle");
}

#[gpui::test]
fn typewriter_keeps_the_source_caret_mid_view(cx: &mut TestAppContext) {
    let (document, cx) = open(&long(), MarkdownMode::Source, cx);
    let look = DocumentLook {
        typewriter: true,
        ..DocumentLook::default()
    };
    document.update(cx, |document, cx| {
        document.set_look(look, cx);
        document.set_caret(90, 0, cx);
    });
    frames(cx);
    let before = document.read_with(cx, |document, cx| document.top_line(cx));
    let editor = document.read_with(cx, |document, _| document.editor().clone());
    let focus = editor.read_with(cx, gpui::Focusable::focus_handle);
    cx.update(|window, cx| window.focus(&focus, cx));
    cx.simulate_input(
        "









",
    );
    frames(cx);
    let after = document.read_with(cx, |document, cx| document.top_line(cx));
    assert_eq!(after, before + 10, "the view follows the caret");
}

#[gpui::test]
fn the_scale_sizes_source_lines(cx: &mut TestAppContext) {
    let (document, cx) = open("a\n", MarkdownMode::Source, cx);
    let editor = document.read_with(cx, |document, _| document.editor().clone());
    frames(cx);
    let plain = editor.read_with(cx, |editor, _| editor.metrics.line);
    let look = DocumentLook {
        scale: 2.0,
        ..DocumentLook::default()
    };
    document.update(cx, |document, cx| document.set_look(look, cx));
    frames(cx);
    let large = editor.read_with(cx, |editor, _| editor.metrics.line);
    assert!(large > plain * 1.8, "{plain:?} to {large:?}");
}

#[gpui::test]
fn focus_counts_the_open_block_as_the_readers(cx: &mut TestAppContext) {
    let (document, cx) = open(&long(), MarkdownMode::Visual, cx);
    document.update_in(cx, |document, window, cx| {
        document.open(Some(3), false, window, cx)
    });
    assert_eq!(
        document.read_with(cx, |document, cx| document.current_block(cx)),
        3
    );
}

fn typewriter() -> DocumentLook {
    DocumentLook {
        typewriter: true,
        ..DocumentLook::default()
    }
}

/// How far block `ix` sits from mid-view.
fn off_middle(
    document: &gpui::Entity<super::MarkdownDocument>,
    ix: usize,
    cx: &mut gpui::VisualTestContext,
) -> gpui::Pixels {
    document.read_with(cx, |document, _| {
        let mut block = document.list.bounds_for_item(ix).expect("laid out");
        if ix == 0 {
            block.origin.y += document.room;
            block.size.height -= document.room;
        }
        if ix == document.parsed.blocks.len() {
            block.size.height -= document.room;
        }
        (block.center().y - document.list.viewport_bounds().center().y).abs()
    })
}

#[gpui::test]
fn an_open_block_takes_the_scale(cx: &mut TestAppContext) {
    let (document, cx) = open("A paragraph.\n", MarkdownMode::Visual, cx);
    document.update_in(cx, |document, window, cx| {
        document.open(Some(0), false, window, cx)
    });
    frames(cx);
    let plain = document
        .read_with(cx, |document, _| document.list.bounds_for_item(0))
        .expect("laid out");
    let large = DocumentLook {
        scale: 2.0,
        ..DocumentLook::default()
    };
    document.update(cx, |document, cx| document.set_look(large, cx));
    frames(cx);
    let scaled = document
        .read_with(cx, |document, _| document.list.bounds_for_item(0))
        .expect("laid out");
    assert!(
        scaled.size.height > plain.size.height * 1.5,
        "{plain:?} to {scaled:?}"
    );
}

#[gpui::test]
fn a_resize_centers_the_block_again(cx: &mut TestAppContext) {
    let (document, cx) = open(&long(), MarkdownMode::Visual, cx);
    document.update_in(cx, |document, window, cx| {
        document.set_look(typewriter(), cx);
        document.open(Some(40), false, window, cx);
        document.list.scroll_to_reveal_item(40);
    });
    frames(cx);
    cx.simulate_resize(gpui::size(px(1000.0), px(400.0)));
    frames(cx);
    assert!(off_middle(&document, 40, cx) < px(4.0));
}

#[gpui::test]
fn the_first_and_last_blocks_center(cx: &mut TestAppContext) {
    let (document, cx) = open(&long(), MarkdownMode::Visual, cx);
    document.update_in(cx, |document, window, cx| {
        document.set_look(typewriter(), cx);
        document.open(Some(0), false, window, cx);
    });
    frames(cx);
    let first = off_middle(&document, 0, cx);
    assert!(first < px(4.0), "the first is {first:?} off");
    document.update_in(cx, |document, window, cx| {
        document.open(Some(59), false, window, cx);
        document.list.scroll_to_reveal_item(59);
    });
    frames(cx);
    assert!(off_middle(&document, 59, cx) < px(4.0), "the last");
}

#[gpui::test]
fn the_last_source_line_centers_and_a_resize_follows(cx: &mut TestAppContext) {
    let (document, cx) = open(&long(), MarkdownMode::Source, cx);
    document.update(cx, |document, cx| {
        document.set_look(typewriter(), cx);
        document.set_caret(119, 0, cx);
    });
    frames(cx);
    let top = document.read_with(cx, |document, cx| document.top_line(cx));
    assert!(
        top > 80,
        "top {top}: the last line is mid-view, not at the bottom"
    );
    cx.simulate_resize(gpui::size(px(1000.0), px(400.0)));
    frames(cx);
    let resized = document.read_with(cx, |document, cx| document.top_line(cx));
    let editor = document.read_with(cx, |document, _| document.editor().clone());
    let (line, height) =
        editor.read_with(cx, |editor, _| (editor.metrics.line, editor.metrics.height));
    let half = (height / line / 2.0).floor() as usize;
    let middle = 119 - half;
    assert!(
        resized.abs_diff(middle) <= 1,
        "top {top} to {resized}; mid-view is {middle}"
    );
}
