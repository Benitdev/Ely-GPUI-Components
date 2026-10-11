use gpui::{
    Context, IntoElement, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ParentElement, Point, Render, TestAppContext, Window, div, point, prelude::*, px,
};

use super::TitleBar;
use crate::theme::{Platform, Theme};

struct Bar {
    closes: usize,
}

impl Render for Bar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        div().w(px(600.0)).child(
            TitleBar::new("bar")
                .platform(Platform::Linux)
                .on_close(move |_, cx| view.update(cx, |bar, _| bar.closes += 1)),
        )
    }
}

/// Presses at `at`, moves two pixels, releases.
fn wobble(at: Point<gpui::Pixels>, cx: &mut gpui::VisualTestContext) {
    let moved = at + point(px(2.0), px(1.0));
    cx.simulate_event(MouseDownEvent {
        button: MouseButton::Left,
        position: at,
        modifiers: Modifiers::none(),
        click_count: 1,
        first_mouse: false,
    });
    cx.simulate_event(MouseMoveEvent {
        position: moved,
        pressed_button: Some(MouseButton::Left),
        modifiers: Modifiers::none(),
    });
    cx.simulate_event(MouseUpEvent {
        button: MouseButton::Left,
        position: moved,
        modifiers: Modifiers::none(),
        click_count: 1,
    });
    cx.run_until_parked();
}

/// A press that wobbles on Linux's close button still closes; it never moves the window.
#[gpui::test]
fn a_wobbling_press_on_a_linux_button_clicks_it(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let (view, cx) = cx.add_window_view(|_, _| Bar { closes: 0 });
    cx.run_until_parked();
    let controls = cx
        .debug_bounds("titlebar-controls")
        .expect("drawn controls");
    // Close is the last of the round buttons.
    wobble(point(controls.right() - px(8.0), controls.center().y), cx);
    assert_eq!(view.read_with(cx, |bar, _| bar.closes), 1);
}

/// The bar's empty space still moves the window (the test window panics on a move).
#[gpui::test]
#[should_panic(expected = "not implemented")]
fn a_wobbling_press_on_the_bar_moves_the_window(cx: &mut TestAppContext) {
    cx.update(Theme::init);
    let (_, cx) = cx.add_window_view(|_, _| Bar { closes: 0 });
    cx.run_until_parked();
    let controls = cx
        .debug_bounds("titlebar-controls")
        .expect("drawn controls");
    wobble(point(controls.left() - px(200.0), controls.center().y), cx);
}
