use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Cell, Paragraph, Row, Table, TableState};

use crate::app::{App, Mode};

/// Herdr's Braille spinner for agents that are actively working.
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn draw(frame: &mut Frame, app: &mut App) {
    let [picker_area, preview_area, footer_area] = areas(frame.area());

    draw_picker(frame, app, picker_area);
    draw_preview(frame, app, preview_area);
    draw_footer(frame, app, footer_area);
}

fn areas(area: Rect) -> [Rect; 3] {
    let [main_area, footer_area] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
    let [picker_area, preview_area] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
            .areas(main_area);
    [picker_area, preview_area, footer_area]
}

pub fn preview_dimensions(area: Rect) -> (u16, u16) {
    let [_, preview_area, _] = areas(area);
    let inner = Block::bordered().inner(preview_area);
    (inner.width.max(1), inner.height.max(1))
}

/// Left panel: filter input and agent list inside a single block, so the
/// picker reads as one surface instead of two stacked boxes.
fn draw_picker(frame: &mut Frame, app: &mut App, area: Rect) {
    let count = format!(" {}/{} ", app.filtered.len(), app.agents.len());
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().dim())
        .title(" Agents ")
        .title(Line::from(count).right_aligned().dim());
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [input_area, rule_area, list_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .areas(inner);

    draw_input(frame, app, input_area);
    frame.render_widget(
        Block::new()
            .borders(Borders::TOP)
            .border_style(Style::new().dim()),
        rule_area,
    );
    draw_list(frame, app, list_area);
}

fn draw_input(frame: &mut Frame, app: &App, area: Rect) {
    let line = match app.mode {
        Mode::Search => Line::from(vec![
            Span::styled("❯ ", Style::new().cyan().bold()),
            Span::raw(app.filter.as_str()),
        ]),
        Mode::Navigate => Line::from(vec![
            Span::styled("❯ ", Style::new().dim()),
            Span::styled("/ to filter", Style::new().dim().italic()),
        ]),
    };
    frame.render_widget(Paragraph::new(line), area);
    if app.mode == Mode::Search {
        let filter_width = u16::try_from(app.filter.chars().count()).unwrap_or(u16::MAX);
        frame.set_cursor_position(Position::new(
            area.x.saturating_add(2).saturating_add(filter_width),
            area.y,
        ));
    }
}

fn draw_list(frame: &mut Frame, app: &mut App, area: Rect) {
    if area.is_empty() {
        return;
    }
    if app.agents.is_empty() {
        empty_state(frame, area, "No agent panes found", "r reload · q close");
        return;
    }
    if app.filtered.is_empty() {
        let message = format!("No matches for \"{}\"", app.filter);
        empty_state(frame, area, &message, "esc clears the filter");
        return;
    }

    let height = usize::from(area.height);
    let last = app.filtered.len() - 1;
    let selected = app.table_state.selected().map(|index| index.min(last));
    let mut start = app.table_state.offset().min(last);
    if let Some(selected) = selected {
        if selected < start {
            start = selected;
        } else if selected >= start + height {
            start = selected + 1 - height;
        }
    }
    let end = (start + height).min(app.filtered.len());
    app.table_state.select(selected);
    *app.table_state.offset_mut() = start;
    let mut table_state = TableState::new()
        .with_selected(selected.map(|index| index - start))
        .with_selected_column(app.table_state.selected_column());

    let tick = app.spinner_tick();
    let rows = app.filtered[start..end]
        .iter()
        .enumerate()
        .map(|(row, &i)| {
            let agent = &app.agents[i];
            let kind = agent.kind();
            let label = app.label(i);
            let location = app.location(i);
            let cwd = app.cwd(i);

            let indices = app.highlights(start + row);
            let label_offset = kind.chars().count() + 1;
            let location_offset = label_offset + label.chars().count() + 1;
            let cwd_offset = location_offset + location.chars().count() + 1;

            let mut label_line = highlight_line(label, label_offset, indices, Style::new());
            if agent.focused {
                label_line.push_span(Span::styled(" (focused)", Style::new().dim()));
            }

            Row::new(vec![
                Cell::from(Span::styled(
                    status_glyph(agent.status(), tick),
                    status_style(agent.status()),
                )),
                Cell::from(highlight_line(
                    location,
                    location_offset,
                    indices,
                    Style::new(),
                )),
                Cell::from(highlight_line(kind, 0, indices, Style::new().dim())),
                Cell::from(label_line),
                Cell::from(highlight_line(cwd, cwd_offset, indices, Style::new().dim())),
            ])
        });

    let table = Table::new(
        rows,
        [
            Constraint::Length(1),
            Constraint::Fill(2),
            Constraint::Length(6),
            Constraint::Fill(3),
            Constraint::Fill(2),
        ],
    )
    .column_spacing(1)
    .row_highlight_style(selected_row_style());

    frame.render_stateful_widget(table, area, &mut table_state);
    app.table_state.select_column(table_state.selected_column());
}

