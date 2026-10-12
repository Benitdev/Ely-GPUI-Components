use gpui::{
    AnyElement, App, ClickEvent, Context, FontWeight, InteractiveElement, IntoElement, KeyBinding,
    MouseButton, MouseDownEvent, ParentElement, Render, StatefulInteractiveElement, Styled, Window,
    actions, div, list,
};

use super::MarkdownDocument;
use crate::{
    documents::{MarkdownMode, render::draw_nodes},
    forms::{Down, Up},
    primitives::FocusRing as _,
    theme::{ActiveTheme, Radius, TextSize},
};

actions!(markdown_document, [Undo, Redo]);

const CONTEXT: &str = "MarkdownDocument";

/// Undo and redo while no block is open.
pub(crate) fn bind_keys(cx: &mut App) {
    let context = Some(CONTEXT);
    let mut bindings = vec![
        KeyBinding::new("secondary-z", Undo, context),
        KeyBinding::new("secondary-shift-z", Redo, context),
    ];
    if !cfg!(target_os = "macos") {
        bindings.push(KeyBinding::new("ctrl-y", Redo, context));
    }
    cx.bind_keys(bindings);
}

impl MarkdownDocument {
    /// List item `ix`: a block, or the end.
    fn item(&mut self, ix: usize, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let colors = theme.colors.clone();
        let (radius, body, mono) = (
            theme.radius(Radius::Md),
            theme.text_size(TextSize::Base),
            theme.mono_family.clone(),
        );
        let editing = self.mode == MarkdownMode::Visual;
        let open = self.open.as_ref();
        let field = |open: &super::Open| {
            div()
                .px_3()
                .py_2()
                .rounded(radius)
                .bg(colors.sunken)
                .child(open.input.clone())
                .into_any_element()
        };
        let Some(block) = self.parsed.blocks.get(ix) else {
            return match (open, editing) {
                (Some(open), _) if open.append => field(open),
                (_, true) => self
                    .pressable(div().id("more").min_h_8(), None, cx)
                    .into_any_element(),
                (_, false) => div().into_any_element(),
            };
        };
        if let Some(open) = open.filter(|open| open.range == block.range) {
            return field(open);
        }
        let shown = match &block.nodes {
            Some(nodes) => div()
                .flex()
                .flex_col()
                .gap_3()
                .text_size(body)
                .line_height(body * 1.6)
                .text_color(colors.fg)
                .children(draw_nodes(
                    &("render", ix).into(),
                    nodes,
                    &self.base,
                    &self.on_link,
                    cx,
                )),
            None => div()
                .font_family(mono)
                .text_color(colors.fg_muted)
                .child(self.parsed.text[block.range.clone()].to_string()),
        };
        let block = div()
            .id(("block", ix))
            .debug_selector(move || format!("markdown-block-{ix}"))
            .px_3()
            .py_1()
            .rounded(radius)
            .child(shown);
        match editing {
            true => self.pressable(block, Some(ix), cx).into_any_element(),
            false => block.into_any_element(),
        }
    }

    /// `element` opens block `ix` on a press, Enter or Space.
    fn pressable(
        &self,
        element: gpui::Stateful<gpui::Div>,
        ix: Option<usize>,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let hover = cx.theme().colors.hover.opacity(0.5);
        element
            .tab_index(0)
            .border_1()
            .border_color(gpui::transparent_black())
            .focus_ring(cx)
            .cursor_text()
            .hover(move |element| element.bg(hover))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |document, event: &MouseDownEvent, window, cx| {
                    // Cmd or Ctrl leaves the press to a link.
                    if event.modifiers.secondary() {
                        return;
                    }
                    window.prevent_default();
                    cx.stop_propagation();
                    document.open(ix, false, window, cx);
                }),
            )
            .on_click(
                cx.listener(move |document, event: &ClickEvent, window, cx| {
                    if event.is_keyboard() {
                        document.open(ix, false, window, cx);
                    }
                }),
            )
    }
}

impl Render for MarkdownDocument {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let root = div()
            .size_full()
            .bg(colors.surface)
            .font_weight(FontWeight::NORMAL);
        let blocks = list(self.list.clone(), cx.processor(Self::item)).size_full();
        match self.mode {
            MarkdownMode::Source => root.child(self.editor.clone()).into_any_element(),
            MarkdownMode::Split => root
                .flex()
                .child(div().flex_1().min_w_0().h_full().child(self.editor.clone()))
                .child(div().flex_1().min_w_0().h_full().child(blocks))
                .into_any_element(),
            MarkdownMode::Preview => root
                .track_focus(&self.focus)
                .px_2()
                .py_3()
                .child(blocks)
                .into_any_element(),
            MarkdownMode::Visual => root
                .key_context(CONTEXT)
                .track_focus(&self.focus)
                .on_action(cx.listener(|document, _: &Undo, _, cx| {
                    document.commit(cx);
                    document.editor.update(cx, |editor, cx| editor.undo(cx));
                }))
                .on_action(cx.listener(|document, _: &Redo, _, cx| {
                    document.commit(cx);
                    document.editor.update(cx, |editor, cx| editor.redo(cx));
                }))
                .capture_action(cx.listener(|document, _: &Up, window, cx| {
                    if document.step(false, window, cx) {
                        cx.stop_propagation();
                    }
                }))
                .capture_action(cx.listener(|document, _: &Down, window, cx| {
                    if document.step(true, window, cx) {
                        cx.stop_propagation();
                    }
                }))
                .on_key_down(
                    cx.listener(|document, event: &gpui::KeyDownEvent, window, cx| {
                        if event.keystroke.key == "escape" && document.open.is_some() {
                            cx.stop_propagation();
                            document.commit(cx);
                            window.focus(&document.focus, cx);
                        }
                    }),
                )
                .px_2()
                .py_3()
                .child(blocks)
                .into_any_element(),
        }
    }
}
