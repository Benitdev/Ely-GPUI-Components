use std::{path::Path, rc::Rc, sync::Arc};

use gpui::{
    AnyElement, App, ElementId, Entity, FontStyle, FontWeight, HighlightStyle, InteractiveText,
    IntoElement, ParentElement, RenderOnce, SharedString, StrikethroughStyle, Styled, StyledText,
    UnderlineStyle, Window, div, prelude::*,
};
use markdown::mdast::AlignKind;

use super::{
    contents::anchor,
    diagram::Diagram,
    picture::Picture,
    tree::{Inline, Node, Style, parse},
};
use crate::{
    editor::{code_colors, code_colors_with},
    navigation::{Anchor, Sections},
    primitives::{FocusRing as _, Icon, IconName},
    theme::{ActiveTheme, IconSize, Radius, TextSize},
    typography::{Latex, failed},
};

pub(crate) type OnLink = Rc<dyn Fn(&str, &mut Window, &mut App)>;
type DrawCode = Rc<dyn Fn(ElementId, Option<SharedString>, SharedString) -> AnyElement>;

/// Markdown drawn as a document; links go to the host.
#[derive(IntoElement)]
pub struct MarkdownRenderer {
    id: ElementId,
    source: SharedString,
    on_link: Option<OnLink>,
    size: TextSize,
    sections: Option<Entity<Sections>>,
    code: Option<DrawCode>,
    base: Option<Arc<Path>>,
}

impl MarkdownRenderer {
    pub fn new(id: impl Into<ElementId>, source: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            source: source.into(),
            on_link: None,
            size: TextSize::Base,
            sections: None,
            code: None,
            base: None,
        }
    }

    /// The folder relative picture addresses start from.
    pub fn base(mut self, folder: impl Into<Arc<Path>>) -> Self {
        self.base = Some(folder.into());
        self
    }

    /// The body's size; headings keep theirs.
    pub fn text_size(mut self, size: TextSize) -> Self {
        self.size = size;
        self
    }

    /// Marks headings for `sections`; `outline` lists them.
    pub fn sections(mut self, sections: &Entity<Sections>) -> Self {
        self.sections = Some(sections.clone());
        self
    }

    /// Code blocks drawn by the owner: id, language, code.
    pub fn code(
        mut self,
        draw: impl Fn(ElementId, Option<SharedString>, SharedString) -> AnyElement + 'static,
    ) -> Self {
        self.code = Some(Rc::new(draw));
        self
    }

    /// Takes link presses instead of the browser.
    pub fn on_link(mut self, handler: impl Fn(&str, &mut Window, &mut App) + 'static) -> Self {
        self.on_link = Some(Rc::new(handler));
        self
    }
}

/// Draws nodes under one owner's ids and link handler.
struct Draw<'a> {
    id: &'a ElementId,
    on_link: &'a Option<OnLink>,
    next: std::cell::Cell<usize>,
    sections: &'a Option<Entity<Sections>>,
    headings: std::cell::Cell<usize>,
    code: &'a Option<DrawCode>,
    base: &'a Option<Arc<Path>>,
    /// Text size against the theme's.
    scale: f32,
}