fn selected_row_style() -> Style {
    Style::new().bg(Color::DarkGray)
}

fn draw_preview(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().dim());
    if let Some(index) = app.selected_index() {
        let agent = &app.agents[index];
        let title = Line::from(vec![
            Span::raw(" "),
            Span::styled(
                status_glyph(agent.status(), app.spinner_tick()),
                status_style(agent.status()),
            ),
            Span::raw(format!(" {} · {} ", agent.kind(), agent.status())),
        ]);
        let cwd = Line::from(format!(" {} ", app.cwd(index)))
            .right_aligned()
            .dim();
        let block = block.title(title).title_bottom(cwd);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        frame.render_widget(&app.preview, inner);
    } else {
        let inner = block.inner(area);
        frame.render_widget(block.title(Line::from(" Preview ").dim()), area);
        empty_state(frame, inner, "Nothing to preview", "");
    }
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    let line = match app.error.as_ref().or(app.preview_error.as_ref()) {
        Some(error) => Line::from(vec![
            Span::styled(" ✗ ", Style::new().fg(Color::Red).bold()),
            Span::styled(error.as_str(), Style::new().fg(Color::Red)),
        ]),
        None => match app.mode {
            Mode::Navigate => hint_line(&[
                ("enter", "focus"),
                ("j/k", "move"),
                ("/", "filter"),
                ("r", "reload"),
                ("q", "close"),
            ]),
            Mode::Search => hint_line(&[
                ("enter", "focus"),
                ("esc", "cancel"),
                ("↑/↓", "move"),
                ("^u", "clear"),
                ("^w", "delete word"),
            ]),
        },
    };
    frame.render_widget(Paragraph::new(line), area);
}

/// Key hints: keys at normal brightness, actions dimmed, so the keys are what
/// the eye lands on when scanning the footer.
fn hint_line(hints: &[(&'static str, &'static str)]) -> Line<'static> {
    let mut spans = Vec::with_capacity(hints.len() * 4 + 1);
    spans.push(Span::raw(" "));
    for (i, &(key, action)) in hints.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::raw(key));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(action, Style::new().dim()));
    }
    Line::from(spans)
}

/// Centered placeholder for empty list/preview areas, with an optional dimmed
/// hint on how to get out of the state.
fn empty_state(frame: &mut Frame, area: Rect, message: &str, hint: &str) {
    let mut lines = vec![Line::raw(""); usize::from(area.height / 3)];
    lines.push(Line::from(message));
    if !hint.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::from(hint).dim());
    }
    frame.render_widget(Paragraph::new(lines).centered(), area);
}

