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

/// Draw the buffer as an SVG document.
pub fn buffer_to_svg(buf: &Buffer) -> String {
    let cols = buf.area.width as usize;
    let rows = buf.area.height as usize;
    let width = cols as f64 * CELL_W + 24.0;
    let height = rows as f64 * CELL_H + 24.0;
    let mut out = String::new();
    out.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width:.0}\" height=\"{height:.0}\" viewBox=\"0 0 {width:.0} {height:.0}\" role=\"img\" aria-label=\"rules-evolve watch screenshot\">\n"
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
            if text.trim().is_empty() {
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
                .filter(|(_, c)| *c != ' ')
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
}
