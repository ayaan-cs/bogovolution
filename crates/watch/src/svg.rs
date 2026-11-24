//! A colored SVG picture of a rendered frame, for documentation. The picture
//! has no scripts and no external fonts: it is plain rectangles and text.

use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};

const CELL_W: f64 = 8.4;
const CELL_H: f64 = 18.0;
const FONT_SIZE: f64 = 14.0;
const DEFAULT_FG: &str = "#cdd6f4";
const DEFAULT_BG: &str = "#1e1e2e";

fn hex(c: Color) -> Option<String> {
    let s = match c {
        Color::Reset => return None,
        Color::Black => "#45475a",
        Color::Red | Color::LightRed => "#f38ba8",
        Color::Green | Color::LightGreen => "#a6e3a1",
        Color::Yellow | Color::LightYellow => "#f9e2af",
        Color::Blue | Color::LightBlue => "#89b4fa",
        Color::Magenta | Color::LightMagenta => "#cba6f7",
        Color::Cyan | Color::LightCyan => "#94e2d5",
        Color::Gray => "#bac2de",
        Color::DarkGray => "#7f849c",
        Color::White => "#ffffff",
        Color::Rgb(r, g, b) => return Some(format!("#{r:02x}{g:02x}{b:02x}")),
        Color::Indexed(_) => return None,
    };
    Some(s.to_string())
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[derive(PartialEq, Clone)]
struct Look {
    fg: String,
    bg: Option<String>,
    bold: bool,
    dim: bool,
}

fn look(fg: Color, bg: Color, m: Modifier) -> Look {
    let mut fg = hex(fg).unwrap_or_else(|| DEFAULT_FG.to_string());
    let mut bg = hex(bg);
    if m.contains(Modifier::REVERSED) {
        let new_fg = bg.take().unwrap_or_else(|| DEFAULT_BG.to_string());
        bg = Some(fg);
        fg = new_fg;
    }
    Look {
        fg,
        bg,
        bold: m.contains(Modifier::BOLD),
        dim: m.contains(Modifier::DIM),
    }
}

/// The parts of a cell, as fractions of its width and height (x, y, w, h), that a block
/// element character fills. Fonts draw these glyphs unevenly (gaps between rows,
/// thin or missing bars), so the picture draws them as rectangles instead.
/// Shades (light, medium, dark) are not covered and stay as text.
fn block_parts(c: char) -> Option<Vec<(f64, f64, f64, f64)>> {
    let code = c as u32;
    let parts = match code {
        0x2580 => vec![(0.0, 0.0, 1.0, 0.5)],
        0x2581..=0x2588 => {
            let h = f64::from(code - 0x2580) / 8.0;
            vec![(0.0, 1.0 - h, 1.0, h)]
        }
        0x2589..=0x258F => vec![(0.0, 0.0, f64::from(0x2590 - code) / 8.0, 1.0)],
        0x2590 => vec![(0.5, 0.0, 0.5, 1.0)],
        0x2594 => vec![(0.0, 0.0, 1.0, 0.125)],
        0x2595 => vec![(0.875, 0.0, 0.125, 1.0)],
        0x2596..=0x259F => {
            // Bits: upper left, upper right, lower left, lower right.
            let bits: u8 = match code {
                0x2596 => 0b0010,
                0x2597 => 0b0001,
                0x2598 => 0b1000,
                0x2599 => 0b1011,
                0x259A => 0b1001,
                0x259B => 0b1110,
                0x259C => 0b1101,
                0x259D => 0b0100,
                0x259E => 0b0110,
                _ => 0b0111,
            };
            let mut v = Vec::new();
            for (bit, x, y) in [(8u8, 0.0, 0.0), (4, 0.5, 0.0), (2, 0.0, 0.5), (1, 0.5, 0.5)] {
                if bits & bit != 0 {
                    v.push((x, y, 0.5, 0.5));
                }
            }
            v
        }
        _ => return None,
    };
    Some(parts)
}

/// Draw the buffer as an SVG document.
pub fn buffer_to_svg(buf: &Buffer) -> String {
    let cols = buf.area.width as usize;
    let rows = buf.area.height as usize;
    let width = cols as f64 * CELL_W + 24.0;
    let height = rows as f64 * CELL_H + 24.0;
    let mut out = String::new();
    out.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width:.0}\" height=\"{height:.0}\" viewBox=\"0 0 {width:.0} {height:.0}\" role=\"img\" aria-label=\"graft watch screenshot\">\n"
    ));
    out.push_str(&format!(
        "<rect width=\"100%\" height=\"100%\" rx=\"8\" fill=\"{DEFAULT_BG}\"/>\n"
    ));
    out.push_str(&format!(
        "<g font-family=\"DejaVu Sans Mono, Menlo, Consolas, monospace\" font-size=\"{FONT_SIZE}\">\n"
    ));
    for y in 0..rows {
        // Group consecutive cells that look the same.
        let mut runs: Vec<(usize, String, Look)> = Vec::new();
        for x in 0..cols {
            let cell = &buf[(x as u16, y as u16)];
            let l = look(cell.fg, cell.bg, cell.modifier);
            let sym = cell.symbol();
            match runs.last_mut() {
                Some((_, text, prev)) if *prev == l => text.push_str(sym),
                _ => runs.push((x, sym.to_string(), l)),
            }
        }
        let top = 12.0 + y as f64 * CELL_H;
        for (x, text, l) in &runs {
            let n = text.chars().count();
            let px = 12.0 + *x as f64 * CELL_W;
            if let Some(bg) = &l.bg {
                out.push_str(&format!(
                    "<rect x=\"{px:.1}\" y=\"{top:.1}\" width=\"{:.1}\" height=\"{CELL_H}\" fill=\"{bg}\"/>\n",
                    n as f64 * CELL_W
                ));
            }
        }
        for (x, text, l) in &runs {
            let opacity = if l.dim { " fill-opacity=\"0.6\"" } else { "" };
            for (i, c) in text.chars().enumerate() {
                if let Some(parts) = block_parts(c) {
                    let cx = 12.0 + (*x + i) as f64 * CELL_W;
                    for (fx, fy, fw, fh) in parts {
                        out.push_str(&format!(
                            "<rect x=\"{:.2}\" y=\"{:.2}\" width=\"{:.2}\" height=\"{:.2}\" fill=\"{}\"{opacity} shape-rendering=\"crispEdges\"/>\n",
                            cx + fx * CELL_W,
                            top + fy * CELL_H,
                            fw * CELL_W,
                            fh * CELL_H,
                            l.fg
                        ));
                    }
                }
            }
        }
        for (x, text, l) in &runs {
            if text.chars().all(|c| c == ' ' || block_parts(c).is_some()) {
                continue;
            }
            let px = 12.0 + *x as f64 * CELL_W;
            // One x position per character keeps the grid exact, whatever
            // font the viewer falls back to for box drawing characters.
            // Spaces are left out, so no viewer can collapse them and shift
            // the characters after them.
            let (xs, glyphs): (Vec<String>, String) = text
                .chars()
                .enumerate()
                .filter(|(_, c)| *c != ' ' && block_parts(*c).is_none())
                .map(|(i, c)| (format!("{:.1}", px + i as f64 * CELL_W), c))
                .unzip();
            let weight = if l.bold { " font-weight=\"700\"" } else { "" };
            let opacity = if l.dim { " fill-opacity=\"0.6\"" } else { "" };
            out.push_str(&format!(
                "<text x=\"{}\" y=\"{:.1}\" fill=\"{}\"{weight}{opacity}>{}</text>\n",
                xs.join(" "),
                top + 13.0,
                l.fg,
                escape(&glyphs)
            ));
        }
    }
    out.push_str("</g>\n</svg>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;
    use ratatui::style::Style;

    #[test]
    fn draws_text_background_and_escapes() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 2));
        buf.set_string(0, 0, "a<b&c", Style::default().fg(Color::Green));
        buf.set_string(
            0,
            1,
            "sel",
            Style::default().add_modifier(Modifier::REVERSED),
        );
        let svg = buffer_to_svg(&buf);
        assert!(svg.starts_with("<svg "));
        assert!(svg.contains("a&lt;b&amp;c"));
        assert!(svg.contains("fill=\"#a6e3a1\""));
        // Reversed text gets a light background and dark text.
        assert!(svg.contains("<rect x=\"12.0\" y=\"30.0\""));
        assert!(svg.trim_end().ends_with("</svg>"));
        assert!(!svg.contains("http://www.w3.org/1999/xlink"));
    }

    #[test]
    fn is_stable_for_the_same_buffer() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        buf.set_string(0, 0, "hello", Style::default().add_modifier(Modifier::BOLD));
        assert_eq!(buffer_to_svg(&buf), buffer_to_svg(&buf));
        assert!(buffer_to_svg(&buf).contains("font-weight=\"700\""));
    }

    #[test]
    fn block_elements_are_rectangles_not_font_glyphs() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
        buf.set_string(
            0,
            0,
            "\u{2588}\u{2584}\u{258c}\u{2598}ab",
            Style::default().fg(Color::Cyan),
        );
        let svg = buffer_to_svg(&buf);
        for c in ['\u{2588}', '\u{2584}', '\u{258c}', '\u{2598}'] {
            assert!(!svg.contains(c), "{c:?} should be drawn as a rectangle");
        }
        // Full block: a whole cell. Lower half: the bottom 9 of 18 pixels.
        assert!(svg.contains(
            "<rect x=\"12.00\" y=\"12.00\" width=\"8.40\" height=\"18.00\" fill=\"#94e2d5\""
        ));
        assert!(svg.contains("<rect x=\"20.40\" y=\"21.00\" width=\"8.40\" height=\"9.00\""));
        // Left half and one quadrant.
        assert!(svg.contains("<rect x=\"28.80\" y=\"12.00\" width=\"4.20\" height=\"18.00\""));
        assert!(svg.contains("width=\"4.20\" height=\"9.00\""));
        // Letters stay text, and the rectangles leave no blanks in the text.
        assert!(svg.contains(">ab</text>"));
    }

    #[test]
    fn every_eighth_block_gets_its_own_height() {
        for n in 1..=8u32 {
            let c = char::from_u32(0x2580 + n).unwrap();
            let parts = block_parts(c).unwrap();
            assert_eq!(parts.len(), 1);
            assert!((parts[0].3 - f64::from(n) / 8.0).abs() < 1e-9);
            assert!(
                (parts[0].1 + parts[0].3 - 1.0).abs() < 1e-9,
                "sits on the bottom"
            );
        }
        assert!(block_parts('a').is_none());
        assert!(block_parts('\u{2591}').is_none(), "shades stay text");
    }
}
