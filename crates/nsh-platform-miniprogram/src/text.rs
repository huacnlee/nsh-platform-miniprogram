//! The text engine: NSH lays text out once, on the engine's side
//! (`TextEngine`: prepare, measure at any width, lay out at the final
//! size), and the Mini Program only draws the lines it is given. Widths come
//! from the host ([`Measure`]: the logic layer's `OffscreenCanvas`
//! `measureText`); lines break greedily at spaces and between CJK
//! characters, so each line is placed where the engine measured it and the
//! render layer never wraps text itself (`white-space: pre`).
//!
//! Each span keeps its own style: a line is runs of one style each,
//! measured in their font and placed on the line's shared baseline, and an
//! inline view takes its placeholder's place.

use std::{fmt::Write as _, sync::Arc};

use nsh_core::{
    AttributedText, DecorationStyle, Points, ResolvedTextStyle, Size, TextAlign, TextEngine,
    TextLayout, TextTransform,
    geometry::{Point, point, size},
};

use crate::encode::color;

/// Measures text in the host's fonts.
pub trait Measure {
    /// The advance width of `text` in `font` (a CSS `font` shorthand).
    fn width(&self, font: &str, text: &str) -> f32;
}

/// A deterministic [`Measure`] for tests and hosts without fonts: every
/// character is `0.55 ×` the font size wide, CJK characters `1 ×`.
#[derive(Debug, Default, Clone, Copy)]
pub struct ApproxMeasure;

impl Measure for ApproxMeasure {
    fn width(&self, font: &str, text: &str) -> f32 {
        let size = font_size_of(font);
        text.chars()
            .map(|c| if is_cjk(c) { size } else { size * 0.55 })
            .sum()
    }
}

/// The pixel size in a CSS `font` shorthand (`"normal 400 16px sans-serif"`).
fn font_size_of(font: &str) -> f32 {
    font.split_whitespace()
        .find_map(|part| part.strip_suffix("px")?.parse().ok())
        .unwrap_or(14.0)
}

/// How one run of a text is measured and drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct RunStyle {
    /// The CSS `font` shorthand the widths are measured in (the canvas's
    /// `font`).
    font: String,
    /// The run as CSS declarations: font (the render layer drops a `font`
    /// shorthand in inline styles), color, decoration, letter spacing.
    pub css: String,
    /// The font size.
    size: f32,
    /// The height of its lines.
    line_height: f32,
    /// Extra space after each character.
    letter_spacing: f32,
    transform: TextTransform,
}

/// A stretch of a line in one style, at its place in the content box.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    /// Its text.
    pub text: String,
    /// Its left edge.
    pub x: f32,
    /// The top of its line box.
    pub y: f32,
    /// The height of its line box (its style's line height): runs of one
    /// line share a baseline.
    pub height: f32,
    /// Its style ([`Layout::styles`]).
    pub style: usize,
}

/// One line of a final layout, in the content box (y down).
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// Its runs, left to right.
    pub runs: Vec<Run>,
    /// Its left edge.
    pub x: f32,
    /// Its top edge.
    pub y: f32,
    /// Its width.
    pub width: f32,
    /// Its height: the tallest run's.
    pub height: f32,
}

impl Line {
    /// Its text, the runs' together.
    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }
}

/// A final layout: what [`nsh_core::MountItem::Text`] carries for this
/// platform.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// The runs' styles.
    pub styles: Vec<RunStyle>,
    /// The lines, top to bottom.
    pub lines: Vec<Line>,
    /// Where each attachment's placeholder went (top left, content box).
    attachments: Vec<Option<Point<Points>>>,
}

/// A text prepared for measuring: its styles and the widths of its pieces.
#[derive(Debug, Clone)]
pub struct Prepared {
    styles: Vec<RunStyle>,
    align: TextAlign,
    max_lines: u32,
    no_wrap: bool,
    /// The height of an empty line (the first style's).
    empty_line: f32,
    /// Paragraphs (split at `\n`), each a list of pieces that do not break
    /// inside: words with their trailing spaces, single CJK characters.
    paragraphs: Vec<Vec<Piece>>,
    /// How many attachments the text has.
    attachments: usize,
}

/// A stretch of a piece in one style (or an attachment's place).
#[derive(Debug, Clone)]
struct Segment {
    text: String,
    style: usize,
    width: f32,
    /// An attachment's index and height, for its placeholder.
    attachment: Option<(usize, f32)>,
}

