//! The batches the engine commits, as the operations the Mini Program's
//! runtime applies (`runtime/nsh-mp.js`): one JSON array per commit, each
//! operation an array led by its code. Everything here is plain Rust over
//! NSH's mount items, so it is tested on the host.
//!
//! | Op | Fields | Meaning |
//! |---|---|---|
//! | `c` | id, kind | create a view (`view`, `text`, `scroll`, `image`) |
//! | `i` | parent, child, before | insert `child` below sibling `before` (`null`: on top) |
//! | `r` | id | detach a view, keeping it |
//! | `d` | id | destroy a view |
//! | `f` | id, x, y, width, height | the frame, in the parent (px) |
//! | `s` | id, css | the view's decoration as inline CSS |
//! | `t` | id, text | the lines of a text: `{ font, color, lineHeight, x, y, lines: [[text, x, y]] }` |
//! | `e` | id, events | the events it reports ([`events`]: bits) |
//! | `sc` | id, horizontal | a scroll view's axis |
//! | `ss` | id, width, height | a scroll view's content size |
//! | `im` | id, src, mode | an image's source (`null`: none) and WeChat `mode` |
//!
//! Items this platform does not show yet are left out (`MountItem`'s other
//! kinds: the README's "Status").

use std::fmt::Write as _;

use nsh_core::{BorderStyle, Color, EventKind, MountBatch, MountItem, ViewKind, ViewStyle};
use serde_json::{Value, json};

use crate::text::Layout;

/// The bits of op `e`.
pub mod events {
    /// `onPress`.
    pub const PRESS: u32 = 1;
    /// `onPressIn` / `onPressOut`.
    pub const PRESS_IN_OUT: u32 = 2;
    /// `onLongPress`.
    pub const LONG_PRESS: u32 = 4;
}

/// A batch's operations, `[]` when it has none for this platform.
pub fn batch(batch: &MountBatch) -> Value {
    Value::Array(batch.items.iter().filter_map(item).collect())
}

/// One item's operation, if this platform shows it.
pub fn item(item: &MountItem) -> Option<Value> {
    Some(match item {
        MountItem::Create { id, kind } => json!(["c", id, kind_name(*kind)]),
        MountItem::Insert {
            parent,
            child,
            before,
        } => json!(["i", parent, child, before]),
        MountItem::Remove { id } => json!(["r", id]),
        MountItem::Delete { id } => json!(["d", id]),
        MountItem::Frame { id, frame } => json!([
            "f",
            id,
            round(frame.origin.x.0),
            round(frame.origin.y.0),
            round(frame.size.width.0),
            round(frame.size.height.0)
        ]),
        MountItem::ViewStyle { id, style } => json!(["s", id, css(style)]),
        MountItem::Text {
            id,
            layout,
            content,
            ..
        } => {
            let layout = layout.downcast::<Layout>()?;
            let lines: Vec<Value> = layout
                .lines
                .iter()
                .map(|l| json!([l.text, round(l.x), round(l.y)]))
                .collect();
            json!(["t", id, {
                "font": layout.style.font,
                "color": color(layout.style.color),
                "lineHeight": layout.style.line_height,
                "x": round(content.origin.x.0),
                "y": round(content.origin.y.0),
                "lines": lines,
            }])
        }
        MountItem::Events { id, events } => {
            let mut bits = 0;
            if events.contains(EventKind::Press) {
                bits |= events::PRESS;
            }
            if events.contains(EventKind::PressDown) || events.contains(EventKind::PressUp) {
                bits |= events::PRESS_IN_OUT;
            }
            if events.contains(EventKind::LongPress) {
                bits |= events::LONG_PRESS;
            }
            json!(["e", id, bits])
        }
        MountItem::ScrollConfig { id, horizontal, .. } => json!(["sc", id, horizontal]),
        MountItem::ScrollContent { id, size } => {
            json!(["ss", id, round(size.width.0), round(size.height.0)])
        }
        MountItem::Image {
            id,
            source,
            resize_mode,
            ..
        } => {
            let mode = match resize_mode {
                nsh_core::ResizeMode::Cover => "aspectFill",
                nsh_core::ResizeMode::Contain | nsh_core::ResizeMode::ScaleDown => "aspectFit",
                nsh_core::ResizeMode::Stretch => "scaleToFill",
                nsh_core::ResizeMode::Center => "center",
            };
            json!(["im", id, source.as_ref().map(|s| image_src(&s.uri)), mode])
        }
        _ => return None,
    })
}

