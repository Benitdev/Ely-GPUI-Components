//! Block commands whose results reread as written.

use markdown::mdast::Node;

use super::block::{kinds, ok, ran};
use crate::documents::{BlockCommand, EditRefusal, markdown::file_tree};

fn plain(marked: &str) -> String {
    marked.chars().filter(|ch| !"‸⟨⟩".contains(*ch)).collect()
}

/// Every node of `text`, in document order.
fn nodes(text: &str, keep: &mut dyn FnMut(&Node)) {
    fn walk(node: &Node, keep: &mut dyn FnMut(&Node)) {
        keep(node);
        for kid in node.children().map(Vec::as_slice).unwrap_or_default() {
            walk(kid, keep);
        }
    }
    walk(&file_tree(text), keep);
}

fn checks(text: &str) -> Vec<Option<bool>> {
    let mut out = Vec::new();
    nodes(text, &mut |node| {
        if let Node::ListItem(item) = node {
            out.push(item.checked);
        }
    });
    out
}

fn codes(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    nodes(text, &mut |node| {
        if let Node::Code(code) = node {
            out.push(code.value.clone());
        }
    });
    out
}

fn texts(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    nodes(text, &mut |node| {
        if let Node::Text(text) = node {
            out.push(text.value.clone());
        }
    });
    out
}

#[test]
fn a_split_that_would_reread_otherwise_refuses() {
    assert_eq!(
        ran("~~ab‸~cd~~", BlockCommand::Rule),
        Err(EditRefusal::Misread)
    );
    assert_eq!(
        ran("__ab‸_cd__", BlockCommand::Rule),
        Err(EditRefusal::Misread)
    );
}

#[test]
fn a_split_rest_that_would_open_a_block_is_escaped() {
    let out = plain(&ok("a ‸- b", BlockCommand::Rule));
    assert_eq!(out, "a\n\n---\n\n\\- b");
    assert_eq!(kinds(&out), ["p", "rule", "p"]);
}

#[test]
fn closing_hashes_stay_text_wherever_a_heading_line_ends() {
    let out = plain(&ok("alpha #‸\n===", BlockCommand::Heading(3)));
    assert_eq!(out, "### alpha \\#");
    assert_eq!(texts(&out), ["alpha #"]);
    let out = plain(&ok("# a #‸ b", BlockCommand::Rule));
    assert_eq!(kinds(&out), ["h", "rule", "h"]);
    assert_eq!(texts(&out), ["a #", "b"]);
}

#[test]
fn a_paragraph_reads_the_heading_body_without_closing_hashes() {
    let out = plain(&ok("# ---‸ ###", BlockCommand::Paragraph));
    assert_eq!(out, "\\---");
    assert_eq!(kinds(&out), ["p"]);
    let out = plain(&ok("# [x]: /u‸ ###", BlockCommand::Paragraph));
    assert_eq!(kinds(&out), ["p"]);
    assert_eq!(texts(&out).concat(), "[x]: /u");
}

#[test]
fn a_task_box_below_its_marker_stays_checked() {
    let out = plain(&ok("1.‸\n   [x] a", BlockCommand::TaskList));
    assert_eq!(checks(&out), [Some(true)]);
    assert_eq!(texts(&out), ["a"]);
}

#[test]
fn retyping_keeps_code_columns_past_tabs() {
    let source = "-\t\t‸code";
    let out = plain(&ok(source, BlockCommand::OrderedList));
    assert_eq!(codes(&out), codes(&plain(source)));
    assert_eq!(codes(&out).len(), 1);
}

#[test]
fn a_task_cannot_box_code() {
    assert_eq!(
        ran("-     ‸a", BlockCommand::TaskList),
        Err(EditRefusal::Code)
    );
}

#[test]
fn an_alert_unwraps_into_its_holder() {
    assert_eq!(
        plain(&ok("- > [!NOTE]‸\n    a", BlockCommand::Quote)),
        "- a"
    );
    assert_eq!(
        plain(&ok("> > [!NOTE]‸\n>   a", BlockCommand::Quote)),
        "> a"
    );
    let out = plain(&ok("- > [!NOTE]‸\n  >\n  >\n  > a", BlockCommand::Quote));
    assert_eq!(out, "- a");
    assert_eq!(kinds(&out), ["list"]);
}

#[test]
fn an_alert_marker_sharing_its_line_refuses() {
    assert_eq!(
        ran("> [!NOTE]&#10;a‸", BlockCommand::Quote),
        Err(EditRefusal::Misread)
    );
}

#[test]
fn delete_keeps_neighbors_apart_past_gone_openers() {
    let out = plain(&ok("p\n> # ‸h\nq", BlockCommand::Delete));
    assert_eq!(out, "p\n\nq");
    assert_eq!(kinds(&out), ["p", "p"]);
}
