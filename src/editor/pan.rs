//! Sideways scroll: code wider than the view pans under a still gutter.

use gpui::{Context, Pixels, ScrollWheelEvent, Window};

use super::state::CodeEditor;

/// Columns kept between the caret and the view's side edges.
const MARGIN: f32 = 2.0;

/// How far the code is panned, and what bounds it.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Pan {
    pub x: Pixels,
    /// The code's box, less the gutter, from its last paint.
    pub width: Pixels,
    /// The widest line in columns, and the text stamp it was counted in.
    widest: Option<(u64, usize)>,
    /// The caret and text stamp last panned into view.
    followed: Option<(usize, u64)>,
}

/// Columns `line` takes, a tab reaching its next stop.
fn columns(line: &str, tab: usize) -> usize {
    line.chars().fold(0, |at, ch| match ch {
        '\t' => (at / tab + 1) * tab,
        _ => at + 1,
    })
}

impl CodeEditor {
    /// A sideways wheel over a row pans the code; an upright one is the list's.
    pub(crate) fn wheel(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(self.frame.line);
        if delta.x.abs() <= delta.y.abs() {
            return;
        }
        cx.stop_propagation();
        let x = (self.pan.x - delta.x).clamp(Pixels::ZERO, self.pan_reach());
        if x != self.pan.x {
            self.pan.x = x;
            cx.notify();
        }
    }

    /// How far the code pans: its widest line past the view, with a column to spare.
    fn pan_reach(&self) -> Pixels {
        let widest = self.pan.widest.map_or(0, |(_, widest)| widest);
        (self.frame.advance * (widest + 2) as f32 - self.pan.width).max(Pixels::ZERO)
    }

    /// Before the rows draw: counts the widest line once per text, holds the pan within it, and pans a moved caret into view.
    pub(crate) fn settle_pan(&mut self, window: &Window, cx: &Context<Self>) {
        let stamp = self.buffer.stamp();
        if self.pan.widest.is_none_or(|(at, _)| at != stamp) {
            let tab = self.options.tab.max(1);
            let widest = (0..self.buffer.lines())
                .map(|line| columns(self.buffer.line(line), tab))
                .max()
                .unwrap_or(0);
            self.pan.widest = Some((stamp, widest));
        }
        self.pan.x = self.pan.x.min(self.pan_reach());
        let head = self.primary().head;
        if self.pan.followed == Some((head, stamp)) {
            return;
        }
        self.pan.followed = Some((head, stamp));
        let (advance, width) = (self.frame.advance, self.pan.width);
        if width <= Pixels::ZERO || head > self.buffer.len() {
            return;
        }
        let line = self.buffer.line_of(head);
        let text = self.row_text(line);
        let (shown, measured) = self.measured(line, window, cx);
        let column = shown.column(text, self.buffer.point(head).1.min(text.len()));
        let caret = advance + measured.x(column);
        let margin = advance * MARGIN;
        if caret - margin < self.pan.x {
            self.pan.x = (caret - margin).max(Pixels::ZERO);
        } else if caret + margin > self.pan.x + width {
            self.pan.x = caret + margin - width;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::columns;

    #[test]
    fn a_tab_reaches_its_next_stop() {
        assert_eq!(columns("ab", 4), 2);
        assert_eq!(columns("\tx", 4), 5);
        assert_eq!(columns("ab\tx", 4), 5);
        assert_eq!(columns("abcd\t", 4), 8);
    }
}
