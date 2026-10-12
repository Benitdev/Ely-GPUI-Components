use super::*;

#[test]
fn blocks_nest_as_written() {
    let nodes = parse(
        "# Lift\n\nBlend **toward** [white](https://ely.dev).\n\n- [x] clamp\n- [ ] gamma\n\n> quoted\n\n```rust\nfn main() {}\n```\n\n---\n",
    );
    assert!(matches!(&nodes[0], Node::Heading(1, text) if text.text == "Lift"));
    let Node::Paragraph(line) = &nodes[1] else {
        panic!("a paragraph, not {:?}", nodes[1])
    };
    assert_eq!(line.text, "Blend toward white.");
    assert_eq!(line.styles, [(6..12, Style::Strong), (13..18, Style::Link)]);
    assert_eq!(line.links, [(13..18, "https://ely.dev".to_string())]);
    let Node::List { start: None, items } = &nodes[2] else {
        panic!("a list, not {:?}", nodes[2])
    };
    assert_eq!(
        items.iter().map(|(task, _)| *task).collect::<Vec<_>>(),
        [Some(true), Some(false)]
    );
    assert!(matches!(&items[0].1[0], Node::Paragraph(text) if text.text == "clamp"));
    assert!(matches!(&nodes[3], Node::Quote(inner) if inner.len() == 1));
    assert_eq!(
        nodes[4],
        Node::Code {
            language: Some("rust".into()),
            text: "fn main() {}".into()
        }
    );
    assert_eq!(nodes[5], Node::Rule);
}

#[test]
fn an_image_inside_an_image_is_the_outer_one() {
    // markdown-rs drops the inner image from the alt.
    assert_eq!(
        parse("![outer ![inner](inner.png)](outer.png)"),
        [Node::Image {
            alt: "outer ".into(),
            url: "outer.png".into(),
            link: None
        }]
    );
}

#[test]
fn an_image_splits_its_paragraph_and_references_resolve() {
    let nodes = parse("Before ![a][pic] after [site].\n\n[pic]: a.png\n[site]: https://ely.dev\n");
    assert!(matches!(&nodes[0], Node::Paragraph(text) if text.text == "Before "));
    assert_eq!(
        nodes[1],
        Node::Image {
            alt: "a".into(),
            url: "a.png".into(),
            link: None
        }
    );
    let Node::Paragraph(after) = &nodes[2] else {
        panic!("a paragraph, not {:?}", nodes[2])
    };
    assert_eq!(after.text, " after site.");
    assert_eq!(after.links, [(7..11, "https://ely.dev".to_string())]);
    assert_eq!(nodes.len(), 3, "definitions draw nothing: {nodes:?}");
}

#[test]
fn a_soft_break_reads_as_a_space_and_a_hard_one_as_a_line() {
    for (source, shown) in [
        ("one\ntwo  \nthree", "one two\nthree"),
        ("one\r\ntwo  \r\nthree", "one two\nthree"),
        ("one\rtwo  \rthree", "one two\nthree"),
    ] {
        let nodes = parse(source);
        assert!(
            matches!(&nodes[0], Node::Paragraph(text) if text.text == shown),
            "{source:?} reads {nodes:?}"
        );
    }
}

#[test]
fn definitions_nested_in_quotes_and_lists_resolve() {
    for source in [
        "[site] ![pic]\n\n> [site]: https://ely.dev\n> [pic]: a.png\n",
        "[site] ![pic]\n\n- [site]: https://ely.dev\n- [pic]: a.png\n",
    ] {
        let nodes = parse(source);
        let Node::Paragraph(text) = &nodes[0] else {
            panic!("a paragraph, not {:?}", nodes[0])
        };
        assert_eq!(text.links, [(0..4, "https://ely.dev".to_string())]);
        assert_eq!(
            nodes[1],
            Node::Image {
                alt: "pic".into(),
                url: "a.png".into(),
                link: None
            }
        );
    }
}

#[test]
fn an_image_inside_styles_and_links_stays_an_image() {
    let nodes = parse("*a ![x](x.png) b* [![y](y.png)](https://ely.dev)");
    let Node::Paragraph(before) = &nodes[0] else {
        panic!("a paragraph, not {:?}", nodes[0])
    };
    assert_eq!(
        (before.text.as_str(), &before.styles[..]),
        ("a ", &[(0..2, Style::Emphasis)][..])
    );
    assert_eq!(
        nodes[1],
        Node::Image {
            alt: "x".into(),
            url: "x.png".into(),
            link: None
        }
    );
    let Node::Paragraph(after) = &nodes[2] else {
        panic!("a paragraph, not {:?}", nodes[2])
    };
    assert_eq!(
        (after.text.as_str(), &after.styles[..]),
        (" b ", &[(0..2, Style::Emphasis)][..])
    );
    assert_eq!(
        nodes[3],
        Node::Image {
            alt: "y".into(),
            url: "y.png".into(),
            link: Some("https://ely.dev".into())
        }
    );
    assert_eq!(nodes.len(), 4, "{nodes:?}");
}

#[test]
fn a_heading_keeps_its_images_after_it() {
    let nodes = parse("# Lift ![x](x.png)");
    assert!(matches!(&nodes[0], Node::Heading(1, text) if text.text == "Lift "));
    assert!(matches!(&nodes[1], Node::Image { url, .. } if url == "x.png"));
}

#[test]
fn display_math_stands_alone() {
    assert_eq!(parse("$$c = 1$$"), [Node::Math("c = 1".into())]);
    assert_eq!(parse("$$\nc = 1\n$$"), [Node::Math("c = 1".into())]);
    let Node::Paragraph(inline) = &parse("so $c$ holds")[0] else {
        panic!("inline math stays in its paragraph")
    };
    assert_eq!(inline.styles, [(3..4, Style::Math)]);
}

#[test]
fn tables_keep_their_cells_and_alignment() {
    let nodes = parse("| Name | Lift |\n|:--|--:|\n| accent | 0.5 |\n");
    let Node::Table { aligns, head, rows } = &nodes[0] else {
        panic!("a table, not {:?}", nodes[0])
    };
    assert_eq!(aligns, &[AlignKind::Left, AlignKind::Right]);
    let text = |cell: &[Node]| match cell {
        [Node::Paragraph(line)] => line.text.clone(),
        other => panic!("one line, not {other:?}"),
    };
    assert_eq!(
        head.iter().map(|cell| text(cell)).collect::<Vec<_>>(),
        ["Name", "Lift"]
    );
    assert_eq!(text(&rows[0][1]), "0.5");
}

#[test]
fn a_picture_in_a_cell_stays_a_picture() {
    let nodes = parse("| Picture |\n| --- |\n| ![shot](assets/shot.png) |\n");
    let Node::Table { rows, .. } = &nodes[0] else {
        panic!("a table, not {:?}", nodes[0])
    };
    assert!(
        matches!(&rows[0][0][..], [Node::Image { alt, url, .. }] if alt == "shot" && url == "assets/shot.png")
    );
}

#[test]
fn footnotes_number_their_references_and_hold_their_body() {
    let nodes = parse("Lift[^a].\n\n[^a]: Toward white.\n");
    let Node::Paragraph(line) = &nodes[0] else {
        panic!("a paragraph, not {:?}", nodes[0])
    };
    assert_eq!(line.text, "Lift[a].");
    assert_eq!(line.styles, [(4..7, Style::Footnote)]);
    assert!(matches!(&nodes[1], Node::Footnote { label, body } if label == "a" && body.len() == 1));
}