impl Draw<'_> {
    fn key(&self, what: &str) -> ElementId {
        let at = self.next.get();
        self.next.set(at + 1);
        (self.id.clone(), format!("{what}-{at}")).into()
    }

    fn inline(&self, inline: &Inline, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let colors = &theme.colors;
        let rule = theme.underline_thickness();
        let styles: Vec<_> = inline
            .styles
            .iter()
            .map(|(range, style)| {
                let highlight = match style {
                    Style::Strong => HighlightStyle {
                        font_weight: Some(FontWeight::SEMIBOLD),
                        ..HighlightStyle::default()
                    },
                    Style::Emphasis => HighlightStyle {
                        font_style: Some(FontStyle::Italic),
                        ..HighlightStyle::default()
                    },
                    Style::Strike => HighlightStyle {
                        strikethrough: Some(StrikethroughStyle {
                            thickness: rule,
                            color: Some(colors.fg_muted),
                        }),
                        ..HighlightStyle::default()
                    },
                    Style::Code | Style::Math => HighlightStyle {
                        background_color: Some(colors.sunken),
                        color: Some(if *style == Style::Math {
                            colors.syntax.function
                        } else {
                            colors.fg
                        }),
                        ..HighlightStyle::default()
                    },
                    Style::Link => HighlightStyle {
                        color: Some(colors.link),
                        underline: Some(UnderlineStyle {
                            thickness: rule,
                            color: Some(colors.link.opacity(0.4)),
                            wavy: false,
                        }),
                        ..HighlightStyle::default()
                    },
                    Style::Footnote => HighlightStyle {
                        color: Some(colors.fg_subtle),
                        ..HighlightStyle::default()
                    },
                };
                (range.clone(), highlight)
            })
            .collect();
        let styled =
            StyledText::new(inline.text.clone()).with_highlights(crate::editor::stack(styles));
        if inline.links.is_empty() {
            return styled.into_any_element();
        }
        let (ranges, urls): (Vec<_>, Vec<_>) = inline.links.iter().cloned().unzip();
        let on_link = self.on_link.clone();
        InteractiveText::new(self.key("links"), styled)
            .on_click(ranges, move |ix, window, cx| {
                open_link(&on_link, &urls[ix], window, cx)
            })
            .into_any_element()
    }

    fn nodes(&self, nodes: &[Node], cx: &App) -> Vec<AnyElement> {
        nodes.iter().map(|node| self.node(node, cx)).collect()
    }

    fn node(&self, node: &Node, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let colors = theme.colors.clone();
        match node {
            Node::Paragraph(inline) => div()
                .text_color(colors.fg)
                .child(self.inline(inline, cx))
                .into_any_element(),
            Node::Heading(level, inline) => {
                let size = match level {
                    1 => TextSize::Xxl,
                    2 => TextSize::Xl,
                    3 => TextSize::Lg,
                    _ => TextSize::Md,
                };
                let heading = div()
                    .pt_2()
                    .text_size(theme.text_size(size) * self.scale)
                    .font_weight(if *level <= 2 {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::MEDIUM
                    })
                    .text_color(colors.fg)
                    .child(self.inline(inline, cx));
                let ordinal = self.headings.get();
                self.headings.set(ordinal + 1);
                match self.sections {
                    Some(sections) => Anchor::new(sections, anchor(ordinal))
                        .child(heading)
                        .into_any_element(),
                    None => heading.into_any_element(),
                }
            }
            Node::Quote(inner) => div()
                .flex()
                .flex_col()
                .gap_2()
                .pl_4()
                .border_l_2()
                .border_color(colors.border_strong)
                .text_color(colors.fg_muted)
                .children(self.nodes(inner, cx))
                .into_any_element(),
            Node::Code { language, text } => {
                if language.as_deref() == Some("mermaid") {
                    return Diagram::new(self.key("diagram"), text.clone()).into_any_element();
                }
                if let Some(draw) = self.code {
                    return draw(
                        self.key("code"),
                        language.clone().map(Into::into),
                        text.clone().into(),
                    );
                }
                // A named fence reads as its language; prose stays plain.
                let styles = match language.as_deref() {
                    Some(name) => crate::forms::syntax_fenced(name)
                        .map_or_else(Vec::new, |syntax| code_colors_with(text, syntax, cx)),
                    None => code_colors(text, cx),
                };
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .p_3()
                    .rounded(theme.radius(Radius::Md))
                    .bg(colors.sunken)
                    .font_family(theme.mono_family.clone())
                    .text_size(theme.text_size(TextSize::Sm) * self.scale)
                    .text_color(colors.syntax.variable)
                    .children(language.clone().map(|name| {
                        div()
                            .text_size(theme.text_size(TextSize::Xs) * self.scale)
                            .text_color(colors.fg_subtle)
                            .child(name)
                    }))
                    .child(StyledText::new(text.clone()).with_highlights(styles))
                    .into_any_element()
            }
            Node::List { start, items } => div()
                .flex()
                .flex_col()
                .gap_1()
                .children(items.iter().enumerate().map(|(ix, (task, body))| {
                    let marker: AnyElement = match (task, start) {
                        (Some(done), _) => Icon::new(if *done {
                            IconName::SquareCheck
                        } else {
                            IconName::Square
                        })
                        .size(IconSize::Sm)
                        .color(if *done {
                            colors.focus
                        } else {
                            colors.fg_subtle
                        })
                        .into_any_element(),
                        (None, Some(first)) => div()
                            .text_color(colors.fg_muted)
                            .child(format!("{}.", first + ix as u64))
                            .into_any_element(),
                        (None, None) => div()
                            .text_color(colors.fg_muted)
                            .child("•")
                            .into_any_element(),
                    };
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            div()
                                .flex_none()
                                .min_w_5()
                                .flex()
                                .justify_end()
                                .child(marker),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .when(*task == Some(true), |body| {
                                    body.text_color(colors.fg_subtle)
                                })
                                .children(self.nodes(body, cx)),
                        )
                }))
                .into_any_element(),
            Node::Table { aligns, head, rows } => {
                let cell = |blocks: &[Node], column: usize, header: bool, cx: &App| {
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .px_3()
                        .py_1p5()
                        .when(aligns.get(column) == Some(&AlignKind::Right), |cell| {
                            cell.justify_end()
                        })
                        .when(aligns.get(column) == Some(&AlignKind::Center), |cell| {
                            cell.justify_center()
                        })
                        .when(header, |cell| {
                            cell.font_weight(FontWeight::MEDIUM)
                                .text_color(colors.fg_muted)
                        })
                        .child(
                            div()
                                .max_w_full()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .children(self.nodes(blocks, cx)),
                        )
                };
                div()
                    .flex()
                    .flex_col()
                    .rounded(theme.radius(Radius::Md))
                    .border_1()
                    .border_color(colors.border)
                    .overflow_hidden()
                    .child(
                        div().flex().bg(colors.hover).children(
                            head.iter()
                                .enumerate()
                                .map(|(column, blocks)| cell(blocks, column, true, cx)),
                        ),
                    )
                    .children(rows.iter().map(|row| {
                        div()
                            .flex()
                            .border_t_1()
                            .border_color(colors.border)
                            .children(
                                row.iter()
                                    .enumerate()
                                    .map(|(column, blocks)| cell(blocks, column, false, cx)),
                            )
                    }))
                    .into_any_element()
            }
            Node::Rule => div()
                .my_2()
                .h_0()
                .border_t_1()
                .border_color(colors.border)
                .into_any_element(),
            Node::Math(tex) => div()
                .flex()
                .justify_center()
                .py_2()
                .child(match Latex::parse(self.key("math"), tex) {
                    Ok(math) => math.display().into_any_element(),
                    Err(error) => failed(&error, tex, cx),
                })
                .into_any_element(),
            Node::Image { alt, url, link } => {
                let picture = div().id(self.key("image")).child(Picture {
                    id: self.key("picture"),
                    alt: alt.clone(),
                    url: url.clone(),
                    base: self.base.clone(),
                });
                let Some(link) = link.clone() else {
                    return picture.into_any_element();
                };
                let on_link = self.on_link.clone();
                picture
                    .tab_index(0)
                    .focus_ring(cx)
                    .cursor_pointer()
                    .on_click(move |_, window, cx| open_link(&on_link, &link, window, cx))
                    .into_any_element()
            }
            Node::Footnote { label, body } => div()
                .flex()
                .gap_2()
                .text_size(theme.text_size(TextSize::Sm))
                .text_color(colors.fg_muted)
                .child(div().flex_none().child(format!("[{label}]")))
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .children(self.nodes(body, cx)),
                )
                .into_any_element(),
        }
    }
}

