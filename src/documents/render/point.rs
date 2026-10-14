use std::rc::Rc;

use gpui::{
    AnyElement, App, InteractiveElement as _, IntoElement as _, MouseButton, MouseDownEvent,
    ParentElement as _, TextLayout, Window, div,
};

use super::Draw;
use crate::documents::tree::Inline;

/// Where a press fell, as the drawing knows it.
pub(crate) enum Place {
    /// A drawn byte, read back through its text's sources.
    Text(Rc<Inline>, usize),
    /// A source byte.
    At(usize),
    /// Somewhere the block holds no byte for.
    Block,
}

/// A press on drawn content.
pub(crate) struct Pointed {
    pub place: Place,
    pub link: Option<String>,
    pub image: Option<String>,
    pub code: bool,
    pub table: bool,
}

impl Pointed {
    /// A press past any drawn text.
    pub(crate) fn block() -> Self {
        Self::at(Place::Block)
    }

    fn at(place: Place) -> Self {
        Self {
            place,
            link: None,
            image: None,
            code: false,
            table: false,
        }
    }
}

/// Takes a press's button and what it fell on.
pub(crate) type OnPoint = Rc<dyn Fn(Pointed, MouseButton, &mut Window, &mut App)>;

impl Draw<'_> {
    /// `element` hands its left and right presses to the owner.
    fn pointable(
        &self,
        element: AnyElement,
        point: impl Fn(&MouseDownEvent) -> Pointed + 'static,
    ) -> AnyElement {
        let Some(on_point) = self.hooks.on_point.clone() else {
            return element;
        };
        let (point, table) = (Rc::new(point), self.table.get());
        let press = move |button: MouseButton| {
            let (on_point, point) = (on_point.clone(), point.clone());
            move |event: &MouseDownEvent, window: &mut Window, cx: &mut App| {
                // Cmd or Ctrl leaves the press to a link.
                if button == MouseButton::Left && event.modifiers.secondary() {
                    return;
                }
                window.prevent_default();
                cx.stop_propagation();
                let mut pointed = point(event);
                pointed.table |= table;
                on_point(pointed, button, window, cx);
            }
        };
        div()
            .on_mouse_down(MouseButton::Left, press(MouseButton::Left))
            .on_mouse_down(MouseButton::Right, press(MouseButton::Right))
            .child(element)
            .into_any_element()
    }

    /// Drawn `inline`; a press finds its byte through `layout`.
    pub(super) fn point_text(
        &self,
        element: AnyElement,
        layout: TextLayout,
        inline: &Inline,
    ) -> AnyElement {
        if self.hooks.on_point.is_none() {
            return element;
        }
        let inline = Rc::new(inline.clone());
        self.pointable(element, move |event| {
            let at = layout
                .index_for_position(event.position)
                .unwrap_or_else(|near| near);
            let link = inline.links.iter().find(|(range, _)| range.contains(&at));
            Pointed {
                link: link.map(|(_, url)| url.clone()),
                ..Pointed::at(Place::Text(inline.clone(), at))
            }
        })
    }

    /// A code block; its text starts at source byte `from`.
    pub(super) fn point_code(
        &self,
        element: AnyElement,
        layout: Option<TextLayout>,
        from: Option<usize>,
    ) -> AnyElement {
        self.pointable(element, move |event| {
            let place = match (&layout, from) {
                (Some(layout), Some(from)) => Place::At(
                    from + layout
                        .index_for_position(event.position)
                        .unwrap_or_else(|near| near),
                ),
                _ => Place::Block,
            };
            Pointed {
                code: true,
                ..Pointed::at(place)
            }
        })
    }

    /// A picture written at source byte `at`.
    pub(super) fn point_image(
        &self,
        element: AnyElement,
        at: usize,
        url: &str,
        link: &Option<String>,
    ) -> AnyElement {
        let (url, link) = (url.to_string(), link.clone());
        self.pointable(element, move |_| Pointed {
            image: Some(url.clone()),
            link: link.clone(),
            ..Pointed::at(Place::At(at))
        })
    }
}
