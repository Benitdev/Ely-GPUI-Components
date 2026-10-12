mod edit;
mod parse;
mod place;
#[cfg(all(test, feature = "test-support"))]
mod tests;
mod visual;

pub(crate) use visual::bind_keys;

use std::{ops::Range, path::Path, rc::Rc, sync::Arc};

use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, ListAlignment,
    ListState, Subscription, Task, Window,
};

pub use parse::DocumentHeading;
use parse::{Parsed, alike, parsed};

use super::{MarkdownMode, markdown::markdown_highlights, modes::joint, render::OnLink};
use crate::{
    editor::{CodeEditor, EditorEvent, LineNumbers},
    forms::TextInput,
};

/// Texts this long parse at once; longer ones elsewhere.
const AT_ONCE: usize = 128 << 10;

/// What the document tells its owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocumentEvent {
    /// The text changed, typed in any mode or replaced.
    Changed,
    /// A new read of the text: blocks and headings.
    Parsed,
}

/// The block open as its source, in its own field.
struct Open {
    range: Range<usize>,
    input: Entity<TextInput>,
    /// Opened past the end, to add a block.
    append: bool,
    _subscriptions: Vec<Subscription>,
}

/// A Markdown file: one text, one history, three views.
pub struct MarkdownDocument {
    editor: Entity<CodeEditor>,
    mode: MarkdownMode,
    base: Option<Arc<Path>>,
    parsed: Arc<Parsed>,
    /// Text changes counted; `read` is the one parsed.
    edits: u64,
    read: u64,
    _parsing: Option<Task<()>>,
    list: ListState,
    open: Option<Open>,
    /// Its own write-backs whose changes are still to come.
    writes: usize,
    focus: FocusHandle,
    on_link: Option<OnLink>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DocumentEvent> for MarkdownDocument {}

impl Focusable for MarkdownDocument {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self.mode {
            MarkdownMode::Source | MarkdownMode::Split => self.editor.focus_handle(cx),
            MarkdownMode::Visual | MarkdownMode::Preview => self.focus.clone(),
        }
    }
}

impl MarkdownDocument {
    pub fn new(
        text: impl Into<String>,
        mode: MarkdownMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let editor = cx.new(|cx| {
            CodeEditor::new(text, window, cx)
                .language("Markdown")
                .line_numbers(LineNumbers::Hidden)
                .hide_indent_guides()
                .highlighter(markdown_highlights)
        });
        let subscriptions = vec![cx.subscribe(&editor, |document, _, event, cx| {
            if *event == EditorEvent::Changed {
                document.changed(cx);
            }
        })];
        let overdraw = window.rem_size() * 40.0;
        let mut document = Self {
            editor,
            mode,
            base: None,
            parsed: Arc::default(),
            edits: 0,
            read: u64::MAX,
            _parsing: None,
            list: ListState::new(1, ListAlignment::Top, overdraw),
            open: None,
            writes: 0,
            focus: cx.focus_handle(),
            on_link: None,
            _subscriptions: subscriptions,
        };
        document.reparse(cx);
        document
    }

    /// The folder relative pictures load from.
    pub fn set_base(&mut self, folder: impl Into<Arc<Path>>, cx: &mut Context<Self>) {
        self.base = Some(folder.into());
        cx.notify();
    }

    /// Takes link presses; without it the system opens them.
    pub fn on_link(&mut self, handler: impl Fn(&str, &mut Window, &mut App) + 'static) {
        self.on_link = Some(Rc::new(handler));
    }

    pub fn editor(&self) -> &Entity<CodeEditor> {
        &self.editor
    }

    pub fn mode(&self) -> MarkdownMode {
        self.mode
    }

    /// Shows `mode` at the same line; an open block writes.
    pub fn set_mode(&mut self, mode: MarkdownMode, window: &mut Window, cx: &mut Context<Self>) {
        self.commit(cx);
        log::info!("markdown document: {:?} to {mode:?}", self.mode);
        let top = self.top_line(cx);
        self.mode = mode;
        self.set_top_line(top, cx);
        let focus = self.focus_handle(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    /// The text now, an open block's typing included.
    pub fn text(&self, cx: &App) -> String {
        let text = self.editor.read(cx).text();
        let Some(open) = &self.open else {
            return text.to_string();
        };
        let typed = open.input.read(cx).text();
        let written = match open.append {
            true if typed.is_empty() => String::new(),
            true => format!("{}{typed}", joint(text)),
            false => typed.to_string(),
        };
        [&text[..open.range.start], &written, &text[open.range.end..]].concat()
    }

    /// Replaces the text in one step; open typing drops.
    pub fn set_text(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        self.drop_open(cx);
        self.editor
            .update(cx, |editor, cx| editor.set_text(text, cx));
    }

    /// Replaces the text, undo afresh; open typing drops.
    pub fn reset(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        self.drop_open(cx);
        self.editor.update(cx, |editor, cx| editor.reset(text, cx));
    }

    /// The headings as last read, with lines from zero.
    pub fn outline(&self, cx: &App) -> Vec<(DocumentHeading, usize)> {
        let editor = self.editor.read(cx);
        self.parsed
            .headings
            .iter()
            .filter(|heading| heading.at <= editor.text().len())
            .map(|heading| (heading.clone(), editor.buffer.line_of(heading.at)))
            .collect()
    }

    /// Whether the blocks shown read the text now.
    pub fn read_now(&self) -> bool {
        self.read == self.edits
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        // Only the document writes while a block is open.
        if self.writes > 0 {
            self.writes -= 1;
        } else if self.open.is_some() {
            log::error!("markdown document: the text changed under the open block");
            self.drop_open(cx);
        }
        self.edits += 1;
        cx.emit(DocumentEvent::Changed);
        self.reparse(cx);
    }

    /// Reads the text again, at once when short.
    fn reparse(&mut self, cx: &mut Context<Self>) {
        let (text, edits) = (self.editor.read(cx).text().to_string(), self.edits);
        if text.len() <= AT_ONCE {
            self._parsing = None;
            return self.adopt(parsed(&text), edits, cx);
        }
        self._parsing = Some(cx.spawn(async move |document, cx| {
            let read = cx.background_spawn(async move { parsed(&text) }).await;
            document
                .update(cx, |document, cx| document.adopt(read, edits, cx))
                .ok();
        }));
    }

    /// Takes the read of edit `edits`; older reads drop.
    fn adopt(&mut self, read: Parsed, edits: u64, cx: &mut Context<Self>) {
        if edits != self.edits {
            return log::debug!("markdown document: read {edits} lapsed");
        }
        let (head, tail) = alike(&self.parsed, &read);
        let old = self.parsed.blocks.len();
        self.list
            .splice(head..old - tail, read.blocks.len() - head - tail);
        self.parsed = Arc::new(read);
        self.read = edits;
        cx.emit(DocumentEvent::Parsed);
        cx.notify();
    }

    /// Closes the open block without writing it.
    fn drop_open(&mut self, cx: &mut Context<Self>) {
        if self.open.take().is_some() {
            log::info!("markdown document: the open block closed unwritten");
            cx.notify();
        }
    }
}
