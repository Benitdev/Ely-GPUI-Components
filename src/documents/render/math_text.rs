//! Inline formulas typeset among a paragraph's words.

use std::{cell::RefCell, ops::Range, rc::Rc, sync::Arc};

use gpui::{
    AnyElement, App, Bounds, Corners, Element, ElementId, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, Pixels, Point, RenderImage, SharedString, Size, TextLayout,
    TextStyleRefinement, Window, point,
};

use super::{OnLink, drawn_text};
use crate::{
    documents::tree::{Inline, Style},
    typography::typeset,
};

/// Glue that reserves a formula's width and never breaks.
const GLUE: char = '\u{00A0}';

/// An original span and the drawn span standing for it.
type Swap = (Range<usize>, Range<usize>);

/// The drawn text and how its bytes map back.
struct Shown {
    layout: TextLayout,
    swaps: Vec<Swap>,
}

/// Paragraph text whose formulas draw typeset over reserved glue.
pub(super) struct MathText {
    id: ElementId,
    inline: Inline,
    links: Option<ElementId>,
    on_link: Option<OnLink>,
    shown: Rc<RefCell<Option<Shown>>>,
    text: Option<AnyElement>,
    /// Each picture, its size, and the drawn span it covers.
    pictures: Vec<(Arc<RenderImage>, Size<Pixels>, Range<usize>)>,
}

impl MathText {
    /// The element and how a point finds its original byte.
    pub(super) fn new(
        id: ElementId,
        inline: Inline,
        links: Option<ElementId>,
        on_link: Option<OnLink>,
    ) -> (Self, impl Fn(Point<Pixels>) -> usize + 'static) {
        let shown = Rc::new(RefCell::new(None));
        let read = shown.clone();
        let at = move |position| match &*read.borrow() {
            Some(Shown { layout, swaps }) => back(
                swaps,
                layout
                    .index_for_position(position)
                    .unwrap_or_else(|near| near),
            ),
            None => 0,
        };
        let text = Self {
            id,
            inline,
            links,
            on_link,
            shown,
            text: None,
            pictures: Vec::new(),
        };
        (text, at)
    }
}

/// Where original byte `at` is drawn.
fn forward(swaps: &[Swap], at: usize) -> usize {
    let mut shift = 0isize;
    for (written, drawn) in swaps {
        if at < written.start {
            break;
        }
        if at < written.end {
            return drawn.start;
        }
        shift = drawn.end as isize - written.end as isize;
    }
    (at as isize + shift) as usize
}

/// The original byte drawn byte `at` stands for.
fn back(swaps: &[Swap], at: usize) -> usize {
    let mut shift = 0isize;
    for (written, drawn) in swaps {
        if at < drawn.start {
            break;
        }
        if at < drawn.end {
            return written.start;
        }
        shift = written.end as isize - drawn.end as isize;
    }
    (at as isize + shift) as usize
}

/// `inline` with each swapped span drawn as `glue[ix]` glue.
fn shown(inline: &Inline, swaps: &mut [Swap], glue: &[usize]) -> Inline {
    let mut text = String::new();
    let mut from = 0;
    for ((written, drawn), count) in swaps.iter_mut().zip(glue) {
        text.push_str(&inline.text[from..written.start]);
        let start = text.len();
        text.extend(std::iter::repeat_n(GLUE, *count));
        *drawn = start..text.len();
        from = written.end;
    }
    text.push_str(&inline.text[from..]);
    let span = |range: &Range<usize>| forward(swaps, range.start)..forward(swaps, range.end);
    Inline {
        text,
        styles: (inline.styles.iter())
            .map(|(range, style)| (span(range), *style))
            .collect(),
        links: (inline.links.iter())
            .map(|(range, url)| (span(range), url.clone()))
            .collect(),
        sources: Vec::new(),
    }
}