#[derive(Debug, Clone)]
struct Piece {
    segments: Vec<Segment>,
    width: f32,
    /// The width without trailing spaces (a line's end does not count them).
    trimmed: f32,
}

/// A line while it is broken: its segments and widths.
#[derive(Default)]
struct Building {
    segments: Vec<Segment>,
    width: f32,
    trimmed: f32,
}

/// [`TextEngine`] over the host's [`Measure`].
#[derive(Debug, Clone, Default)]
pub struct MiniProgramTextEngine<M> {
    measure: M,
}

impl<M: Measure> MiniProgramTextEngine<M> {
    /// An engine measuring with `measure`.
    pub fn new(measure: M) -> Self {
        MiniProgramTextEngine { measure }
    }

    fn width(&self, style: &RunStyle, text: &str) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        self.measure.width(&style.font, text) + style.letter_spacing * text.chars().count() as f32
    }

    /// The text's lines broken at `max_width`, each its segments and its
    /// width without trailing spaces.
    fn break_lines(&self, prepared: &Prepared, max_width: Option<f32>) -> Vec<Building> {
        let limit = match prepared.no_wrap {
            true => None,
            false => max_width,
        };
        let mut lines = Vec::new();
        for pieces in &prepared.paragraphs {
            let mut line = Building::default();
            for piece in pieces {
                if let Some(limit) = limit
                    && !line.segments.is_empty()
                    && line.width + piece.trimmed > limit + 0.01
                {
                    lines.push(std::mem::take(&mut line));
                }
                line.segments.extend(piece.segments.iter().cloned());
                line.trimmed = line.width + piece.trimmed;
                line.width += piece.width;
            }
            lines.push(line);
        }
        if prepared.max_lines > 0 {
            lines.truncate(prepared.max_lines as usize);
        }
        lines
    }

    /// A line's height and the distance from its top to its baseline:
    /// each run's line box holds its glyphs around its middle, so a
    /// baseline sits about 0.3 em below it.
    fn metrics(prepared: &Prepared, segments: &[Segment]) -> (f32, f32) {
        let (mut above, mut below) = (0.0f32, 0.0f32);
        for segment in segments {
            let style = &prepared.styles[segment.style];
            let (a, b) = match segment.attachment {
                // An attachment stands on the baseline.
                Some((_, height)) => (height, 0.0),
                None => {
                    let a = style.line_height / 2.0 + 0.3 * style.size;
                    (a, style.line_height - a)
                }
            };
            above = above.max(a);
            below = below.max(b);
        }
        if above + below <= 0.0 {
            return (prepared.empty_line, prepared.empty_line / 2.0);
        }
        (above + below, above)
    }

    fn lines(&self, prepared: &Prepared, max_width: Option<f32>, width: f32) -> Layout {
        let mut lines = Vec::new();
        let mut attachments = vec![None; prepared.attachments];
        let mut y = 0.0;
        for built in self.break_lines(prepared, max_width) {
            let (height, baseline) = Self::metrics(prepared, &built.segments);
            let x0 = match prepared.align {
                TextAlign::Center => (width - built.trimmed) / 2.0,
                TextAlign::Right => width - built.trimmed,
                TextAlign::Auto | TextAlign::Left | TextAlign::Justify => 0.0,
            };
            let mut runs: Vec<Run> = Vec::new();
            let mut x = x0;
            for segment in &built.segments {
                let style = &prepared.styles[segment.style];
                if let Some((index, h)) = segment.attachment {
                    attachments[index] = Some(point(Points(x), Points(y + baseline - h)));
                } else if let Some(last) = runs.last_mut()
                    && last.style == segment.style
                    && (last.x + self.width(style, &last.text) - x).abs() < 0.5
                {
                    last.text.push_str(&segment.text);
                } else {
                    let a = style.line_height / 2.0 + 0.3 * style.size;
                    runs.push(Run {
                        text: segment.text.clone(),
                        x,
                        y: y + baseline - a,
                        height: style.line_height,
                        style: segment.style,
                    });
                }
                x += segment.width;
            }
            if let Some(last) = runs.last_mut() {
                let kept = last.text.trim_end().len();
                last.text.truncate(kept);
            }
            lines.push(Line {
                runs,
                x: x0,
                y,
                width: built.trimmed,
                height,
            });
            y += height;
        }
        Layout {
            styles: prepared.styles.clone(),
            lines,
            attachments,
        }
    }
}

