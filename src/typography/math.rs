use std::{
    fmt,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::Arc,
};

use gpui::{
    App, Asset, IntoElement, ParentElement, Pixels, RenderImage, RenderOnce, SharedString, Size,
    Styled, Window, div, img, px, size,
};
use ratex_layout::{LayoutOptions, layout, to_display_list};
use ratex_svg::{SvgOptions, render_to_svg};
use ratex_types::{color::Color, math_style::MathStyle};

use crate::theme::{ActiveTheme, TextSize};

/// Why a formula did not typeset: RaTeX's own words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MathError(SharedString);

impl fmt::Display for MathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for MathError {}

/// TeX typeset by RaTeX, in the text's color.
#[derive(IntoElement)]
pub struct Latex {
    source: SharedString,
    size: TextSize,
    display: bool,
}

impl Latex {
    pub fn parse(source: &str) -> Result<Self, MathError> {
        ratex_parser::parse(source).map_err(|error| MathError(error.message.into()))?;
        Ok(Self {
            source: source.to_string().into(),
            size: TextSize::Lg,
            display: false,
        })
    }

    pub fn size(mut self, size: TextSize) -> Self {
        self.size = size;
        self
    }

    /// Display style: limits above and below, larger operators.
    pub fn display(mut self) -> Self {
        self.display = true;
        self
    }
}

/// What a typeset picture depends on.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct Formula {
    source: SharedString,
    display: bool,
    rgba: u32,
    /// Font size and window scale, in hundredths.
    font: u32,
    scale: u32,
}

/// A typeset formula: its picture and logical size.
type Typeset = Result<(Arc<RenderImage>, Size<Pixels>), MathError>;

enum Typesetter {}

impl Asset for Typesetter {
    type Source = Formula;
    type Output = Typeset;

    fn load(formula: Formula, cx: &mut App) -> impl Future<Output = Typeset> + Send + 'static {
        let renderer = cx.svg_renderer();
        async move {
            let (svg, shape) = svg(&formula)?;
            let picture = renderer
                .render_single_frame(svg.as_bytes(), formula.scale as f32 / 100.0)
                .map_err(|error| MathError(format!("{error}").into()))?;
            Ok((picture, shape))
        }
    }
}

/// The formula as SVG, glyphs outlined, and its size.
fn svg(formula: &Formula) -> Result<(String, Size<Pixels>), MathError> {
    let ast =
        ratex_parser::parse(&formula.source).map_err(|error| MathError(error.message.into()))?;
    let [r, g, b, a] = formula
        .rgba
        .to_be_bytes()
        .map(|channel| f32::from(channel) / 255.0);
    let options = LayoutOptions::default()
        .with_style(match formula.display {
            true => MathStyle::Display,
            false => MathStyle::Text,
        })
        .with_color(Color::new(r, g, b, a));
    let font = f64::from(formula.font) / 100.0;
    // RaTeX may panic on input it parsed.
    let laid = catch_unwind(AssertUnwindSafe(|| {
        let list = to_display_list(&layout(&ast, &options));
        let svg = render_to_svg(
            &list,
            &SvgOptions {
                font_size: font,
                padding: 0.0,
                stroke_width: (font / 26.0).max(0.5),
                embed_glyphs: true,
                font_dir: String::new(),
            },
        );
        (list.width, list.height + list.depth, svg)
    }));
    let (width, height, svg) = laid.map_err(|_| {
        log::error!("latex: RaTeX failed to lay out {:?}", formula.source);
        MathError("RaTeX could not lay out this formula".into())
    })?;
    let shape = size(px((width * font) as f32), px((height * font) as f32));
    Ok((svg, shape))
}

impl RenderOnce for Latex {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (color, danger) = (theme.colors.fg, theme.colors.danger);
        let font = theme.text_size(self.size).to_pixels(window.rem_size());
        let formula = Formula {
            source: self.source.clone(),
            display: self.display,
            rgba: u32::from(color.to_rgb()),
            font: (font.as_f32() * 100.0).round() as u32,
            scale: (window.scale_factor() * 100.0).round() as u32,
        };
        match window.use_asset::<Typesetter>(&formula, cx) {
            Some(Ok((picture, shape))) => img(picture)
                .w(shape.width)
                .h(shape.height)
                .into_any_element(),
            Some(Err(error)) => div()
                .text_color(danger)
                .child(error.to_string())
                .into_any_element(),
            None => div().h(font).into_any_element(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn formula(source: &str, display: bool) -> Formula {
        Formula {
            source: source.to_string().into(),
            display,
            rgba: 0x112233ff,
            font: 1600,
            scale: 200,
        }
    }

    #[test]
    fn a_formula_typesets_to_outlined_svg_in_its_color() {
        let (svg, shape) = svg(&formula(r"\frac{a}{b} + \sqrt{x}", false)).expect("typesets");
        assert!(svg.starts_with("<svg") && !svg.contains("<text"), "{svg}");
        assert!(
            svg.contains("17,34,51") || svg.contains("#112233"),
            "the color: {svg}"
        );
        assert!(shape.width > px(16.) && shape.height > px(16.), "{shape:?}");
    }

    #[test]
    fn display_style_sets_limits_larger() {
        let (_, text) = svg(&formula(r"\sum_{k=1}^{n} k", false)).expect("typesets");
        let (_, display) = svg(&formula(r"\sum_{k=1}^{n} k", true)).expect("typesets");
        assert!(display.height > text.height, "{display:?} over {text:?}");
    }

    #[test]
    fn bad_tex_says_why() {
        let Err(error) = Latex::parse(r"\frac{a}") else {
            panic!("a missing argument parses")
        };
        assert!(!error.to_string().is_empty());
        assert!(Latex::parse(r"x^{2").is_err(), "an open brace");
    }
}
