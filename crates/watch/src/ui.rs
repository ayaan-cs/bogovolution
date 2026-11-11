//! Drawing for `watch`. Takes the state model and the view state and paints a
//! frame. It never reads files or the clock, so the same inputs always give the
//! same screen and tests can render to a `TestBackend`.

use crate::state::{
    pct, points, Details, FeedItem, FeedKind, Progress, Row, RowStatus, WatchState,
};
use crate::view::{ChildStatus, Overlay, Pane, ViewState};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Gauge, List, ListItem, Paragraph, Widget, Wrap};
use ratatui::{Frame, Terminal};
use rules_evolve_core::event::VerdictKind;

pub const MIN_WIDTH: u16 = 70;
pub const MIN_HEIGHT: u16 = 20;

const SPINNER: [&str; 4] = ["|", "/", "-", "\\"];

/// Things the screen shows that do not come from the log.
#[derive(Debug, Clone, Default)]
pub struct Extra {
    pub child: ChildStatus,
}

fn status_color(s: RowStatus) -> Color {
    match s {
        RowStatus::Root => Color::Cyan,
        RowStatus::Pending => Color::Blue,
        RowStatus::Accepted => Color::Green,
        RowStatus::Rejected => Color::Red,
        RowStatus::Noise => Color::Yellow,
        RowStatus::Refused => Color::DarkGray,
    }
}

fn feed_color(k: FeedKind) -> Color {
    match k {
        FeedKind::Verdict(VerdictKind::Accepted) => Color::Green,
        FeedKind::Verdict(VerdictKind::Rejected) => Color::Red,
        FeedKind::Verdict(VerdictKind::Noise) => Color::Yellow,
        FeedKind::Refused => Color::DarkGray,
    }
}

fn pane_block<'a>(title: &'a str, focused: bool) -> Block<'a> {
    let style = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let title_style = if focused {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    Block::default()
        .borders(Borders::ALL)
        .border_style(style)
        .title(Span::styled(format!(" {title} "), title_style))
}

fn selected_style() -> Style {
    Style::default().add_modifier(Modifier::REVERSED)
}

pub fn draw(frame: &mut Frame, state: &WatchState, view: &mut ViewState, extra: &Extra) {
    view.tick = view.tick.wrapping_add(1);
    let area = frame.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        let msg = format!(
            "Terminal too small ({}x{}). rules-evolve watch needs at least {}x{}.",
            area.width, area.height, MIN_WIDTH, MIN_HEIGHT
        );
        frame.render_widget(Paragraph::new(msg).wrap(Wrap { trim: true }), area);
        return;
    }

    let banner_h = if state.chain.valid { 0 } else { 2 };
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(banner_h),
            Constraint::Min(8),
            Constraint::Length(2),
        ])
        .split(area);

    draw_title(frame, rows[0], state);
    if banner_h > 0 {
        draw_banner(frame, rows[1], state);
    }
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(56), Constraint::Percentage(44)])
        .split(rows[2]);
    draw_tree(frame, cols[0], state, view);
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(38),
            Constraint::Length(8),
            Constraint::Min(8),
        ])
        .split(cols[1]);
    draw_details(frame, right[0], state, view);
    draw_progress(frame, right[1], state, view);
    draw_feed(frame, right[2], state, view);
    draw_status(frame, rows[3], state, view, extra);

    if let Some(overlay) = &view.overlay {
        draw_overlay(frame, area, state, overlay);
    }
}

