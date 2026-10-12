use std::sync::Arc;

use gpui::{App, Asset, ElementId, RenderImage, Window};

/// An element's asset; freed on change or when it leaves.
pub(crate) struct Held<A: Asset> {
    source: Option<A::Source>,
    image: Option<Arc<RenderImage>>,
}

impl<A: Asset> Held<A> {
    fn free(&mut self, cx: &mut App) {
        if let Some(source) = self.source.take() {
            cx.remove_asset::<A>(&source);
        }
        if let Some(image) = self.image.take() {
            // The window may be drawing; free after it.
            cx.defer(move |cx| cx.drop_image(image, None));
        }
    }
}

/// `source` as `A` for element `id`; the previous one freed.
pub(crate) fn hold<A: Asset>(
    id: impl Into<ElementId>,
    source: &A::Source,
    image: impl Fn(&A::Output) -> Option<Arc<RenderImage>>,
    window: &mut Window,
    cx: &mut App,
) -> Option<A::Output>
where
    A::Source: PartialEq,
{
    let held = window.use_keyed_state(id, cx, |_, cx| {
        cx.on_release(|held: &mut Held<A>, cx| held.free(cx))
            .detach();
        Held {
            source: None,
            image: None,
        }
    });
    let loaded = window.use_asset::<A>(source, cx);
    held.update(cx, |held, cx| {
        if held.source.as_ref() != Some(source) {
            held.free(cx);
            held.source = Some(source.clone());
        }
        if let Some(loaded) = &loaded {
            held.image = image(loaded);
        }
    });
    loaded
}