/// The kind a view is shown as.
fn kind_name(kind: ViewKind) -> &'static str {
    match kind {
        ViewKind::Text => "text",
        ViewKind::Scroll => "scroll",
        ViewKind::Image => "image",
        _ => "view",
    }
}

/// An image URI as the `<image>` component loads it: an app's own file
/// (`asset:/x.png`) from the package's `assets/`, a URL as it is.
fn image_src(uri: &str) -> String {
    match uri.strip_prefix("asset:/") {
        Some(path) => format!("/assets/{}", path.trim_start_matches('/')),
        None => uri.to_owned(),
    }
}

/// `v` to a hundredth: the render layer's pixels need no more.
fn round(v: f32) -> f64 {
    (f64::from(v) * 100.0).round() / 100.0
}

/// A color as CSS (`rgba(…)`).
pub fn color(c: Color) -> String {
    let [r, g, b, a] = c.0.to_be_bytes();
    if a == 255 {
        format!("rgb({r},{g},{b})")
    } else {
        format!("rgba({r},{g},{b},{})", round(f32::from(a) / 255.0))
    }
}

/// A view's decoration as inline CSS: background, corners, borders,
/// opacity, clipping, hiding.
pub fn css(style: &ViewStyle) -> String {
    let mut out = String::new();
    if style.background_color.0 & 0xff != 0 {
        let _ = write!(out, "background-color:{};", color(style.background_color));
    }
    let corners = style.corner_radius;
    let radius = |r: Option<nsh_core::Points>| round(r.unwrap_or(style.border_radius).0);
    let radii = [
        radius(corners.top_left),
        radius(corners.top_right),
        radius(corners.bottom_right),
        radius(corners.bottom_left),
    ];
    if radii.iter().any(|r| *r > 0.0) {
        if radii.iter().all(|r| *r == radii[0]) {
            let _ = write!(out, "border-radius:{}px;", radii[0]);
        } else {
            let _ = write!(
                out,
                "border-radius:{}px {}px {}px {}px;",
                radii[0], radii[1], radii[2], radii[3]
            );
        }
    }
    let borders = &style.borders;
    let kind = match borders.style {
        BorderStyle::Solid => "solid",
        BorderStyle::Dashed => "dashed",
        BorderStyle::Dotted => "dotted",
    };
    let widths = borders.widths;
    for (side, width, c) in [
        ("top", widths.top, borders.colors[0]),
        ("right", widths.right, borders.colors[1]),
        ("bottom", widths.bottom, borders.colors[2]),
        ("left", widths.left, borders.colors[3]),
    ] {
        if width.0 > 0.0 {
            let _ = write!(
                out,
                "border-{side}:{}px {kind} {};",
                round(width.0),
                color(c)
            );
        }
    }
    if style.opacity < 1.0 {
        let _ = write!(out, "opacity:{};", round(style.opacity));
    }
    if style.overflow_hidden {
        out.push_str("overflow:hidden;");
    }
    if style.hidden {
        out.push_str("visibility:hidden;");
    }
    out
}

#[cfg(test)]
mod tests {
    use nsh_core::{
        Points,
        geometry::{Bounds, point, size},
    };

    use super::*;

    #[test]
    fn structure_and_frames() {
        let items = [
            MountItem::Create {
                id: 2,
                kind: ViewKind::View,
            },
            MountItem::Insert {
                parent: 0,
                child: 2,
                before: None,
            },
            MountItem::Frame {
                id: 2,
                frame: Bounds::new(
                    point(Points(1.0), Points(2.5)),
                    size(Points(100.0), Points(1.0 / 3.0)),
                ),
            },
        ];
        let ops: Vec<_> = items.iter().filter_map(item).collect();
        assert_eq!(
            Value::Array(ops),
            json!([["c", 2, "view"], ["i", 0, 2, null], ["f", 2, 1.0, 2.5, 100.0, 0.33]])
        );
    }

    #[test]
    fn decoration_as_css() {
        let style = ViewStyle {
            background_color: Color(0xff000080),
            border_radius: Points(8.0),
            opacity: 0.5,
            overflow_hidden: true,
            ..ViewStyle::default()
        };
        assert_eq!(
            css(&style),
            "background-color:rgba(255,0,0,0.5);border-radius:8px;opacity:0.5;overflow:hidden;"
        );
        assert_eq!(css(&ViewStyle::default()), "");
    }

    #[test]
    fn image_sources() {
        assert_eq!(image_src("asset:/img/a.png"), "/assets/img/a.png");
        assert_eq!(image_src("https://x.dev/a.png"), "https://x.dev/a.png");
    }
}
