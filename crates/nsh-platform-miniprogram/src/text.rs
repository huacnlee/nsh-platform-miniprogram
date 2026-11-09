//! The text engine: NSH lays text out once, on the engine's side
//! (`TextEngine`: prepare, measure at any width, lay out at the final
//! size), and the Mini Program only draws the lines it is given. Widths come
//! from the host ([`Measure`]: the logic layer's `OffscreenCanvas`
//! `measureText`); lines break greedily at spaces and between CJK
//! characters, so each line is placed where the engine measured it and the
//! render layer never wraps text itself (`white-space: pre`).
//!
//! The first span's style is the text's (a text of several styles draws in
//! the first one's): see the README's "Status".

use std::sync::Arc;

use nsh_core::{
    AttributedText, Points, Size, TextEngine, TextLayout,
    geometry::size,
    Color, TextAlign,
};

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

/// What the render layer needs to draw a text's run of lines.
#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    /// The CSS `font` shorthand the widths were measured in.
    pub font: String,
    /// The text color.
    pub color: Color,
    /// The height of each line.
    pub line_height: f32,
}

/// One line of a final layout, in the content box (y down).
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// Its text.
    pub text: String,
    /// Its left edge.
    pub x: f32,
    /// Its top edge.
    pub y: f32,
    /// Its width.
    pub width: f32,
}

/// A final layout: what [`nsh_core::MountItem::Text`] carries for this
/// platform.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// How the lines are drawn.
    pub style: TextStyle,
    /// The lines, top to bottom.
    pub lines: Vec<Line>,
}

/// A text prepared for measuring: its style and the widths of its pieces.
#[derive(Debug, Clone)]
pub struct Prepared {
    style: TextStyle,
    align: TextAlign,
    max_lines: u32,
    no_wrap: bool,
    /// Paragraphs (split at `\n`), each a list of pieces that do not break
    /// inside: words with their trailing spaces, single CJK characters.
    paragraphs: Vec<Vec<Piece>>,
}

#[derive(Debug, Clone)]
struct Piece {
    text: String,
    width: f32,
    /// The width without trailing spaces (a line's end does not count them).
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

    fn lines(&self, prepared: &Prepared, max_width: Option<f32>) -> Vec<Line> {
        let limit = match prepared.no_wrap {
            true => None,
            false => max_width,
        };
        let mut lines = Vec::new();
        for pieces in &prepared.paragraphs {
            let mut text = String::new();
            let (mut width, mut trimmed) = (0.0f32, 0.0f32);
            for piece in pieces {
                if let Some(limit) = limit
                    && !text.is_empty()
                    && width + piece.trimmed > limit + 0.01
                {
                    lines.push((std::mem::take(&mut text), trimmed));
                    width = 0.0;
                }
                text.push_str(&piece.text);
                trimmed = width + piece.trimmed;
                width += piece.width;
            }
            lines.push((text, trimmed));
        }
        if prepared.max_lines > 0 {
            lines.truncate(prepared.max_lines as usize);
        }
        let line_height = prepared.style.line_height;
        lines
            .into_iter()
            .enumerate()
            .map(|(i, (text, width))| Line {
                text: text.trim_end().to_owned(),
                x: 0.0,
                y: i as f32 * line_height,
                width,
            })
            .collect()
    }
}

impl<M: Measure> TextEngine for MiniProgramTextEngine<M> {
    type Prepared = Prepared;

    fn prepare(&self, text: &AttributedText) -> Prepared {
        let first = text.spans.first().map(|s| &s.style);
        let font_size = first.map_or(14.0, |s| s.font_size.0);
        let weight = first.map_or(400, |s| s.font_weight);
        let italic = first.is_some_and(|s| s.italic);
        let family = first
            .and_then(|s| s.font_family)
            .map_or_else(|| "sans-serif".to_owned(), |f| format!("\"{}\"", f.name()));
        let line_height = first
            .map(|s| s.line_height.0)
            .filter(|h| *h > 0.0)
            .unwrap_or(font_size * 1.2);
        let font = format!(
            "{} {weight} {font_size}px {family}",
            if italic { "italic" } else { "normal" }
        );
        let paragraphs = text
            .text
            .split('\n')
            .map(|paragraph| {
                pieces(paragraph)
                    .into_iter()
                    .map(|piece| {
                        let width = self.measure.width(&font, &piece);
                        let trimmed = match piece.trim_end().len() == piece.len() {
                            true => width,
                            false => self.measure.width(&font, piece.trim_end()),
                        };
                        Piece {
                            text: piece,
                            width,
                            trimmed,
                        }
                    })
                    .collect()
            })
            .collect();
        Prepared {
            style: TextStyle {
                font,
                color: first.map_or(Color(0x000000ff), |s| s.color),
                line_height,
            },
            align: text.align,
            max_lines: text.max_lines,
            no_wrap: text.no_wrap,
            paragraphs,
        }
    }

    fn measure(&self, prepared: &Prepared, max_width: Option<Points>) -> Size<Points> {
        let lines = self.lines(prepared, max_width.map(|w| w.0));
        let width = lines.iter().map(|l| l.width).fold(0.0, f32::max);
        // Rounded up, as the platforms' text stacks do: a box exactly this
        // wide must not wrap the text again.
        size(
            Points(width.ceil()),
            Points(lines.len() as f32 * prepared.style.line_height),
        )
    }

    fn breaks_lines_greedily(&self) -> bool {
        true
    }

    fn layout(&self, prepared: &Prepared, size: Size<Points>) -> TextLayout {
        let mut lines = self.lines(prepared, Some(size.width.0));
        for line in &mut lines {
            line.x = match prepared.align {
                TextAlign::Center => (size.width.0 - line.width) / 2.0,
                TextAlign::Right => size.width.0 - line.width,
                TextAlign::Auto | TextAlign::Left | TextAlign::Justify => 0.0,
            };
        }
        TextLayout(Arc::new(Layout {
            style: prepared.style.clone(),
            lines,
        }))
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
        let texts: Vec<_> = laid.lines.iter().map(|l| l.text.as_str()).collect();
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
        let texts: Vec<_> = laid.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["你好", "世界"]);
    }
}