/// Render `text` with fuzzy-matched chars emphasized. `offset` is where
/// `text` starts (in chars) inside the string `indices` was computed against.
/// Spans borrow byte ranges of `text`, so this never copies the string.
fn highlight_line<'a>(text: &'a str, offset: usize, indices: &[usize], base: Style) -> Line<'a> {
    if indices.is_empty() {
        return Line::from(Span::styled(text, base));
    }
    let matched_style = Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let style_for = |matched: bool| if matched { matched_style } else { base };

    let mut spans: Vec<Span<'a>> = Vec::new();
    let mut run_start = 0;
    let mut run_matched = false;
    for (i, (byte_start, _)) in text.char_indices().enumerate() {
        let matched = indices.binary_search(&(offset + i)).is_ok();
        if i == 0 {
            run_matched = matched;
        } else if matched != run_matched {
            spans.push(Span::styled(
                &text[run_start..byte_start],
                style_for(run_matched),
            ));
            run_start = byte_start;
            run_matched = matched;
        }
    }
    if !text.is_empty() {
        spans.push(Span::styled(&text[run_start..], style_for(run_matched)));
    }
    Line::from(spans)
}

/// Mirrors Herdr's built-in Agents sidebar `state_icon` token.
fn status_glyph(status: &str, tick: usize) -> &'static str {
    match status {
        "working" => SPINNER[tick % SPINNER.len()],
        "blocked" => "◉",
        "done" => "●",
        "idle" => "✓",
        _ => "○",
    }
}

