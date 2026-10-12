use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, OnceLock},
};

use gpui::{
    App, Asset, ElementId, Hsla, IntoElement, ParentElement, Pixels, RenderImage, RenderOnce,
    SharedString, Size, Styled, Window, div, img, px, size,
};
use mermaid_rs_renderer::{LayoutConfig, RenderOptions, Theme, render_strict};

use crate::{
    primitives::hold,
    theme::{ActiveTheme, Radius, TextSize},
};

/// A Mermaid diagram; a failure shows why and the source.
#[derive(IntoElement)]
pub struct Diagram {
    id: ElementId,
    source: SharedString,
}

impl Diagram {
    pub fn new(id: impl Into<ElementId>, source: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
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
    let (ground, ink) = (chart.ground.to_string(), chart.ink.to_string());
    theme.background = ground.clone();
    theme.edge_label_background = ground;
    theme.primary_text_color = ink.clone();
    theme.text_color = ink.clone();
    theme.pie_title_text_color = ink.clone();
    theme.pie_section_text_color = ink.clone();
    theme.pie_legend_text_color = ink;
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

/// `source` as light SVG of plain paths, for export.
pub fn diagram_svg(source: &str) -> Result<String, SharedString> {
    let chart = Chart {
        source: source.to_owned().into(),
        dark: false,
        ground: "#ffffff".into(),
        ink: "#000000".into(),
        scale: 100,
    };
    let drawn = svg(&chart)?;
    // Markers and text become paths any reader draws.
    let options = usvg::Options {
        fontdb: fonts(),
        ..usvg::Options::default()
    };
    let tree = usvg::Tree::from_data(drawn.as_bytes(), &options)
        .map_err(|error| SharedString::from(format!("{error}")))?;
    // A label without a font vanishes: say so.
    let (written, kept) = (labels(&drawn)?, texts(tree.root()));
    if kept < written {
        log::error!("diagram: {kept} of {written} labels found a font");
        return Err("no installed font could set this diagram's labels".into());
    }
    Ok(tree.to_string(&usvg::WriteOptions::default()))
}

/// The `<text>` elements holding words.
fn labels(svg: &str) -> Result<usize, SharedString> {
    let doc = usvg::roxmltree::Document::parse(svg)
        .map_err(|error| SharedString::from(format!("{error}")))?;
    let worded = |node: &usvg::roxmltree::Node| {
        node.descendants()
            .any(|word| word.text().is_some_and(|text| !text.trim().is_empty()))
    };
    Ok(doc
        .descendants()
        .filter(|node| node.has_tag_name("text") && worded(node))
        .count())
}

/// Text nodes usvg set.
fn texts(group: &usvg::Group) -> usize {
    group
        .children()
        .iter()
        .map(|node| match node {
            usvg::Node::Group(inner) => texts(inner),
            usvg::Node::Text(_) => 1,
            _ => 0,
        })
        .sum()
}

/// The system's fonts, read once.
fn fonts() -> Arc<usvg::fontdb::Database> {
    static FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut fonts = usvg::fontdb::Database::new();
            fonts.load_system_fonts();
            log::info!("diagram: {} system font faces for export", fonts.len());
            Arc::new(fonts)
        })
        .clone()
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
        let picture = |drawn: &Drawn| drawn.as_ref().ok().map(|(image, _)| image.clone());
        match hold::<Drawer>(self.id, &chart, picture, window, cx) {
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
        let svg = svg(&chart("flowchart LR\n  A[Start] --> B", true)).expect("draws");
        let svg = svg.to_lowercase();
        assert!(svg.contains("#2c2c2c"), "the ground: {svg}");
        let label = svg.find(">start<").expect("the label");
        let tag = &svg[svg[..label].rfind("<text").expect("its tag")..label];
        assert!(tag.contains("fill=\"#eeeeee\""), "the label's ink: {tag}");
    }

    #[test]
    fn a_broken_diagram_says_why() {
        let why = svg(&chart("flowchart LR\n  A -->", false)).expect_err("an error");
        assert!(!why.is_empty());
    }

    #[test]
    fn an_exported_diagram_is_plain_paths() {
        let svg = diagram_svg(
            "flowchart LR
  A[Start] --> B",
        )
        .expect("draws");
        assert!(svg.contains("<path"), "{svg}");
        assert!(!svg.contains("<marker") && !svg.contains("<text"), "{svg}");
    }

    #[test]
    fn colors_write_as_hex() {
        assert_eq!(hex(gpui::rgb(0x2c2c2c).into()), "#2c2c2c");
    }
}
