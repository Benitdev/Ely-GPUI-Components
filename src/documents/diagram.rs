use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::Arc,
};

use gpui::{
    App, Asset, Hsla, IntoElement, ParentElement, Pixels, RenderImage, RenderOnce, SharedString,
    Size, Styled, Window, div, img, px, size,
};
use mermaid_rs_renderer::{LayoutConfig, RenderOptions, Theme, render_strict};

use crate::theme::{ActiveTheme, Radius, TextSize};

/// A Mermaid diagram; a failure shows why and the source.
#[derive(IntoElement)]
pub struct Diagram {
    source: SharedString,
}

impl Diagram {
    pub fn new(source: impl Into<SharedString>) -> Self {
        Self {
            source: source.into(),
        }
    }
}

/// What a drawn diagram depends on.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct Chart {
    source: SharedString,
    dark: bool,
    /// Background and text, as `#rrggbb`.
    ground: SharedString,
    ink: SharedString,
    /// Window scale, in hundredths.
    scale: u32,
}

type Drawn = Result<(Arc<RenderImage>, Size<Pixels>), SharedString>;

enum Drawer {}

impl Asset for Drawer {
    type Source = Chart;
    type Output = Drawn;

    fn load(chart: Chart, cx: &mut App) -> impl Future<Output = Drawn> + Send + 'static {
        let renderer = cx.svg_renderer();
        async move {
            let svg = svg(&chart)?;
            let tree = usvg::Tree::from_data(svg.as_bytes(), &usvg::Options::default())
                .map_err(|error| SharedString::from(format!("{error}")))?;
            let shape = size(px(tree.size().width()), px(tree.size().height()));
            let picture = renderer
                .render_single_frame(svg.as_bytes(), chart.scale as f32 / 100.0)
                .map_err(|error| SharedString::from(format!("{error}")))?;
            Ok((picture, shape))
        }
    }
}

/// The diagram as SVG in Mermaid's theme for the mode.
fn svg(chart: &Chart) -> Result<String, SharedString> {
    let mut theme = match chart.dark {
        true => Theme::dark(),
        false => Theme::modern(),
    };
    theme.background = chart.ground.to_string();
    theme.edge_label_background = chart.ground.to_string();
    theme.text_color = chart.ink.to_string();
    let options = RenderOptions {
        theme,
        layout: LayoutConfig::default(),
    };
    // The renderer may panic on input it parsed.
    let drawn = catch_unwind(AssertUnwindSafe(|| render_strict(&chart.source, options)));
    match drawn {
        Ok(Ok(svg)) => Ok(svg),
        Ok(Err(error)) => Err(error.to_string().into()),
        Err(_) => {
            log::error!("diagram: the renderer failed on a parsed diagram");
            Err("the Mermaid renderer could not draw this diagram".into())
        }
    }
}

/// `#rrggbb` of a theme color.
fn hex(color: Hsla) -> SharedString {
    let rgb = color.to_rgb();
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        channel(rgb.r),
        channel(rgb.g),
        channel(rgb.b)
    )
    .into()
}

impl RenderOnce for Diagram {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let colors = theme.colors.clone();
        let chart = Chart {
            source: self.source.clone(),
            dark: theme.is_dark(),
            ground: hex(colors.surface),
            ink: hex(colors.fg),
            scale: (window.scale_factor() * 100.0).round() as u32,
        };
        let (radius, small, mono) = (
            theme.radius(Radius::Md),
            theme.text_size(TextSize::Sm),
            theme.mono_family.clone(),
        );
        match window.use_asset::<Drawer>(&chart, cx) {
            Some(Ok((picture, shape))) => div()
                .flex()
                .justify_center()
                .child(img(picture).w(shape.width).h(shape.height).max_w_full())
                .into_any_element(),
            Some(Err(why)) => div()
                .flex()
                .flex_col()
                .gap_1()
                .p_3()
                .rounded(radius)
                .bg(colors.sunken)
                .text_size(small)
                .child(div().text_color(colors.danger).child(why))
                .child(
                    div()
                        .font_family(mono)
                        .text_color(colors.fg_muted)
                        .child(self.source),
                )
                .into_any_element(),
            None => div().h_12().into_any_element(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chart(source: &str, dark: bool) -> Chart {
        Chart {
            source: source.to_string().into(),
            dark,
            ground: "#2c2c2c".into(),
            ink: "#eeeeee".into(),
            scale: 100,
        }
    }

    #[test]
    fn a_flowchart_draws_with_the_ground_and_ink() {
        let svg = svg(&chart("flowchart LR\n  A --> B", true)).expect("draws");
        assert!(svg.contains("<svg"), "{svg}");
        assert!(svg.to_lowercase().contains("#2c2c2c"), "the ground: {svg}");
    }

    #[test]
    fn a_broken_diagram_says_why() {
        let why = svg(&chart("flowchart LR\n  A -->", false)).expect_err("an error");
        assert!(!why.is_empty());
    }

    #[test]
    fn colors_write_as_hex() {
        assert_eq!(hex(gpui::rgb(0x2c2c2c).into()), "#2c2c2c");
    }
}
