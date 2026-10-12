use std::{
    any::TypeId,
    collections::HashMap,
    hash::{DefaultHasher, Hash, Hasher},
    sync::Arc,
};

use gpui::{App, Asset, ElementId, Global, RenderImage, Window};

/// Holders of each asset, keyed as gpui's cache keys it.
#[derive(Default)]
struct Holders(HashMap<(TypeId, u64), usize>);

impl Global for Holders {}

fn key<A: Asset>(source: &A::Source) -> (TypeId, u64) {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    (TypeId::of::<A>(), hasher.finish())
}

/// An element's asset; the last holder to leave frees it.
pub(crate) struct Held<A: Asset> {
    source: Option<A::Source>,
    image: Option<Arc<RenderImage>>,
}

impl<A: Asset> Held<A> {
    fn take(&mut self, source: &A::Source, cx: &mut App) {
        let holders = &mut cx.default_global::<Holders>().0;
        *holders.entry(key::<A>(source)).or_default() += 1;
        self.source = Some(source.clone());
    }

    fn free(&mut self, cx: &mut App) {
        let Some(source) = self.source.take() else {
            return;
        };
        let image = self.image.take();
        let holders = &mut cx.default_global::<Holders>().0;
        let key = key::<A>(&source);
        let count = holders.get_mut(&key).expect("a held asset is counted");
        *count -= 1;
        if *count > 0 {
            return;
        }
        holders.remove(&key);
        cx.remove_asset::<A>(&source);
        if let Some(image) = image {
            // The window may be drawing; free after it.
            cx.defer(move |cx| cx.drop_image(image, None));
        }
    }
}

/// `source` as `A` for element `id`; the previous one let go.
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
            held.take(source, cx);
        }
        if let Some(loaded) = &loaded {
            held.image = image(loaded);
        }
    });
    loaded
}