fn draw_title(frame: &mut Frame, area: Rect, state: &WatchState) {
    let mut spans = vec![
        Span::styled(
            " rules-evolve watch ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
    ];
    if state.tree.is_some() {
        spans.push(Span::raw(format!(
            "{}  model {}  ",
            state.rules_file, state.model
        )));
        spans.push(Span::styled(
            format!("{} accepted", state.accepted),
            Style::default().fg(Color::Green),
        ));
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!("{} rejected", state.rejected),
            Style::default().fg(Color::Red),
        ));
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!("{} noise", state.noise),
            Style::default().fg(Color::Yellow),
        ));
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!("{} pending", state.pending),
            Style::default().fg(Color::Blue),
        ));
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!("{} refused", state.refused),
            Style::default().fg(Color::DarkGray),
        ));
    } else {
        spans.push(Span::raw("waiting for the log"));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_banner(frame: &mut Frame, area: Rect, state: &WatchState) {
    let first = state
        .chain
        .problems
        .first()
        .cloned()
        .unwrap_or_else(|| "the log failed verification".to_string());
    let lines = vec![
        Line::from(Span::styled(
            " CHAIN FAILED VERIFICATION. This view is read only and shows the log only up to the first problem. ",
            Style::default()
                .fg(Color::White)
                .bg(Color::Red)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!(" {first}"),
            Style::default().fg(Color::Red),
        )),
    ];
    frame.render_widget(Paragraph::new(lines), area);
}

fn tree_line(row: &Row) -> Line<'static> {
    let color = status_color(row.status);
    let dim = Style::default()
        .fg(Color::DarkGray)
        .add_modifier(Modifier::DIM);
    if row.status == RowStatus::Refused {
        return Line::from(vec![
            Span::styled(row.prefix.clone(), dim),
            Span::styled(format!("  refused  {}", row.title), dim),
        ]);
    }
    let marker = if row.is_current { "*" } else { " " };
    let med = row.median.map(pct).unwrap_or_else(|| "-".to_string());
    let gain = row.gain_pp.map(points).unwrap_or_default();
    let spans = vec![
        Span::styled(row.prefix.clone(), Style::default().fg(Color::DarkGray)),
        Span::styled(
            marker,
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            row.short.clone(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {:<8}", row.status.label()),
            Style::default().fg(color),
        ),
        Span::raw(format!(" {med:>4} {gain:>8}  ")),
        Span::raw(row.title.clone()),
    ];
    Line::from(spans)
}

fn draw_tree(frame: &mut Frame, area: Rect, state: &WatchState, view: &mut ViewState) {
    let focused = view.focus == Pane::Tree;
    let block = pane_block("Mutation tree (* is current)", focused);
    if state.rows.is_empty() {
        let msg = if state.tree.is_none() && !state.chain.valid {
            "No readable events."
        } else {
            "Waiting for the first event in .rules-evolve/log.jsonl."
        };
        frame.render_widget(Paragraph::new(msg).block(block), area);
        return;
    }
    let items: Vec<ListItem> = state
        .rows
        .iter()
        .map(|r| ListItem::new(tree_line(r)))
        .collect();
    let selected = view.selected.as_ref().and_then(|id| state.row_index(id));
    view.tree_list.select(selected);
    let list = List::new(items).block(block).highlight_style(if focused {
        selected_style()
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    });
    frame.render_stateful_widget(list, area, &mut view.tree_list);
}

fn details_lines(d: &Details, full: bool) -> Vec<Line<'static>> {
    let color = status_color(d.status);
    let mut lines = vec![Line::from(vec![
        Span::styled(
            d.title.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            d.status.label().to_string(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        if d.is_current {
            Span::styled("  current", Style::default().fg(Color::Green))
        } else {
            Span::raw("")
        },
    ])];
    if let Some(h) = &d.hypothesis {
        lines.push(Line::from(vec![
            Span::styled("hypothesis ", Style::default().fg(Color::DarkGray)),
            Span::raw(h.clone()),
        ]));
    }
    if let Some(r) = &d.reason {
        lines.push(Line::from(vec![
            Span::styled("verdict    ", Style::default().fg(Color::DarkGray)),
            Span::styled(r.clone(), Style::default().fg(color)),
        ]));
    }
    for (k, v) in &d.fields {
        lines.push(Line::from(vec![
            Span::styled(format!("{k:<17}"), Style::default().fg(Color::DarkGray)),
            Span::raw(v.clone()),
        ]));
    }
    if full && !d.runs.is_empty() {
        lines.push(Line::from(""));
        for r in &d.runs {
            lines.push(Line::from(Span::raw(r.clone())));
        }
    }
    lines
}

fn draw_details(frame: &mut Frame, area: Rect, state: &WatchState, view: &ViewState) {
    let block = pane_block("Details", view.focus == Pane::Details);
    let details = view.selected.as_ref().and_then(|id| state.details(id));
    let para = match details {
        Some(d) => Paragraph::new(details_lines(&d, true))
            .wrap(Wrap { trim: false })
            .scroll((view.details_scroll, 0)),
        None => Paragraph::new("Select a version in the tree to see what changed and why."),
    };
    frame.render_widget(para.block(block), area);
}

fn progress_text(p: &Progress, tick: u64) -> String {
    let what = p.hypothesis.as_deref().unwrap_or("starting rules");
    let state = if p.settled {
        "judged".to_string()
    } else if p.runs_done >= p.repeats {
        "runs done, waiting for verify".to_string()
    } else {
        format!("benchmarking {}", SPINNER[(tick / 3 % 4) as usize])
    };
    format!("{} {}  {}: {}", p.role.label(), p.version, state, what)
}

fn draw_progress(frame: &mut Frame, area: Rect, state: &WatchState, view: &ViewState) {
    let block = pane_block("Run progress", false);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let Some(p) = &state.progress else {
        let msg = if state.is_fresh() {
            "No runs yet. Start `rules-evolve evolve --generations N` in another terminal."
        } else {
            "Waiting for the first run."
        };
        frame.render_widget(Paragraph::new(msg).wrap(Wrap { trim: true }), inner);
        return;
    };
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Min(1),
        ])
        .split(inner);
    frame.render_widget(Paragraph::new(progress_text(p, view.tick)), rows[0]);
    let gauge = Gauge::default()
        .ratio(p.ratio())
        .label(format!("runs {} of {}", p.runs_done, p.repeats))
        .gauge_style(Style::default().fg(Color::Green).bg(Color::DarkGray));
    frame.render_widget(gauge, rows[1]);
    frame.render_widget(RunBars::new(&p.rates, p.repeats), rows[2]);
    let rates: Vec<String> = p.rates.iter().map(|r| pct(*r)).collect();
    if !rates.is_empty() {
        frame.render_widget(
            Paragraph::new(format!("pass rate per run: {}", rates.join(" "))),
            rows[3],
        );
    }
}

