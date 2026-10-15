//! Hover intent: open after a pause, hold while aiming at a submenu.

use std::time::Duration;

use gpui::{App, Bounds, Entity, Pixels, Point};

use super::{
    menu::{Open, mark},
    model::Menu,
};

/// A submenu row opens after the pointer rests this long.
pub(super) const OPEN: Duration = Duration::from_millis(150);
/// A row crossed toward an open submenu waits this long.
pub(super) const HOLD: Duration = Duration::from_millis(300);

/// The pointer moved over row `ix` at `depth`, a submenu row when `sub`.
pub(super) fn hover(
    state: &Entity<Open>,
    menu: &Menu,
    (depth, ix): (usize, usize),
    sub: bool,
    at: Point<Pixels>,
    cx: &mut App,
) {
    let seen = state.update(cx, |open, _| {
        let from = open.pointer.replace(at);
        open.over = Some((depth, ix));
        let level = open.levels.get(depth)?;
        let shown = (open.levels.get(depth + 1)).map(|level| level.panel);
        Some((from, level.at, shown))
    });
    let Some((from, marked, shown)) = seen else {
        return;
    };
    if marked == Some(ix) {
        if !sub || shown.is_some() {
            state.update(cx, |open, _| open.pending = None);
        } else if state.read(cx).pending.is_none() {
            later(state, menu, (depth, ix), OPEN, cx);
        }
        return;
    }
    let aimed = from
        .zip(shown.filter(|panel| *panel != Bounds::default()))
        .is_some_and(|(from, panel)| toward(from, at, panel));
    if aimed {
        log::debug!("menu: row {ix} of level {depth} waits on the aim");
        return later(state, menu, (depth, ix), HOLD, cx);
    }
    state.update(cx, |open, _| open.pending = None);
    mark(state, menu, depth, (ix, 0), false, cx);
    if sub {
        later(state, menu, (depth, ix), OPEN, cx);
    }
}

/// Marks row `ix` after `wait`, its submenu open, if still under the pointer.
fn later(
    state: &Entity<Open>,
    menu: &Menu,
    (depth, ix): (usize, usize),
    wait: Duration,
    cx: &mut App,
) {
    let (state_, menu) = (state.clone(), menu.clone());
    let task = cx.spawn(async move |cx| {
        cx.background_executor().timer(wait).await;
        cx.update(|cx| {
            if state_.read(cx).over != Some((depth, ix)) {
                return log::debug!("menu: the pointer left row {ix} before it opened");
            }
            state_.update(cx, |open, _| open.pending = None);
            mark(&state_, &menu, depth, (ix, 0), true, cx);
        });
    });
    state.update(cx, |open, _| open.pending = Some(task));
}

/// Whether `to` lies in the triangle from `from` to `panel`'s near edge.
pub(super) fn toward(from: Point<Pixels>, to: Point<Pixels>, panel: Bounds<Pixels>) -> bool {
    let near = if panel.left() >= from.x {
        panel.left()
    } else {
        panel.right()
    };
    let (top, bottom) = (
        Point::new(near, panel.top()),
        Point::new(near, panel.bottom()),
    );
    let side = |a: Point<Pixels>, b: Point<Pixels>| {
        f32::from(b.x - a.x) * f32::from(to.y - a.y) - f32::from(b.y - a.y) * f32::from(to.x - a.x)
    };
    let sides = [side(from, top), side(top, bottom), side(bottom, from)];
    sides.iter().all(|side| *side >= 0.0) || sides.iter().all(|side| *side <= 0.0)
}

#[cfg(test)]
mod tests {
    use gpui::{Bounds, point, px, size};

    use super::toward;

    #[test]
    fn the_aim_holds_toward_the_panel_on_either_side() {
        let right = Bounds::new(point(px(200.), px(0.)), size(px(100.), px(200.)));
        let from = point(px(100.), px(100.));
        assert!(toward(from, point(px(110.), px(98.)), right));
        assert!(
            !toward(from, point(px(100.), px(130.)), right),
            "straight down"
        );
        let left = Bounds::new(point(px(0.), px(0.)), size(px(50.), px(200.)));
        assert!(toward(from, point(px(90.), px(102.)), left));
        assert!(
            !toward(from, point(px(110.), px(100.)), left),
            "away from it"
        );
    }
}