impl IntoElement for MathText {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for MathText {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let style = window.text_style();
        let rem = window.rem_size();
        let font = style.font_size.to_pixels(rem);
        let glue = String::from(GLUE);
        let run = style.to_run(glue.len());
        let width = window
            .text_system()
            .shape_line(glue.into(), font, &[run], None)
            .width;
        let mut maths: Vec<Range<usize>> = (self.inline.styles.iter())
            .filter(|(_, style)| *style == Style::Math)
            .map(|(range, _)| range.clone())
            .collect();
        maths.sort_by_key(|range| range.start);
        let mut swaps = Vec::new();
        let mut counts = Vec::new();
        let mut pictures = Vec::new();
        for (ix, range) in maths.into_iter().enumerate() {
            let source = SharedString::from(self.inline.text[range.clone()].to_string());
            match typeset(
                (self.id.clone(), SharedString::from(format!("formula-{ix}"))),
                (&source, false),
                font,
                window,
                cx,
            ) {
                Some(Ok((image, size))) => {
                    counts.push((size.width / width).ceil().max(1.0) as usize);
                    pictures.push((image, size));
                    swaps.push((range, 0..0));
                }
                // The formula is the user's text: not logged.
                Some(Err(_)) => log::warn!("markdown: an inline formula shows as TeX, unset"),
                None => {}
            }
        }
        let inline = shown(&self.inline, &mut swaps, &counts);
        let hidden: Vec<Range<usize>> = swaps.iter().map(|(_, drawn)| drawn.clone()).collect();
        let (mut text, layout) =
            drawn_text(&inline, &hidden, self.links.clone(), &self.on_link, cx);
        let tallest = pictures
            .iter()
            .map(|(_, size)| size.height)
            .fold(Pixels::ZERO, Pixels::max);
        // A tall formula opens the paragraph's lines.
        let taller = (tallest > style.line_height_in_pixels(rem)).then(|| TextStyleRefinement {
            line_height: Some(tallest.into()),
            ..TextStyleRefinement::default()
        });
        let id = window.with_text_style(taller, |window| text.request_layout(window, cx));
        self.pictures = (pictures.into_iter().zip(&hidden))
            .map(|((image, size), drawn)| (image, size, drawn.clone()))
            .collect();
        *self.shown.borrow_mut() = Some(Shown { layout, swaps });
        self.text = Some(text);
        (id, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(text) = &mut self.text {
            text.prepaint(window, cx);
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(text) = &mut self.text else {
            return;
        };
        text.paint(window, cx);
        let shown = self.shown.borrow();
        let Some(Shown { layout, .. }) = &*shown else {
            return;
        };
        let line = layout.line_height();
        for (image, size, drawn) in &self.pictures {
            let (Some(start), Some(end)) = (
                layout.position_for_index(drawn.start),
                layout.position_for_index(drawn.end),
            ) else {
                continue;
            };
            // Glue at a line's end ends there.
            let room = if end.y == start.y {
                end.x - start.x
            } else {
                size.width
            };
            let origin = start + point((room - size.width) / 2.0, (line - size.height) / 2.0);
            let bounds = Bounds {
                origin,
                size: *size,
            };
            if let Err(error) =
                window.paint_image(bounds, bounds, Corners::default(), image.clone(), 0, false)
            {
                log::error!("markdown: an inline formula did not paint: {error}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_map_both_ways_around_a_formula() {
        let inline = Inline {
            text: "a x^2 b".into(),
            styles: vec![(2..5, Style::Math), (6..7, Style::Strong)],
            ..Inline::default()
        };
        let mut swaps = vec![(2..5, 0..0)];
        let drawn = shown(&inline, &mut swaps, &[4]);
        assert_eq!(drawn.text, format!("a {} b", GLUE.to_string().repeat(4)));
        let glue = GLUE.len_utf8() * 4;
        assert_eq!(swaps[0].1, 2..2 + glue);
        assert_eq!(
            drawn.styles[1].0,
            3 + glue..4 + glue,
            "the bold b moves past the glue"
        );
        assert_eq!(back(&swaps, 3), 2, "glue reads back to its formula");
        assert_eq!(back(&swaps, 3 + glue), 6);
        assert_eq!(forward(&swaps, 6), 3 + glue);
    }

    type Found = Rc<
        RefCell<
            Option<(
                Rc<RefCell<Option<Shown>>>,
                Box<dyn Fn(Point<Pixels>) -> usize>,
            )>,
        >,
    >;

    struct Probe(Found);

    impl gpui::Render for Probe {
        fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
            use gpui::{ParentElement as _, Styled as _};
            let inline = Inline {
                text: "a x^2 b".into(),
                styles: vec![(2..5, Style::Math)],
                ..Inline::default()
            };
            let (text, at) = MathText::new("probe".into(), inline, None, None);
            *self.0.borrow_mut() = Some((text.shown.clone(), Box::new(at)));
            gpui::div().text_size(gpui::px(16.0)).child(text)
        }
    }

    #[gpui::test]
    fn a_formula_draws_over_glue_and_reads_back(cx: &mut gpui::TestAppContext) {
        cx.update(crate::theme::Theme::init);
        let found = Found::default();
        let held = found.clone();
        let (_, cx) = cx.add_window_view(move |_, _| Probe(held));
        for _ in 0..4 {
            cx.run_until_parked();
            cx.update(|window, _| window.refresh());
        }
        let found = found.borrow();
        let (shown, at) = found.as_ref().expect("drawn");
        let shown = shown.borrow();
        let Shown { layout, swaps } = shown.as_ref().expect("laid out");
        assert_eq!(swaps.len(), 1, "the formula typeset in place");
        let drawn = swaps[0].1.clone();
        let (start, end) = (
            layout.position_for_index(drawn.start),
            layout.position_for_index(drawn.end),
        );
        let (start, end) = (start.expect("its start"), end.expect("its end"));
        assert!(
            end.x - start.x > gpui::px(10.0),
            "glue holds the picture's width"
        );
        let middle = point(
            (start.x + end.x) / 2.0,
            start.y + layout.line_height() / 2.0,
        );
        assert_eq!(at(middle), 2, "a press on the picture is the formula");
    }
}