fn status_style(status: &str) -> Style {
    let color = match status {
        "working" => Color::Yellow,
        "blocked" => Color::Red,
        "done" => Color::Cyan,
        "idle" => Color::Green,
        _ => Color::DarkGray,
    };
    Style::new().fg(color)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::herdr::parse_agent_list;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::text::Text;

    fn sample_app() -> App {
        let agents = parse_agent_list(
            r#"{"result":{"agents":[
                {"agent":"claude","agent_status":"working","cwd":"/w/dotfiles","focused":true,
                 "pane_id":"w1:p1","terminal_id":"term_1","terminal_title_stripped":"dotfiles work"},
                {"agent":"codex","agent_status":"idle","cwd":"/w/dealon",
                 "pane_id":"w2:p1","terminal_id":"term_2","terminal_title_stripped":"dealon review"}
            ]}}"#,
        )
        .unwrap();
        let mut app = App::new(None, None);
        app.set_agents(agents);
        app
    }

    fn render(app: &mut App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let frame = terminal.draw(|frame| draw(frame, app)).unwrap();
        frame
            .buffer
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect()
    }

    fn numbered_app() -> App {
        let agents: Vec<_> = (0..8)
            .map(|index| {
                serde_json::json!({
                    "agent": "codex",
                    "agent_status": "idle",
                    "pane_id": format!("w1:p{index}"),
                    "terminal_title_stripped": format!("row-{index:02}")
                })
            })
            .collect();
        let response = serde_json::json!({"result": {"agents": agents}});
        let mut app = App::new(None, None);
        app.set_agents(parse_agent_list(&response.to_string()).unwrap());
        app
    }

    fn render_list(app: &mut App, height: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(100, height)).unwrap();
        let frame = terminal
            .draw(|frame| {
                let area = frame.area();
                draw_list(frame, app, area);
            })
            .unwrap();
        frame
            .buffer
            .content
            .chunks(100)
            .map(|row| {
                let line: String = row.iter().map(ratatui::buffer::Cell::symbol).collect();
                line.split_whitespace()
                    .find(|word| word.starts_with("row-"))
                    .unwrap_or("")
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn list_scrolls_and_wraps_with_the_selected_agent() {
        let mut app = numbered_app();
        assert_eq!(render_list(&mut app, 3), ["row-00", "row-01", "row-02"]);
        for _ in 0..4 {
            app.move_selection(1);
        }
        assert_eq!(render_list(&mut app, 3), ["row-02", "row-03", "row-04"]);
        assert_eq!(app.table_state.offset(), 2);
        for _ in 0..4 {
            app.move_selection(1);
        }
        assert_eq!(render_list(&mut app, 3), ["row-00", "row-01", "row-02"]);
        app.move_selection(-1);
        assert_eq!(render_list(&mut app, 3), ["row-05", "row-06", "row-07"]);
        assert_eq!(app.table_state.selected(), Some(7));
        assert_eq!(app.table_state.offset(), 5);
    }

    #[test]
    fn resizing_keeps_the_offset_until_selection_requires_scrolling() {
        let mut app = numbered_app();
        app.table_state.select(Some(7));
        assert_eq!(render_list(&mut app, 3), ["row-05", "row-06", "row-07"]);
        assert_eq!(
            render_list(&mut app, 5),
            ["row-05", "row-06", "row-07", "", ""]
        );
        assert_eq!(render_list(&mut app, 2), ["row-06", "row-07"]);
        app.move_selection(-1);
        app.move_selection(-1);
        assert_eq!(render_list(&mut app, 2), ["row-05", "row-06"]);
        assert_eq!(app.table_state.offset(), 5);
    }

    #[test]
    fn empty_list_area_preserves_selection_and_offset() {
        let mut app = numbered_app();
        app.table_state = TableState::new()
            .with_selected(Some(99))
            .with_offset(99)
            .with_selected_column(Some(99));
        let state = app.table_state.clone();
        let mut terminal = Terminal::new(TestBackend::new(100, 1)).unwrap();
        for area in [Rect::new(0, 0, 0, 1), Rect::new(0, 0, 100, 0)] {
            terminal
                .draw(|frame| draw_list(frame, &mut app, area))
                .unwrap();
            assert_eq!(app.table_state, state);
        }
    }

    #[test]
    fn filtering_clamps_the_viewport_to_the_remaining_agent() {
        let mut app = numbered_app();
        app.table_state.select(Some(7));
        render_list(&mut app, 3);
        app.enter_search();
        for character in "row-02".chars() {
            app.push_char(character);
        }
        assert_eq!(render_list(&mut app, 3), ["row-02", "", ""]);
        assert_eq!(app.table_state.selected(), Some(0));
        assert_eq!(app.table_state.offset(), 0);
    }

    #[test]
    fn viewport_preserves_no_selection_and_clamps_selected_columns() {
        let mut app = numbered_app();
        app.table_state = TableState::new()
            .with_offset(6)
            .with_selected_column(Some(99));
        assert_eq!(render_list(&mut app, 3), ["row-06", "row-07", ""]);
        assert_eq!(app.table_state.selected(), None);
        assert_eq!(app.table_state.offset(), 6);
        assert_eq!(app.table_state.selected_column(), Some(4));
        app.table_state.select(Some(99));
        assert_eq!(render_list(&mut app, 3), ["row-06", "row-07", ""]);
        assert_eq!(app.table_state.selected(), Some(7));
    }

    #[test]
    fn scrolled_rows_use_their_own_search_highlights() {
        let agents = parse_agent_list(
            r#"{"result":{"agents":[
                {"agent":"codex","terminal_title_stripped":"long row-first"},
                {"agent":"codex","terminal_title_stripped":"row-second"}
            ]}}"#,
        )
        .unwrap();
        let mut app = App::new(None, None);
        app.set_agents(agents);
        app.enter_search();
        for character in "row".chars() {
            app.push_char(character);
        }
        let row = app
            .filtered
            .iter()
            .position(|&index| app.label(index) == "long row-first")
            .unwrap();
        assert_eq!(row, 1);
        app.table_state.select(Some(row));
        let mut terminal = Terminal::new(TestBackend::new(100, 1)).unwrap();
        let frame = terminal
            .draw(|frame| {
                let area = frame.area();
                draw_list(frame, &mut app, area);
            })
            .unwrap();
        let cells = &frame.buffer.content;
        let text: String = cells.iter().map(ratatui::buffer::Cell::symbol).collect();
        let label_start = text.find("long row-first").unwrap();
        let column = text[..label_start].chars().count();
        assert_ne!(cells[column].fg, Color::Cyan);
        assert_eq!(
            [
                cells[column + 5].fg,
                cells[column + 6].fg,
                cells[column + 7].fg
            ],
            [Color::Cyan; 3]
        );
    }

    #[test]
    fn draws_the_agent_list_with_status_and_count() {
        let mut app = sample_app();
        let screen = render(&mut app, 200, 20);
        assert!(screen.contains("2/2"));
        assert!(screen.contains("dotfiles work"));
        assert!(screen.contains("(focused)"));
        assert!(screen.contains("✓")); // idle codex
    }

    #[test]
    fn status_icons_match_the_builtin_agents_sidebar() {
        assert_eq!(status_glyph("working", 0), "⠋");
        assert_eq!(status_glyph("blocked", 0), "◉");
        assert_eq!(status_glyph("done", 0), "●");
        assert_eq!(status_glyph("idle", 0), "✓");
        assert_eq!(status_glyph("unknown", 0), "○");
    }

    #[test]
    fn status_colors_match_the_builtin_agents_sidebar() {
        assert_eq!(status_style("working").fg, Some(Color::Yellow));
        assert_eq!(status_style("blocked").fg, Some(Color::Red));
        assert_eq!(status_style("done").fg, Some(Color::Cyan));
        assert_eq!(status_style("idle").fg, Some(Color::Green));
        assert_eq!(status_style("unknown").fg, Some(Color::DarkGray));
    }

    #[test]
    fn selected_row_uses_a_subtle_background_without_reversing_colors() {
        let style = selected_row_style();

        assert_eq!(
            (style.bg, style.add_modifier),
            (Some(Color::DarkGray), Modifier::empty())
        );
    }

    #[test]
    fn draws_empty_and_no_match_states() {
        let mut app = App::new(None, None);
        let screen = render(&mut app, 100, 20);
        assert!(screen.contains("No agent panes found"));
        assert!(screen.contains("Nothing to preview"));

        let mut app = sample_app();
        app.enter_search();
        for c in "zzz".chars() {
            app.push_char(c);
        }
        let screen = render(&mut app, 100, 20);
        assert!(screen.contains("No matches for \"zzz\""));
    }

    #[test]
    fn search_mode_shows_the_filter_and_hints() {
        let mut app = sample_app();
        app.enter_search();
        for c in "deal".chars() {
            app.push_char(c);
        }
        let screen = render(&mut app, 100, 20);
        assert!(screen.contains("❯ deal"));
        assert!(screen.contains("1/2"));
        assert!(screen.contains("esc cancel"));
    }

    #[test]
    fn survives_tiny_terminals() {
        let mut app = sample_app();
        render(&mut app, 8, 3);
        render(&mut app, 1, 1);
    }

    #[test]
    fn preview_dimensions_match_the_odd_width_layout() {
        assert_eq!(preview_dimensions(Rect::new(0, 0, 81, 24)), (38, 21));
    }

    #[test]
    fn preview_renders_the_stored_terminal_style() {
        let mut app = sample_app();
        app.preview = Text::styled(
            "X",
            Style::new()
                .fg(Color::Indexed(1))
                .bg(Color::Rgb(1, 2, 3))
                .bold(),
        );
        let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
        let frame = terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let [_, preview_area, _] = areas(Rect::new(0, 0, 100, 20));
        let position = Block::bordered().inner(preview_area).as_position();
        let cell = frame.buffer.cell(position).unwrap();

        assert_eq!(
            (cell.symbol(), cell.fg, cell.bg, cell.modifier),
            ("X", Color::Indexed(1), Color::Rgb(1, 2, 3), Modifier::BOLD,)
        );
    }

    #[test]
    fn preview_error_is_visible_in_the_footer() {
        let mut app = sample_app();
        app.preview_error = Some("live preview unavailable: unsupported command".to_string());

        assert!(render(&mut app, 120, 20).contains("unsupported command"));
    }
}
