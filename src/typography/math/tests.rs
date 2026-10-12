use super::*;

fn formula(source: &str, display: bool) -> Formula {
    Formula {
        source: source.to_string().into(),
        display,
        rgba: 0x112233ff,
        font: 1600,
        scale: 200,
    }
}

#[test]
fn a_formula_typesets_to_outlined_svg_in_its_color() {
    let (svg, shape) = svg(&formula(r"\frac{a}{b} + \sqrt{x}", false)).expect("typesets");
    assert!(svg.starts_with("<svg") && !svg.contains("<text"), "{svg}");
    assert!(svg.contains("17,34,51"), "the color: {svg}");
    assert!(shape.width > px(16.) && shape.height > px(16.), "{shape:?}");
}

#[test]
fn display_style_sets_limits_larger() {
    let (_, text) = svg(&formula(r"\sum_{k=1}^{n} k", false)).expect("typesets");
    let (_, display) = svg(&formula(r"\sum_{k=1}^{n} k", true)).expect("typesets");
    assert!(display.height > text.height, "{display:?} over {text:?}");
}

#[test]
fn every_glyph_lies_inside_the_picture() {
    for source in [
        r"\text{私密} x",
        "x",
        r"\int_0^1 f(x)\,dx",
        r"\left(\frac{a}{b}\right)",
    ] {
        let (svg, shape) = svg(&formula(source, false)).expect("typesets");
        let tree = usvg::Tree::from_data(svg.as_bytes(), &usvg::Options::default()).expect("svg");
        let ink = tree.root().abs_stroke_bounding_box();
        let (w, h) = (shape.width.as_f32(), shape.height.as_f32());
        let near = |a: f32, b: f32| (a - b).abs() < 0.01;
        assert!(
            near(tree.size().width(), w) && near(tree.size().height(), h),
            "{source}"
        );
        assert!(
            ink.left() >= -0.01 && ink.top() >= -0.01,
            "{source}: ink {ink:?} starts before the picture"
        );
        assert!(
            ink.right() <= w + 0.01 && ink.bottom() <= h + 0.01,
            "{source}: ink {ink:?} passes {w}×{h}"
        );
    }
}

#[test]
fn bad_tex_says_why_and_where() {
    let Err(error) = Latex::parse("x", r"x^2^3") else {
        panic!("a double superscript parses")
    };
    assert_eq!(error.to_string(), "Double superscript at 3");
    assert!(Latex::parse("x", r"x^{2").is_err(), "an open brace");
}

#[test]
fn a_layout_panic_is_an_error_without_the_panic_text() {
    let laid: Result<(), MathError> = guarded(|| panic!("private formula text"));
    let error = laid.expect_err("the panic is an error");
    assert!(!error.to_string().contains("private"), "{error}");
}

/// Formulas drawn by id; the test changes them.
struct Shown(Vec<&'static str>);

impl gpui::Render for Shown {
    fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        div().children(
            self.0
                .iter()
                .enumerate()
                .map(|(ix, source)| Latex::parse(ix, source).expect("parses")),
        )
    }
}

fn open<'a>(
    sources: Vec<&'static str>,
    cx: &'a mut gpui::TestAppContext,
) -> (gpui::Entity<Shown>, &'a mut gpui::VisualTestContext) {
    cx.update(crate::theme::Theme::init);
    let (view, cx) = cx.add_window_view(|_, _| Shown(sources));
    settle(cx);
    (view, cx)
}

fn settle(cx: &mut gpui::VisualTestContext) {
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
    }
    cx.run_until_parked();
}

/// Whether `source`'s picture sits in the asset cache.
fn cached(source: &str, cx: &mut gpui::VisualTestContext) -> bool {
    cx.update(|window, cx| {
        let latex = Latex::parse("probe", source).expect("parses");
        let (formula, _) = latex.formula(window, cx);
        cx.has_asset::<Typesetter>(&formula)
    })
}

#[gpui::test]
fn a_formula_frees_its_picture_when_it_changes_or_leaves(cx: &mut gpui::TestAppContext) {
    let (view, cx) = open(vec!["a^2"], cx);
    assert!(cached("a^2", cx), "drawn once");
    view.update(cx, |view, cx| {
        view.0 = vec!["b^2"];
        cx.notify();
    });
    settle(cx);
    assert!(!cached("a^2", cx) && cached("b^2", cx), "the old one freed");
    view.update(cx, |view, cx| {
        view.0.clear();
        cx.notify();
    });
    settle(cx);
    assert!(!cached("b^2", cx), "gone with its element");
}

#[gpui::test]
fn a_shared_formula_draws_on_after_one_user_leaves(cx: &mut gpui::TestAppContext) {
    let (view, cx) = open(vec!["c^2", "c^2"], cx);
    view.update(cx, |view, cx| {
        view.0.pop();
        cx.notify();
    });
    settle(cx);
    assert!(cached("c^2", cx), "the one left loads it again");
}