/// The family texts without one of their own are measured and drawn in,
/// named rather than `sans-serif`: the logic layer's canvas and the render
/// layer resolve `sans-serif` to different fonts (Helvetica and PingFang SC
/// in WeChat DevTools on a Mac), so lines measured in one would not fit in
/// the other. WeChat's own on iOS first, then the common system fonts.
const DEFAULT_FAMILY: &str = "'PingFang SC','Helvetica Neue',Helvetica,Arial,sans-serif";

/// The CSS family list of a text in `name` (the default's without one).
fn family(name: Option<&str>) -> String {
    name.map_or_else(
        || DEFAULT_FAMILY.to_owned(),
        |f| format!("'{f}',{DEFAULT_FAMILY}"),
    )
}

/// A font as CSS declarations (the render layer drops a `font` shorthand
/// in inline styles).
pub(crate) fn font_css(size: f32, weight: u16, italic: bool, name: Option<&str>) -> String {
    format!(
        "font-style:{};font-weight:{weight};font-size:{size}px;font-family:{};",
        if italic { "italic" } else { "normal" },
        family(name)
    )
}

/// The style of a run in `style`.
fn run_style(style: &ResolvedTextStyle) -> RunStyle {
    let size = style.font_size.0;
    let name = style.font_family.as_ref().map(|f| f.name());
    let italic = if style.italic { "italic" } else { "normal" };
    let font = format!(
        "{italic} {} {size}px {}",
        style.font_weight,
        family(name.as_deref())
    );
    let mut css = font_css(size, style.font_weight, style.italic, name.as_deref());
    let _ = write!(css, "color:{};", color(style.color));
    let d = style.decoration;
    if !d.is_none() {
        let lines = match (d.underline, d.line_through) {
            (true, true) => "underline line-through",
            (true, false) => "underline",
            _ => "line-through",
        };
        let kind = match style.decoration_style {
            DecorationStyle::Double => "double",
            DecorationStyle::Dotted => "dotted",
            DecorationStyle::Dashed => "dashed",
            DecorationStyle::Wavy => "wavy",
            _ => "solid",
        };
        let _ = write!(css, "text-decoration:{lines} {kind};");
        if let Some(c) = style.decoration_color {
            let _ = write!(css, "text-decoration-color:{};", color(c));
        }
    }
    if style.letter_spacing.0 != 0.0 {
        let _ = write!(css, "letter-spacing:{}px;", style.letter_spacing.0);
    }
    let line_height = Some(style.line_height.0)
        .filter(|h| *h > 0.0)
        .unwrap_or(size * 1.2);
    RunStyle {
        font,
        css,
        size,
        line_height,
        letter_spacing: style.letter_spacing.0,
        transform: style.transform,
    }
}

fn transformed(text: &str, transform: TextTransform) -> String {
    match transform {
        TextTransform::Uppercase => text.to_uppercase(),
        TextTransform::Lowercase => text.to_lowercase(),
        TextTransform::Capitalize => {
            let mut out = String::with_capacity(text.len());
            let mut start = true;
            for c in text.chars() {
                if start && c.is_alphabetic() {
                    out.extend(c.to_uppercase());
                } else {
                    out.push(c);
                }
                start = c.is_whitespace();
            }
            out
        }
        _ => text.to_owned(),
    }
}

impl<M: Measure> TextEngine for MiniProgramTextEngine<M> {
    type Prepared = Prepared;