/// Width of one bar and the gap after it, in columns.
const BAR_W: u16 = 3;
const BAR_GAP: u16 = 1;
/// Fill characters, from one eighth to a full cell.
const EIGHTHS: [&str; 8] = [
    "\u{2581}", "\u{2582}", "\u{2583}", "\u{2584}", "\u{2585}", "\u{2586}", "\u{2587}", "\u{2588}",
];

/// One bar per run on a fixed scale of 0 to 100 percent, two rows tall (sixteen
/// levels). Equal rates give equal bars, and 100% is two full rows. A run that
/// scored 0% shows a thin red line, a run still to come shows a dim dot, so the
/// row always says how many runs are expected and how many are in.
pub struct RunBars<'a> {
    rates: &'a [f64],
    slots: usize,
}

impl<'a> RunBars<'a> {
    pub fn new(rates: &'a [f64], repeats: u32) -> Self {
        RunBars {
            rates,
            slots: (repeats as usize).max(rates.len()),
        }
    }

    /// Sixteen levels for a rate from 0 to 1, rounded to the nearest.
    fn level(rate: f64) -> u16 {
        (rate.clamp(0.0, 1.0) * 16.0).round() as u16
    }
}

impl Widget for RunBars<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let bottom = area.y + area.height - 1;
        let has_top = area.height >= 2;
        for slot in 0..self.slots {
            let x0 = area.x + slot as u16 * (BAR_W + BAR_GAP);
            if x0 + BAR_W > area.x + area.width {
                break;
            }
            let Some(rate) = self.rates.get(slot) else {
                // Not run yet: one dim dot in the middle of the slot.
                buf[(x0 + BAR_W / 2, bottom)]
                    .set_symbol("\u{b7}")
                    .set_style(Style::default().fg(Color::DarkGray));
                continue;
            };
            let level = Self::level(*rate);
            let (low, high) = if has_top {
                (level.min(8), level.saturating_sub(8))
            } else {
                (level.div_ceil(2), 0)
            };
            let style = Style::default().fg(Color::Cyan);
            for dx in 0..BAR_W {
                if level == 0 {
                    buf[(x0 + dx, bottom)]
                        .set_symbol(EIGHTHS[0])
                        .set_style(Style::default().fg(Color::Red));
                    continue;
                }
                if low > 0 {
                    buf[(x0 + dx, bottom)]
                        .set_symbol(EIGHTHS[low as usize - 1])
                        .set_style(style);
                }
                if has_top && high > 0 {
                    buf[(x0 + dx, bottom - 1)]
                        .set_symbol(EIGHTHS[high as usize - 1])
                        .set_style(style);
                }
            }
        }
    }
}