/// Opens `url` through the owner, else the system.
fn open_link(on_link: &Option<OnLink>, url: &str, window: &mut Window, cx: &mut App) {
    // The address is the document's: never logged.
    log::info!("markdown: a link opens");
    match on_link {
        Some(on_link) => on_link(url, window, cx),
        None => cx.open_url(url),
    }
}

/// Parsed `nodes` drawn under `id`; pictures read from `base`.
pub(crate) fn draw_nodes(
    id: &ElementId,
    nodes: &[Node],
    base: &Option<Arc<Path>>,
    on_link: &Option<OnLink>,
    scale: f32,
    cx: &App,
) -> Vec<AnyElement> {
    let draw = Draw {
        id,
        on_link,
        next: std::cell::Cell::new(0),
        sections: &None,
        headings: std::cell::Cell::new(0),
        code: &None,
        base,
        scale,
    };
    draw.nodes(nodes, cx)
}

impl RenderOnce for MarkdownRenderer {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let nodes = parse(&self.source);
        let draw = Draw {
            id: &self.id,
            on_link: &self.on_link,
            next: std::cell::Cell::new(0),
            sections: &self.sections,
            headings: std::cell::Cell::new(0),
            code: &self.code,
            base: &self.base,
            scale: 1.0,
        };
        div()
            .flex()
            .flex_col()
            .gap_3()
            .text_size(theme.text_size(self.size))
            .line_height(theme.text_size(self.size) * 1.6)
            .children(draw.nodes(&nodes, cx))
    }
}
