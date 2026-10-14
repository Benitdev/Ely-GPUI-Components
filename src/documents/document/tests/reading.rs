//! Drawn text reads back to its source byte (R4.5).

use crate::documents::{
    TargetBlock,
    document::parse::parsed,
    tree::{Inline, Node, lines_at},
};

/// The source byte under drawn `shown` in block 0.
fn read_back(source: &str, shown: &str) -> Option<usize> {
    fn find<'a>(nodes: &'a [Node], shown: &str) -> Option<&'a Inline> {
        nodes.iter().find_map(|node| match node {
            Node::Paragraph(inline) | Node::Heading(_, inline) if inline.text.contains(shown) => {
                Some(inline)
            }
            Node::Quote(body) | Node::Alert { body, .. } | Node::Footnote { body, .. } => {
                find(body, shown)
            }
            _ => None,
        })
    }
    let parsed = parsed(source);
    let block = &parsed.blocks[0];
    let inline = find(block.nodes.as_ref().expect("drawn"), shown).expect("shown");
    let at = inline.text.find(shown).expect("shown");
    let written = inline.source_at(&source[block.range.start..], at)?;
    Some(block.range.start + written)
}

/// The source byte under drawn `word` of block 0's code.
fn code_back(source: &str, word: &str) -> Option<usize> {
    fn code(nodes: &[Node]) -> (&str, usize) {
        match &nodes[0] {
            Node::Code { text, from, .. } => (text, *from),
            Node::Quote(body) => code(body),
            other => panic!("not code: {other:?}"),
        }
    }
    let parsed = parsed(source);
    let block = &parsed.blocks[0];
    let (drawn, from) = code(block.nodes.as_ref().expect("drawn"));
    let at = drawn.find(word).expect("drawn");
    let written = lines_at(&source[block.range.start..], from, drawn, at)?;
    Some(block.range.start + written)
}

#[test]
fn text_reads_back_past_marks_escapes_trims_and_markers() {
    let cases = [
        ("> alpha\n> beta", "eta"),
        ("> [!NOTE]\n> alpha beta", "beta"),
        ("> [!NOTE]  \n> alpha beta", "beta"),
        ("> [!NOTE]\\\n> alpha beta", "beta"),
        ("> [!NOTE]\n> \\*beta\\*", "*beta"),
        ("> [!NOTE]&#10;alpha beta", "beta"),
        ("a \\*star\\* z", "*star"),
        ("a \\\\ z", "\\ z"),
        ("a ` xy ` z", "xy"),
        ("a `  xy  ` z", " xy "),
        ("a ` >xy ` z", ">xy"),
        ("a `\\\\xy` z", "xy"),
        ("<div>alpha beta</div>\n", "eta"),
        ("> <div>\n> alpha beta</div>\n", "beta"),
        ("> <div>\n> >alpha beta</div>", ">alpha"),
        ("> <div>\n> >alpha beta</div>", "beta"),
        ("<div>alpha <b>beta</b> gamma</div>", "gamma"),
        ("<div>alpha <b>beta</b> gamma</div>", "beta"),
        ("<div>alpha <b> beta</b></div>", "eta"),
        ("<div>one</div>\n<div>alpha beta</div>", "beta"),
    ];
    for (source, shown) in cases {
        assert_eq!(read_back(source, shown), source.find(shown), "{source:?}");
    }
}

#[test]
fn a_reference_reads_back_to_its_ampersand() {
    let source = "a &amp; b";
    assert_eq!(read_back(source, "& b"), source.find('&'));
    assert_eq!(read_back(source, "b"), source.find('b'));
}

#[test]
fn text_after_an_emoji_reads_back_and_before_it_lands_at_its_start() {
    let source = "ab :smile: y";
    assert_eq!(read_back(source, "y"), source.find('y'));
    assert_eq!(read_back(source, "b"), Some(0));
}

#[test]
fn code_reads_back_line_for_line_whatever_its_line_ends() {
    let cases = [
        "```\nalpha\nbeta\n```\n",
        "```\r\nalpha\r\nbeta\r\n```\r\n",
        "```\ralpha\rbeta\r```\r",
        "    alpha\n    beta\n",
        "> ```\n> alpha\n> beta\n> ```\n",
    ];
    for source in cases {
        for word in ["pha", "eta"] {
            assert_eq!(code_back(source, word), source.find(word), "{source:?}");
        }
    }
}

#[test]
fn code_cut_from_a_tab_reads_back_past_it() {
    for spaces in 0..=3 {
        for prefix in ["\t", " \t", "  \t", "   \t", "\t\t"] {
            let indent = " ".repeat(spaces);
            let source = format!("{indent}```\n{indent}alpha\n{prefix}beta\n{indent}```\n");
            assert_eq!(code_back(&source, "eta"), source.find("eta"), "{source:?}");
        }
    }
}

#[test]
fn a_decoded_line_end_reads_without_a_written_one() {
    let quote = parsed("> alpha&#10;beta");
    assert_eq!(quote.blocks[0].kind, TargetBlock::Quote);
    let alert = parsed("> [!NOTE]&#10;alpha beta");
    assert_eq!(alert.blocks[0].kind, TargetBlock::Alert);
}
