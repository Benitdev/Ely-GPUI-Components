//! Where a drawn line's columns fall, as gpui lays it out.

use std::sync::Arc;

use gpui::{Hsla, LineLayout, Pixels, SharedString, TextRun, Window, font};

use super::layout::Shown;

/// A drawn line laid out: wide characters take their own width.
pub(crate) struct Measured {
    text: SharedString,
    layout: Arc<LineLayout>,
    advance: Pixels,
}

impl Measured {
    /// `shown` in the code font `family` at `size`.
    pub fn new(
        shown: &Shown,
        family: SharedString,
        size: Pixels,
        advance: Pixels,
        window: &Window,
    ) -> Self {
        let base = font(family);
        let run = |len: usize, style: Option<&gpui::HighlightStyle>| {
            let mut font = base.clone();
            if let Some(style) = style {
                font.weight = style.font_weight.unwrap_or(font.weight);
                font.style = style.font_style.unwrap_or(font.style);
            }
            TextRun {
                len,
                font,
                color: Hsla::default(),
                background_color: None,
                underline: None,
                strikethrough: None,
            }
        };
        let (text, mut runs, mut at) = (shown.text.clone(), Vec::new(), 0);
        for (range, style) in shown.styles.iter().filter(|(range, _)| !range.is_empty()) {
            if range.start > at {
                runs.push(run(range.start - at, None));
            }
            runs.push(run(range.len(), Some(style)));
            at = range.end;
        }
        if at < text.len() {
            runs.push(run(text.len() - at, None));
        }
        let layout = window.text_system().layout_line(&text, size, &runs, None);
        Self {
            text,
            layout,
            advance,
        }
    }

    /// Where drawn column `column` starts; past the end in whole advances.
    pub fn x(&self, column: usize) -> Pixels {
        match self.text.char_indices().nth(column) {
            Some((at, _)) => self.layout.x_for_index(at),
            None => {
                let past = column - self.text.chars().count();
                self.layout.width + self.advance * past as f32
            }
        }
    }

    /// The drawn column whose start is nearest `x`.
    pub fn column(&self, x: Pixels) -> usize {
        let at = self.layout.closest_index_for_x(x).min(self.text.len());
        self.text[..at].chars().count()
    }
}
