//! What CSS says of a view beyond its box: shadows, gradients, filters, the
//! backdrop's blur, transforms, blending, transitions and keyframes. After
//! NSH's web backend (`nsh-platform-web`'s `encode.rs`), which draws the
//! same styles in a browser; the Mini Program's render layer is one too.

use std::fmt::Write as _;

use nsh_core::{
    Filter, Gradient, GradientKind, GradientSize, Keyframes, Transform, TransformKind,
    TransitionProps, ViewStyle,
    geometry::{DefiniteLength, REM_SIZE, Radians, Size, relative},
};

use crate::encode::color;

/// Appends the CSS of `s`'s effects, transform, blending, transition and
/// keyframes to `css`.
pub(crate) fn write(css: &mut String, s: &ViewStyle) {
    if let Some(effects) = &s.effects {
        let shadows: Vec<String> = effects
            .box_shadows
            .iter()
            .map(|b| {
                format!(
                    "{}{}px {}px {}px {}px {}",
                    if b.inset { "inset " } else { "" },
                    b.offset_x.0,
                    b.offset_y.0,
                    b.blur.0,
                    b.spread.0,
                    color(b.color)
                )
            })
            .collect();
        if !shadows.is_empty() {
            let _ = write!(css, "box-shadow:{};", shadows.join(","));
        }
        if !effects.background_images.is_empty() {
            let images: Vec<String> = effects.background_images.iter().map(gradient).collect();
            let _ = write!(css, "background-image:{};", images.join(","));
        }
        if !effects.filters.is_empty() {
            let _ = write!(css, "filter:{};", filters(&effects.filters));
        }
        if !effects.backdrop_filters.is_empty() {
            let f = filters(&effects.backdrop_filters);
            let _ = write!(css, "backdrop-filter:{f};-webkit-backdrop-filter:{f};");
        }
        if !effects.mask_images.is_empty() {
            let images: Vec<String> = effects.mask_images.iter().map(gradient).collect();
            let m = images.join(",");
            let _ = write!(css, "mask-image:{m};-webkit-mask-image:{m};");
        }
    }
    if !s.transform.is_identity() {
        let _ = write!(css, "transform:{};", transform(&s.transform, |_| None));
    }
    let o = s.transform_origin;
    if o.x != relative(0.5) || o.y != relative(0.5) {
        let _ = write!(css, "transform-origin:{} {};", origin(o.x), origin(o.y));
    }
    if s.backface_hidden {
        css.push_str("backface-visibility:hidden;");
    }
    let blend = kebab(&format!("{:?}", s.blend_mode));
    if blend != "normal" {
        let _ = write!(css, "mix-blend-mode:{blend};");
    }
    if let Some(t) = &s.transition {
        // Style changes arrive as setData: the render layer animates them.
        let p = t.properties;
        let mut props = Vec::new();
        if p.has(TransitionProps::COLORS) {
            props.extend(["background-color", "border-color", "color"]);
        }
        if p.has(TransitionProps::OPACITY) {
            props.push("opacity");
        }
        if p.has(TransitionProps::TRANSFORM) {
            props.push("transform");
        }
        if p.has(TransitionProps::SHADOW) {
            props.push("box-shadow");
        }
        if p.has(TransitionProps::SHAPE) {
            props.extend(["border-radius", "border-width"]);
        }
        let _ = write!(
            css,
            "transition-property:{};transition-duration:{}ms;transition-delay:{}ms;transition-timing-function:{};",
            props.join(","),
            t.duration,
            t.delay,
            bezier(t.easing)
        );
    }
    if let Some(a) = &s.animation {
        // `@keyframes` in the component's style (`nsh-node/index.wxss`).
        let name = match a.keyframes {
            Keyframes::Spin => "nsh-spin",
            Keyframes::Ping => "nsh-ping",
            Keyframes::Pulse => "nsh-pulse",
            Keyframes::Bounce => "nsh-bounce",
        };
        let iterations = a
            .iterations
            .map_or("infinite".to_owned(), |n| n.to_string());
        let _ = write!(
            css,
            "animation:{name} {}ms {} {}ms {iterations};",
            a.duration,
            bezier(a.easing),
            a.delay
        );
    }
}