fn feed_line(f: &FeedItem) -> Line<'static> {
    let color = feed_color(f.kind);
    let label = match f.kind {
        FeedKind::Verdict(v) => v.as_str().to_uppercase(),
        FeedKind::Refused => "REFUSED".to_string(),
    };
    let numbers = match f.kind {
        FeedKind::Verdict(_) => {
            let g = f.gain_pp.map(points).unwrap_or_else(|| "n/a".to_string());
            let w = match (f.paired_wins, f.repeats) {
                (Some(w), Some(n)) => format!("{w}/{n}"),
                _ => "-".to_string(),
            };
            format!("{g:>8} {w:>4}")
        }
        FeedKind::Refused => " ".repeat(13),
    };
    let style = if f.kind == FeedKind::Refused {
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::DIM)
    } else {
        Style::default().fg(color)
    };
    Line::from(vec![
        Span::styled(format!("{label:<8}"), style.add_modifier(Modifier::BOLD)),
        Span::styled(format!(" {numbers}  "), style),
        Span::raw(f.hypothesis.clone()),
    ])
}

fn draw_feed(frame: &mut Frame, area: Rect, state: &WatchState, view: &mut ViewState) {
    let focused = view.focus == Pane::Feed;
    let block = pane_block("Verdict feed", focused);
    if state.feed.is_empty() {
        frame.render_widget(
            Paragraph::new("Verdicts show up here as verify finishes.").block(block),
            area,
        );
        return;
    }
    let items: Vec<ListItem> = state
        .feed
        .iter()
        .map(|f| ListItem::new(feed_line(f)))
        .collect();
    let selected = view
        .selected
        .as_ref()
        .and_then(|id| state.feed.iter().position(|f| &f.target == id));
    view.feed_list.select(selected);
    let list = List::new(items).block(block).highlight_style(if focused {
        selected_style()
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    });
    frame.render_stateful_widget(list, area, &mut view.feed_list);
}