    fn prepare(&self, text: &AttributedText) -> Prepared {
        let mut styles: Vec<RunStyle> = text.spans.iter().map(|s| run_style(&s.style)).collect();
        if styles.is_empty() {
            styles.push(run_style(&ResolvedTextStyle::default()));
        }
        // Each character's style (spans are in UTF-16 offsets), and the
        // attachments by the offset of their placeholder.
        let mut char_styles = Vec::with_capacity(text.text.len());
        let mut span = 0;
        let mut offset = 0u32;
        let mut attachment_at = std::collections::HashMap::new();
        for (i, a) in text.attachments.iter().enumerate() {
            attachment_at.insert(a.index, (i, a.width.0, a.height.0));
        }
        let mut char_attachments = Vec::with_capacity(text.text.len());
        for c in text.text.chars() {
            while span + 1 < text.spans.len() && offset >= text.spans[span].end {
                span += 1;
            }
            char_styles.push(span.min(styles.len() - 1));
            char_attachments.push(match c {
                '\u{FFFC}' => attachment_at.get(&offset).copied(),
                _ => None,
            });
            offset += c.len_utf16() as u32;
        }
        let mut index = 0;
        let paragraphs =
            text.text
                .split('\n')
                .map(|paragraph| {
                    let pieces = pieces(paragraph)
                        .into_iter()
                        .map(|piece| {
                            let mut segments: Vec<Segment> = Vec::new();
                            for c in piece.chars() {
                                let style = char_styles[index];
                                if let Some((i, width, height)) = char_attachments[index] {
                                    segments.push(Segment {
                                        text: String::new(),
                                        style,
                                        width,
                                        attachment: Some((i, height)),
                                    });
                                } else {
                                    match segments.last_mut() {
                                        Some(last)
                                            if last.style == style && last.attachment.is_none() =>
                                        {
                                            last.text.push(c)
                                        }
                                        _ => segments.push(Segment {
                                            text: c.to_string(),
                                            style,
                                            width: 0.0,
                                            attachment: None,
                                        }),
                                    }
                                }
                                index += 1;
                            }
                            let mut width = 0.0;
                            let mut trimmed = 0.0;
                            let count = segments.len();
                            for (n, segment) in segments.iter_mut().enumerate() {
                                if segment.attachment.is_none() {
                                    let style = &styles[segment.style];
                                    segment.text = transformed(&segment.text, style.transform);
                                    segment.width = self.width(style, &segment.text);
                                }
                                trimmed = width
                                    + match n + 1 == count && segment.attachment.is_none() {
                                        true => self
                                            .width(&styles[segment.style], segment.text.trim_end()),
                                        false => segment.width,
                                    };
                                width += segment.width;
                            }
                            Piece {
                                segments,
                                width,
                                trimmed,
                            }
                        })
                        .collect();
                    // The newline.
                    index += 1;
                    pieces
                })
                .collect();
        let empty_line = styles[0].line_height;
        Prepared {
            styles,
            align: text.align,
            max_lines: text.max_lines,
            no_wrap: text.no_wrap,
            empty_line,
            paragraphs,
            attachments: text.attachments.len(),
        }
    }

    fn measure(&self, prepared: &Prepared, max_width: Option<Points>) -> Size<Points> {
        let layout = self.lines(prepared, max_width.map(|w| w.0), 0.0);
        let width = layout.lines.iter().map(|l| l.width).fold(0.0, f32::max);
        let height: f32 = layout.lines.iter().map(|l| l.height).sum();
        // Rounded up, as the platforms' text stacks do: a box exactly this
        // wide must not wrap the text again.
        size(Points(width.ceil()), Points(height))
    }

    fn breaks_lines_greedily(&self) -> bool {
        true
    }

    fn layout(&self, prepared: &Prepared, size: Size<Points>) -> TextLayout {
        TextLayout(Arc::new(self.lines(
            prepared,
            Some(size.width.0),
            size.width.0,
        )))
    }

    fn attachments(&self, layout: &TextLayout) -> Vec<Option<Point<Points>>> {
        layout
            .downcast::<Layout>()
            .map(|l| l.attachments.clone())
            .unwrap_or_default()
    }
}

/// Whether a line may break before and after `c`: CJK ideographs, kana,
/// hangul and their punctuation.
fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x2E80..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFFEF
            | 0x20000..=0x3FFFF)
}

/// `text` cut where lines may break: after the spaces following a word,
/// and around each CJK character.
fn pieces(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_spaces = false;
    for c in text.chars() {
        if is_cjk(c) {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            out.push(c.to_string());
            in_spaces = false;
            continue;
        }
        if c.is_whitespace() {
            in_spaces = true;
        } else if in_spaces {
            out.push(std::mem::take(&mut current));
            in_spaces = false;
        }
        current.push(c);
    }
    if !current.is_empty() || out.is_empty() {
        out.push(current);
    }
    out
}

#[cfg(test)]
mod tests {
    use nsh_core::geometry::size;

    use super::*;