/// The CSS transform functions of `t` (`value(i)` overrides entry `i`:
/// an animated one).
pub(crate) fn transform(t: &Transform, value: impl Fn(usize) -> Option<f32>) -> String {
    let mut out = String::new();
    for (i, op) in t.ops().iter().enumerate() {
        let v = value(i).unwrap_or(op.value);
        let _ = match op.kind {
            TransformKind::TranslateX => write!(out, "translateX({v}px) "),
            TransformKind::TranslateY => write!(out, "translateY({v}px) "),
            TransformKind::TranslateXFraction => write!(out, "translateX({}%) ", v * 100.0),
            TransformKind::TranslateYFraction => write!(out, "translateY({}%) ", v * 100.0),
            TransformKind::Scale => write!(out, "scale({v}) "),
            TransformKind::ScaleX => write!(out, "scaleX({v}) "),
            TransformKind::ScaleY => write!(out, "scaleY({v}) "),
            TransformKind::Rotate => write!(out, "rotate({v}deg) "),
            TransformKind::RotateX => write!(out, "rotateX({v}deg) "),
            TransformKind::RotateY => write!(out, "rotateY({v}deg) "),
            TransformKind::SkewX => write!(out, "skewX({v}deg) "),
            TransformKind::SkewY => write!(out, "skewY({v}deg) "),
            TransformKind::Perspective => write!(out, "perspective({v}px) "),
        };
    }
    out.trim_end().to_owned()
}

fn origin(o: DefiniteLength) -> String {
    match o {
        DefiniteLength::Absolute(v) => format!("{}px", v.to_points(REM_SIZE).0),
        DefiniteLength::Fraction(f) => format!("{}%", f.0 * 100.0),
    }
}

/// The CSS of `g`: the stops the core resolves, or, with lengths, the
/// stops as written (which only the box resolves).
fn gradient(g: &Gradient) -> String {
    let stops: Vec<String> = if g.has_lengths() {
        g.stops
            .iter()
            .map(|s| match s.position {
                Some(DefiniteLength::Fraction(p)) => format!("{} {}%", color(s.color), p.0 * 100.0),
                Some(DefiniteLength::Absolute(a)) => {
                    format!("{} {}px", color(s.color), a.to_points(REM_SIZE).0)
                }
                None => color(s.color),
            })
            .collect()
    } else {
        g.resolved_stops(Size::default())
            .iter()
            .map(|(c, p)| format!("{} {}%", color(*c), p.0 * 100.0))
            .collect()
    };
    let stops = stops.join(", ");
    let repeating = if g.repeating { "repeating-" } else { "" };
    match g.kind {
        GradientKind::Linear { angle } => {
            format!("{repeating}linear-gradient({}deg, {stops})", degrees(angle))
        }
        GradientKind::LinearCorner { right, bottom } => format!(
            "{repeating}linear-gradient(to {} {}, {stops})",
            if bottom { "bottom" } else { "top" },
            if right { "right" } else { "left" }
        ),
        GradientKind::Radial { circle, size, x, y } => {
            let size = match size {
                GradientSize::ClosestSide => "closest-side",
                GradientSize::ClosestCorner => "closest-corner",
                GradientSize::FarthestSide => "farthest-side",
                GradientSize::FarthestCorner => "farthest-corner",
            };
            format!(
                "{repeating}radial-gradient({} {size} at {}% {}%, {stops})",
                if circle { "circle" } else { "ellipse" },
                x.0 * 100.0,
                y.0 * 100.0
            )
        }
        GradientKind::Conic { from, x, y } => format!(
            "{repeating}conic-gradient(from {}deg at {}% {}%, {stops})",
            degrees(from),
            x.0 * 100.0,
            y.0 * 100.0
        ),
    }
}

/// Degrees rounded to 1e-4, so whole degrees stay whole.
fn degrees(a: Radians) -> f32 {
    (a.to_degrees() * 1e4).round() / 1e4
}

fn filters(list: &[Filter]) -> String {
    let mut out = String::new();
    for f in list {
        let _ = match *f {
            Filter::Blur(v) => write!(out, "blur({}px) ", v.0),
            Filter::Brightness(v) => write!(out, "brightness({v}) "),
            Filter::Contrast(v) => write!(out, "contrast({v}) "),
            Filter::Grayscale(v) => write!(out, "grayscale({v}) "),
            Filter::HueRotate(v) => write!(out, "hue-rotate({}deg) ", degrees(v)),
            Filter::Invert(v) => write!(out, "invert({v}) "),
            Filter::Opacity(v) => write!(out, "opacity({v}) "),
            Filter::Saturate(v) => write!(out, "saturate({v}) "),
            Filter::Sepia(v) => write!(out, "sepia({v}) "),
            Filter::DropShadow {
                offset_x,
                offset_y,
                blur,
                color: c,
            } => write!(
                out,
                "drop-shadow({}px {}px {}px {}) ",
                offset_x.0,
                offset_y.0,
                blur.0,
                color(c)
            ),
        };
    }
    out.trim_end().to_owned()
}

fn bezier(e: [f32; 4]) -> String {
    format!("cubic-bezier({},{},{},{})", e[0], e[1], e[2], e[3])
}

/// `FooBar` (a `Debug` name) as `foo-bar`.
fn kebab(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                out.push('-');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}