fn group(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn draw_status(frame: &mut Frame, area: Rect, state: &WatchState, view: &ViewState, extra: &Extra) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(area);

    let mut spans: Vec<Span> = Vec::new();
    if state.chain.valid {
        spans.push(Span::styled(
            " chain OK ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ));
    } else {
        spans.push(Span::styled(
            " CHAIN INVALID ",
            Style::default()
                .fg(Color::White)
                .bg(Color::Red)
                .add_modifier(Modifier::BOLD),
        ));
    }
    spans.push(Span::raw(format!(
        " {} events  tokens {} (biggest run {} of {} cap)  ",
        state.chain.events,
        group(state.tokens_total),
        group(state.tokens_max_run),
        group(state.config.max_tokens_per_run),
    )));
    match &extra.child {
        ChildStatus::None => {}
        ChildStatus::Running(label) => spans.push(Span::styled(
            format!("running: {label} {}", SPINNER[(view.tick / 3 % 4) as usize]),
            Style::default().fg(Color::Yellow),
        )),
        ChildStatus::Exited { ok, text } => spans.push(Span::styled(
            text.clone(),
            Style::default().fg(if *ok { Color::Green } else { Color::Red }),
        )),
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), rows[0]);

    let key = Style::default().add_modifier(Modifier::BOLD);
    let mut help: Vec<Span> = Vec::new();
    if view.quit_armed && extra.child.is_running() {
        help.push(Span::styled(
            " evolve is still running. Press q again to stop it and quit, or any other key to keep watching.",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ));
    } else {
        for (k, what) in [
            ("up/down j/k", "move"),
            ("enter", "details"),
            ("tab", "pane"),
            ("f", if view.follow { "following" } else { "follow" }),
            ("q", "quit"),
        ] {
            help.push(Span::styled(format!(" {k}"), key));
            help.push(Span::styled(
                format!(" {what} "),
                Style::default().fg(Color::DarkGray),
            ));
        }
        if !state.chain.valid {
            help.push(Span::styled(
                " read only",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ));
        }
    }
    frame.render_widget(Paragraph::new(Line::from(help)), rows[1]);
}

fn centered(area: Rect, w_pct: u16, h_pct: u16) -> Rect {
    let w = area.width * w_pct / 100;
    let h = area.height * h_pct / 100;
    Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    }
}

fn diff_line(l: &str) -> Line<'static> {
    let style = if l.starts_with("+++") || l.starts_with("---") || l.starts_with("diff ") {
        Style::default().add_modifier(Modifier::BOLD)
    } else if l.starts_with('+') {
        Style::default().fg(Color::Green)
    } else if l.starts_with('-') {
        Style::default().fg(Color::Red)
    } else if l.starts_with("@@") {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default()
    };
    Line::from(Span::styled(l.to_string(), style))
}

fn draw_overlay(frame: &mut Frame, area: Rect, state: &WatchState, overlay: &Overlay) {
    let rect = centered(area, 92, 90);
    frame.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            " Details (esc or enter to close, j/k to scroll) ",
            Style::default().add_modifier(Modifier::BOLD),
        ));
    let mut lines = match state.details(&overlay.id) {
        Some(d) => details_lines(&d, true),
        None => vec![Line::from("This entry is no longer in the log.")],
    };
    if !overlay.diff.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Change from the parent",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        lines.extend(overlay.diff.iter().map(|l| diff_line(l)));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((overlay.scroll, 0)),
        rect,
    );
}

/// Render one frame into a buffer. Used by tests and by `watch --snapshot`.
pub fn render_to_buffer(
    state: &WatchState,
    view: &mut ViewState,
    extra: &Extra,
    width: u16,
    height: u16,
) -> Buffer {
    let mut terminal =
        Terminal::new(TestBackend::new(width, height)).expect("test backend never fails");
    terminal
        .draw(|f| draw(f, state, view, extra))
        .expect("drawing to a test backend never fails");
    terminal.backend().buffer().clone()
}