    fn text(s: &str) -> AttributedText {
        AttributedText {
            text: s.to_owned(),
            ..AttributedText::default()
        }
    }

    fn layout(s: &str, width: f32) -> Layout {
        let engine = MiniProgramTextEngine::new(ApproxMeasure);
        let prepared = engine.prepare(&text(s));
        let laid = engine.layout(&prepared, size(Points(width), Points(100.0)));
        laid.downcast::<Layout>().unwrap().clone()
    }

    #[test]
    fn pieces_break_after_spaces_and_around_cjk() {
        assert_eq!(pieces("hello  world"), ["hello  ", "world"]);
        assert_eq!(pieces("你好abc"), ["你", "好", "abc"]);
        assert_eq!(pieces(""), [""]);
    }

    #[test]
    fn lines_wrap_greedily_and_measure_their_widest() {
        // 14px: 7.7 per character; "hello " and "world" are 46.2 and 38.5.
        let laid = layout("hello world", 60.0);
        let texts: Vec<_> = laid.lines.iter().map(Line::text).collect();
        assert_eq!(texts, ["hello", "world"]);
        assert_eq!(laid.lines[1].y, 14.0 * 1.2);
        let engine = MiniProgramTextEngine::new(ApproxMeasure);
        let prepared = engine.prepare(&text("hello world"));
        let one_line = engine.measure(&prepared, None);
        assert_eq!(one_line.width.0, (11.0f32 * 7.7).ceil());
        // As wide as measured: the same single line.
        let again = engine.measure(&prepared, Some(one_line.width));
        assert_eq!(again.height.0, 14.0 * 1.2);
    }

    #[test]
    fn line_breaks_and_line_limits() {
        assert_eq!(layout("a\nb\nc", 100.0).lines.len(), 3);
        let engine = MiniProgramTextEngine::new(ApproxMeasure);
        let prepared = engine.prepare(&AttributedText {
            max_lines: 1,
            ..text("a\nb")
        });
        assert_eq!(engine.measure(&prepared, None).height.0, 14.0 * 1.2);
    }

    #[test]
    fn cjk_wraps_between_characters() {
        let laid = layout("你好世界", 30.0);
        let texts: Vec<_> = laid.lines.iter().map(Line::text).collect();
        assert_eq!(texts, ["你好", "世界"]);
    }

    #[test]
    fn spans_keep_their_styles_on_a_shared_baseline() {
        let span = |start, end, font_size: f32, color: nsh_core::Color| {
            let style = ResolvedTextStyle {
                font_size: Points(font_size),
                line_height: Points(font_size * 1.2),
                color,
                ..ResolvedTextStyle::default()
            };
            nsh_core::TextSpan { start, end, style }
        };
        let attachment = nsh_core::TextAttachment {
            index: 4,
            width: Points(10.0),
            height: Points(10.0),
        };
        let content = AttributedText {
            spans: vec![
                span(0, 4, 14.0, nsh_core::Color(0x000000ff)),
                span(4, 7, 28.0, nsh_core::Color(0xff0000ff)),
            ],
            attachments: vec![attachment],
            ..text("Hi, \u{FFFC}Yo")
        };
        let engine = MiniProgramTextEngine::new(ApproxMeasure);
        let prepared = engine.prepare(&content);
        let laid = engine.layout(&prepared, size(Points(500.0), Points(100.0)));
        let laid = laid.downcast::<Layout>().unwrap();
        let line = &laid.lines[0];
        let texts: Vec<_> = line.runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(texts, ["Hi, ", "Yo"]);
        assert_eq!(line.runs[1].style, 1);
        assert!(laid.styles[1].css.contains("font-size:28px"));
        // Both baselines meet: each run's top is its own line box's.
        let baseline = |r: &Run, size: f32| r.y + r.height / 2.0 + 0.3 * size;
        assert!((baseline(&line.runs[0], 14.0) - baseline(&line.runs[1], 28.0)).abs() < 0.01);
        // The view follows "Hi, " and stands on the baseline.
        let place = engine.attachments(&TextLayout(Arc::new(laid.clone())))[0].unwrap();
        assert!((place.x.0 - 4.0 * 7.7).abs() < 0.01);
        assert!((place.y.0 + 10.0 - baseline(&line.runs[0], 14.0)).abs() < 0.01);
    }
}
