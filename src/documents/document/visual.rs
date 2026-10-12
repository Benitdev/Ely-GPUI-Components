use gpui::{
    AnyElement, App, AppContext as _, Context, Focusable as _, FontWeight, InteractiveElement,
    IntoElement, KeyBinding, MouseButton, ParentElement, Render, Styled, Window, actions, div,
    list,
};

use super::{DocumentEvent, MarkdownDocument, Open};
use crate::{
    documents::{
        MarkdownMode,
        markdown::markdown_highlights,
        modes::{at_edge, joint},
        render::draw_nodes,
    },
    forms::{Down, InputEvent, TextInput, Up},
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
    /// Opens block `ix`, or a new one past the end.
    pub(super) fn open(
        &mut self,
        ix: Option<usize>,
        at_start: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.commit(cx);
        if !self.read_now() {
            return log::warn!("markdown document: pressed while the text is read again");
        }
        let text = self.editor.read(cx).text();
        let (range, append) = match ix {
            Some(ix) => (self.parsed.blocks[ix].range.clone(), false),
            None => (text.len()..text.len(), true),
        };
        let source = text[range.clone()].to_string();
        let input = cx.new(|cx| {
            TextInput::new(window, cx)
                .multi_line(1, usize::MAX)
                .highlighter(markdown_highlights)
        });
        input.update(cx, |input, cx| {
            input.set_text(source, cx);
            if at_start {
                input.select(0..0, cx);
            }
        });
        window.focus(&input.focus_handle(cx), cx);
        let subscriptions =
            vec![
                cx.subscribe(&input, |document, _, event: &InputEvent, cx| match event {
                    InputEvent::Blur => document.commit(cx),
                    InputEvent::Changed => cx.emit(DocumentEvent::Changed),
                    _ => {}
                }),
            ];
        log::info!("markdown document: block {range:?} opens");
        self.open = Some(Open {
            range,
            input,
            append,
            _subscriptions: subscriptions,
        });
        cx.notify();
    }

    /// Writes the open block back as one undo step.
    pub(super) fn commit(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.open.take() else {
            return;
        };
        cx.notify();
        let typed = open.input.read(cx).text().to_string();
        let text = self.editor.read(cx).text();
        let written = match open.append {
            true if typed.is_empty() => return,
            true => format!("{}{typed}", joint(text)),
            false => typed,
        };
        self.rest = open.range.start + written.len();
        if text[open.range.clone()] == written {
            return;
        }
        log::info!("markdown document: block {:?} written back", open.range);
        self.editor
            .update(cx, |editor, cx| editor.edit([(open.range, written)], cx));
    }

    /// Up past the open block's top, or down past it.
    fn step(&mut self, down: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(open) = &self.open else {
            return false;
        };
        if open.append || !at_edge(open.input.read(cx), down) {
            return false;
        }
        let start = open.range.start;
        let here = |blocks: &[super::parse::Block]| {
            blocks.partition_point(|block| block.range.start <= start)
        };
        // The block next door, before writing back.
        let next = |blocks: &[super::parse::Block]| match down {
            true => (here(blocks) < blocks.len()).then(|| here(blocks)),
            false => here(blocks).checked_sub(2),
        };
        if next(&self.parsed.blocks).is_none() {
            return false;
        }
        self.commit(cx);
        let Some(target) = next(&self.parsed.blocks) else {
            return false;
        };
        self.open(Some(target), down, window, cx);
        true
    }

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
        let Some(block) = self.parsed.blocks.get(ix) else {
            return match (&self.open, editing) {
                (Some(open), _) if open.append => div()
                    .px_3()
                    .py_2()
                    .rounded(radius)
                    .bg(colors.sunken)
                    .child(open.input.clone())
                    .into_any_element(),
                (_, true) => div()
                    .id("more")
                    .min_h_8()
                    .cursor_text()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|document, _, window, cx| {
                            window.prevent_default();
                            document.open(None, false, window, cx);
                        }),
                    )
                    .into_any_element(),
                (_, false) => div().into_any_element(),
            };
        };
        if let Some(open) = self.open.as_ref().filter(|open| open.range == block.range) {
            return div()
                .px_3()
                .py_2()
                .rounded(radius)
                .bg(colors.sunken)
                .child(open.input.clone())
                .into_any_element();
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
        if !editing {
            return block.into_any_element();
        }
        block
            .cursor_text()
            .hover(|block| block.bg(colors.hover.opacity(0.5)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |document, event: &gpui::MouseDownEvent, window, cx| {
                    // Cmd or Ctrl leaves the press to a link.
                    if event.modifiers.secondary() {
                        return;
                    }
                    window.prevent_default();
                    cx.stop_propagation();
                    document.open(Some(ix), false, window, cx);
                }),
            )
            .into_any_element()
    }

    fn blocks(&self, cx: &mut Context<Self>) -> AnyElement {
        list(self.list.clone(), cx.processor(Self::item))
            .size_full()
            .into_any_element()
    }
}

impl Render for MarkdownDocument {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let root = div()
            .size_full()
            .bg(colors.surface)
            .font_weight(FontWeight::NORMAL);
        match self.mode {
            MarkdownMode::Source => root.child(self.editor.clone()).into_any_element(),
            MarkdownMode::Split => root
                .flex()
                .child(div().flex_1().min_w_0().h_full().child(self.editor.clone()))
                .child(div().flex_1().min_w_0().h_full().child(self.blocks(cx)))
                .into_any_element(),
            MarkdownMode::Visual | MarkdownMode::Preview => root
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
                .child(self.blocks(cx))
                .into_any_element(),
        }
    }
}