/// The buffer as plain text, one line per row, trailing spaces trimmed.
pub fn buffer_to_text(buf: &Buffer) -> String {
    let mut out = String::new();
    for y in 0..buf.area.height {
        let mut line = String::new();
        for x in 0..buf.area.width {
            line.push_str(buf[(x, y)].symbol());
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{ChainStatus, ConfigView};
    use crate::testkit::*;
    use rules_evolve_core::event::VerdictKind;
    use std::path::PathBuf;

    const W: u16 = 110;
    const H: u16 = 32;

    /// Compare with `snapshots/<name>.txt`. Run with `UPDATE_SNAPSHOTS=1` to
    /// write or refresh the files, then read the diff before committing.
    fn check(name: &str, text: &str) {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("snapshots")
            .join(format!("{name}.txt"));
        if std::env::var("UPDATE_SNAPSHOTS").is_ok() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, text).unwrap();
            return;
        }
        let want = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("missing {}. Run with UPDATE_SNAPSHOTS=1", path.display()));
        assert_eq!(
            text, want,
            "snapshot {name} changed. Run with UPDATE_SNAPSHOTS=1 to refresh"
        );
    }

    fn render(
        payloads: &[rules_evolve_core::event::Payload],
        extra: &Extra,
    ) -> (String, ViewState) {
        let s = build(payloads);
        let mut v = ViewState::default();
        v.sync(&s);
        let buf = render_to_buffer(&s, &mut v, extra, W, H);
        (buffer_to_text(&buf), v)
    }

    #[test]
    fn empty_lineage() {
        let (text, _) = render(&[init()], &Extra::default());
        assert!(text.contains("No runs yet."));
        assert!(text.contains("in another terminal."));
        assert!(text.contains("chain OK"));
        assert!(text.contains("starting rules"));
        check("empty", &text);
    }

    #[test]
    fn nothing_in_the_log_yet() {
        let s = WatchState::build(&[], ChainStatus::ok(0), ConfigView::default());
        let mut v = ViewState::default();
        v.sync(&s);
        let text = buffer_to_text(&render_to_buffer(&s, &mut v, &Extra::default(), W, H));
        assert!(text.contains("Waiting for the first event"));
        assert!(text.contains("waiting for the log"));
        assert!(text.contains("Verdicts show up here as verify finishes."));
    }

    #[test]
    fn mid_run_shows_progress_and_a_running_child() {
        let payloads = vec![
            init(),
            run('a', 0, 3),
            run('a', 1, 4),
            run('a', 2, 3),
            run('a', 3, 5),
            run('a', 4, 4),
            proposed('a', 'b', "Run the test suite before finishing."),
            run('b', 0, 6),
            run('b', 1, 7),
        ];
        let extra = Extra {
            child: ChildStatus::Running("evolve --generations 3".into()),
        };
        let (text, _) = render(&payloads, &extra);
        assert!(text.contains("runs 2 of 5"));
        assert!(text.contains("child bbbbbbb"));
        assert!(text.contains("pass rate per run: 60% 70%"));
        assert!(text.contains("running: evolve --generations 3"));
        assert!(text.contains("pending"));
        check("mid_run", &text);
    }

    #[test]
    fn finished_lineage_with_every_outcome() {
        let (text, _) = render(&story(), &Extra::default());
        for want in [
            "1 accepted",
            "1 rejected",
            "1 noise",
            "1 pending",
            "3 refused",
            "ACCEPTED",
            "REJECTED",
            "NOISE",
            "REFUSED",
            "+50 pts",
            "5/5",
            "current",
        ] {
            assert!(text.contains(want), "missing {want:?}\n{text}");
        }
        check("finished", &text);
    }

    #[test]
    fn broken_chain_is_loud_and_read_only() {
        let ev = events(&story());
        let chain = ChainStatus {
            valid: false,
            events: 4,
            problems: vec!["event 5 does not link to the one before it (line 6)".into()],
        };
        let s = WatchState::build(&ev[..4], chain, ConfigView::default());
        let mut v = ViewState::default();
        v.sync(&s);
        let text = buffer_to_text(&render_to_buffer(&s, &mut v, &Extra::default(), W, H));
        assert!(text.contains("CHAIN FAILED VERIFICATION"));
        assert!(text.contains("read only"));
        assert!(text.contains("CHAIN INVALID"));
        assert!(text.contains("does not link to the one before it"));
        check("broken_chain", &text);
    }

    #[test]
    fn broken_chain_uses_red_styles() {
        let chain = ChainStatus {
            valid: false,
            events: 1,
            problems: vec!["bad".into()],
        };
        let ev = events(&[init()]);
        let s = WatchState::build(&ev, chain, ConfigView::default());
        let mut v = ViewState::default();
        let buf = render_to_buffer(&s, &mut v, &Extra::default(), W, H);
        assert_eq!(buf[(1, 1)].bg, Color::Red);
    }

    #[test]
    fn details_overlay_shows_the_diff() {
        let s = build(&story());
        let mut v = ViewState::default();
        v.sync(&s);
        v.selected = Some(crate::state::RowId::Version(h('b')));
        v.overlay = Some(Overlay {
            id: crate::state::RowId::Version(h('b')),
            scroll: 0,
            diff: vec![
                "diff --git a/AGENTS.md b/AGENTS.md".into(),
                "@@ -1,2 +1,3 @@".into(),
                " keep".into(),
                "-old line".into(),
                "+new line".into(),
            ],
        });
        let buf = render_to_buffer(&s, &mut v, &Extra::default(), W, H);
        let text = buffer_to_text(&buf);
        assert!(text.contains("Change from the parent"));
        assert!(text.contains("+new line"));
        assert!(text.contains("-old line"));
        assert!(text.contains("esc or enter to close"));
        check("overlay", &text);
    }

    #[test]
    fn selection_is_reversed_in_the_focused_pane() {
        let s = build(&story());
        let mut v = ViewState::default();
        v.sync(&s);
        let buf = render_to_buffer(&s, &mut v, &Extra::default(), W, H);
        let reversed = (0..H)
            .flat_map(|y| (0..W).map(move |x| (x, y)))
            .filter(|(x, y)| buf[(*x, *y)].modifier.contains(Modifier::REVERSED))
            .count();
        assert!(reversed > 20, "the selected tree row is highlighted");
    }

    #[test]
    fn quit_warning_replaces_the_hint_while_a_child_runs() {
        let s = build(&story());
        let mut v = ViewState::default();
        v.sync(&s);
        v.quit_armed = true;
        let extra = Extra {
            child: ChildStatus::Running("evolve".into()),
        };
        let text = buffer_to_text(&render_to_buffer(&s, &mut v, &extra, W, H));
        assert!(text.contains("Press q again to stop it and quit"));
    }

    #[test]
    fn exited_child_is_reported() {
        let s = build(&story());
        let mut v = ViewState::default();
        v.sync(&s);
        let extra = Extra {
            child: ChildStatus::Exited {
                ok: false,
                text: "evolve failed (exit 1) replay miss".into(),
            },
        };
        let text = buffer_to_text(&render_to_buffer(&s, &mut v, &extra, W, H));
        assert!(text.contains("evolve failed (exit 1) replay miss"));
    }

    #[test]
    fn tiny_terminals_get_a_plain_message() {
        let s = build(&story());
        let mut v = ViewState::default();
        let text = buffer_to_text(&render_to_buffer(&s, &mut v, &Extra::default(), 40, 10));
        assert!(text.contains("Terminal too small"));
    }

    #[test]
    fn no_dashes_in_any_rendered_text() {
        let s = build(&story());
        let mut v = ViewState::default();
        v.sync(&s);
        let text = buffer_to_text(&render_to_buffer(&s, &mut v, &Extra::default(), W, H));
        assert!(!text.contains('\u{2013}') && !text.contains('\u{2014}'));
    }

    #[test]
    fn group_adds_thousands_separators() {
        assert_eq!(group(0), "0");
        assert_eq!(group(999), "999");
        assert_eq!(group(1000), "1,000");
        assert_eq!(group(200_000), "200,000");
        assert_eq!(group(1_234_567), "1,234,567");
    }

    #[test]
    fn verdict_kind_is_imported_for_colors() {
        assert_eq!(
            feed_color(FeedKind::Verdict(VerdictKind::Accepted)),
            Color::Green
        );
    }

    // ---- run bars ----

    fn bars(rates: &[f64], repeats: u32, w: u16) -> Vec<String> {
        let area = Rect::new(0, 0, w, 2);
        let mut buf = Buffer::empty(area);
        RunBars::new(rates, repeats).render(area, &mut buf);
        (0..2)
            .map(|y| {
                (0..w)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn equal_rates_give_equal_full_height_bars() {
        let rows = bars(&[1.0; 5], 5, 20);
        assert_eq!(rows[0], "\u{2588}\u{2588}\u{2588} \u{2588}\u{2588}\u{2588} \u{2588}\u{2588}\u{2588} \u{2588}\u{2588}\u{2588} \u{2588}\u{2588}\u{2588} ");
        assert_eq!(rows[0], rows[1], "100% fills both rows");
        // Equal middling rates are equal too, never a mix of heights.
        let half = bars(&[0.5; 3], 3, 12);
        assert_eq!(half[0].trim(), "");
        assert_eq!(
            half[1],
            "\u{2588}\u{2588}\u{2588} \u{2588}\u{2588}\u{2588} \u{2588}\u{2588}\u{2588} "
        );
        let a = bars(&[0.4; 4], 4, 16);
        let cells: Vec<&str> = a[1].split(' ').filter(|s| !s.is_empty()).collect();
        assert!(cells.windows(2).all(|w| w[0] == w[1]));
    }

    #[test]
    fn bars_use_a_fixed_zero_to_one_hundred_scale() {
        // 25% is a quarter of two rows: half of the bottom row.
        let q = bars(&[0.25], 1, 4);
        assert_eq!(q[0].trim(), "");
        assert_eq!(q[1], "\u{2584}\u{2584}\u{2584} ");
        // 75% fills the bottom row and half of the top one.
        let t = bars(&[0.75], 1, 4);
        assert_eq!(t[0], "\u{2584}\u{2584}\u{2584} ");
        assert_eq!(t[1], "\u{2588}\u{2588}\u{2588} ");
        // A lone bar does not stretch to fill: 40% stays low even when it is the only data.
        let l = bars(&[0.4], 1, 4);
        assert_eq!(l[0].trim(), "");
        // Out of range values are clamped, not drawn outside.
        assert_eq!(bars(&[3.0], 1, 4), bars(&[1.0], 1, 4));
        assert_eq!(bars(&[-1.0], 1, 4), bars(&[0.0], 1, 4));
    }

    #[test]
    fn zero_percent_is_a_thin_line_and_unrun_slots_are_dots() {
        let rows = bars(&[0.0, 1.0], 4, 16);
        assert_eq!(
            rows[1].chars().take(3).collect::<String>(),
            "\u{2581}\u{2581}\u{2581}"
        );
        assert_eq!(
            rows[1].chars().nth(9).unwrap(),
            '\u{b7}',
            "run 3 has not happened"
        );
        assert_eq!(rows[1].chars().nth(13).unwrap(), '\u{b7}');
        // The zero run and the not yet run slot look different.
        assert_ne!(rows[1].chars().nth(1), rows[1].chars().nth(9));
    }

    #[test]
    fn empty_state_is_a_row_of_dots() {
        let rows = bars(&[], 5, 20);
        assert_eq!(rows[0].trim(), "");
        assert_eq!(rows[1].matches('\u{b7}').count(), 5);
        assert!(!rows[1].contains('\u{2581}'));
        // Nothing at all is fine too, and a tiny area does not panic.
        assert_eq!(bars(&[], 0, 4)[1].trim(), "");
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 1));
        RunBars::new(&[1.0, 1.0], 2).render(Rect::new(0, 0, 2, 1), &mut buf);
        RunBars::new(&[1.0], 1).render(Rect::new(0, 0, 0, 0), &mut buf);
    }

    #[test]
    fn all_runs_scoring_the_same_render_cleanly_in_the_whole_screen() {
        let mut payloads = vec![init()];
        payloads.push(proposed('a', 'b', "Run the test suite before finishing."));
        for i in 0..5 {
            payloads.push(run('b', i, 10));
        }
        let (text, _) = render(&payloads, &Extra::default());
        assert!(text.contains("pass rate per run: 100% 100% 100% 100% 100%"));
        let full = "\u{2588}\u{2588}\u{2588} ".repeat(5);
        // Five equal bars, two rows tall, and nothing else drawn in that area.
        assert_eq!(text.matches(full.trim_end()).count(), 2, "{text}");
        check("all_runs_equal", &text);
    }
}
