use std::collections::{BTreeMap, HashMap, HashSet};
use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration as StdDuration;

use chrono::{DateTime, Datelike, Duration, Local, LocalResult, NaiveDate, TimeZone, Utc};
use crossterm::event::{self, Event as CEvent, KeyCode, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{execute, ExecutableCommand};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::{Frame, Terminal};

use crate::domain::{format_duration, EventKind, Ledger, LedgerSnapshot, Task, TimeEvent};
use crate::ledgers::{recent_ledgers, remember_ledger};
use crate::storage::{load_ledger, save_ledger};

const TERMINAL_COLORS: [&str; 16] = [
	"black",
	"red",
	"green",
	"yellow",
	"blue",
	"magenta",
	"cyan",
	"gray",
	"dark_gray",
	"light_red",
	"light_green",
	"light_yellow",
	"light_blue",
	"light_magenta",
	"light_cyan",
	"white",
];
const FOCUSED_PANEL_BORDER_COLOR: Color = Color::Yellow;
const INACTIVE_PANEL_BORDER_COLOR: Color = Color::DarkGray;
const HIGHLIGHT_BACKGROUND_COLOR: Color = Color::Rgb(42, 45, 52);

pub fn run_dashboard(ledger: &mut Ledger, ledger_path: &mut PathBuf) -> Result<(), Box<dyn Error>> {
	enable_raw_mode()?;
	let mut stdout = io::stdout();
	stdout.execute(EnterAlternateScreen)?;
	let backend = CrosstermBackend::new(stdout);
	let mut terminal = Terminal::new(backend)?;

	let result = run_event_loop(&mut terminal, ledger, ledger_path);

	disable_raw_mode()?;
	execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
	terminal.show_cursor()?;

	result
}

fn run_event_loop(
	terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
	ledger: &mut Ledger,
	ledger_path: &mut PathBuf,
) -> Result<(), Box<dyn Error>> {
	let mut app = App::default();

	loop {
		let now = Utc::now();
		let snapshot = ledger.snapshot(now);
		let view = build_view(&app, ledger, &snapshot, now);
		app.clamp_selection(&view);
		terminal.draw(|frame| draw_dashboard(frame, &app, &view))?;

		if event::poll(StdDuration::from_millis(250))? {
			if let CEvent::Key(key) = event::read()? {
				if key.kind != KeyEventKind::Press {
					continue;
				}

				let should_quit = match &app.mode {
					InputMode::Prompt(_) => handle_prompt_key(&mut app, key.code, ledger, ledger_path),
					InputMode::Select(_) => handle_select_key(&mut app, key.code, ledger, ledger_path),
					InputMode::Normal => {
						handle_normal_key(&mut app, key.code, ledger, ledger_path, &snapshot, &view)
					}
				};

				if should_quit {
					break;
				}
			}
		}
	}

	Ok(())
}

fn draw_dashboard(frame: &mut Frame, app: &App, view: &ViewModel) {
	let layout = Layout::default()
		.direction(Direction::Vertical)
		.constraints([Constraint::Min(12), Constraint::Length(4)])
		.split(frame.area());

	let body = Layout::default()
		.direction(Direction::Horizontal)
		.constraints([
			Constraint::Percentage(28),
			Constraint::Percentage(44),
			Constraint::Percentage(28),
		])
		.split(layout[0]);

	let left = Layout::default()
		.direction(Direction::Vertical)
		.constraints([Constraint::Length(11), Constraint::Min(8)])
		.split(body[0]);

	render_calendar_panel(frame, left[0], app, &view.calendar_active_days);
	render_explorer_panel(frame, left[1], app, view);
	render_selected_day_panel(frame, body[1], app, view);
	render_week_stats_panel(frame, body[2], view);
	render_footer(frame, layout[1], app);

	if let InputMode::Select(select) = &app.mode {
		render_select_popup(frame, select);
	}
}

fn render_calendar_panel(
	frame: &mut Frame,
	area: Rect,
	app: &App,
	active_days: &HashSet<NaiveDate>,
) {
	let month = app.calendar_month;
	let selected_day = app.selected_day;
	let mut lines = Vec::new();
	lines.push(Line::from(format!("{} {}", month.format("%B"), month.year())));
	lines.push(Line::from("Mo Tu We Th Fr Sa Su"));

	let first_weekday = month.weekday().number_from_monday() as usize - 1;
	let days_in_month = days_in_month(month.year(), month.month());
	let mut day_counter = 1u32;
	for week in 0..6 {
		let mut spans = Vec::new();
		for weekday_index in 0..7 {
			let before_first = week == 0 && weekday_index < first_weekday;
			let after_last = day_counter > days_in_month;
			if before_first || after_last {
				spans.push(Span::raw("   "));
				continue;
			}

			let date = NaiveDate::from_ymd_opt(month.year(), month.month(), day_counter)
				.expect("calendar day must be valid");
			let mut style = Style::default();
			if date == selected_day {
				style = style.fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD);
			} else if active_days.contains(&date) {
				style = style.fg(Color::LightYellow).add_modifier(Modifier::BOLD);
			}

			spans.push(Span::styled(format!("{:>2} ", day_counter), style));
			day_counter += 1;
		}
		lines.push(Line::from(spans));
	}

	let block = Block::default()
		.borders(Borders::ALL)
		.title("Calendar")
		.border_style(border_style(app.focus == FocusPane::Calendar));
	let calendar = Paragraph::new(lines).block(block);
	frame.render_widget(calendar, area);
}

fn render_explorer_panel(frame: &mut Frame, area: Rect, app: &App, view: &ViewModel) {
	let title = match &app.explorer_mode {
		ExplorerMode::Projects => "Explorer: Projects".to_string(),
		ExplorerMode::ProjectTasks { project_name, .. } => format!("Explorer: {project_name}"),
	};

	let items = view
		.explorer_rows
		.iter()
		.map(|row| ListItem::new(row.line.clone()))
		.collect::<Vec<_>>();

	let mut state = ListState::default();
	if !view.explorer_rows.is_empty() {
		state.select(Some(app.explorer_index.min(view.explorer_rows.len() - 1)));
	}

	let block = Block::default()
		.borders(Borders::ALL)
		.title(title)
		.border_style(border_style(app.focus == FocusPane::Explorer));
	let list = List::new(if items.is_empty() {
		vec![ListItem::new("(empty)")]
	} else {
		items
	})
	.block(block)
	.highlight_style(Style::default().bg(HIGHLIGHT_BACKGROUND_COLOR).add_modifier(Modifier::BOLD));

	frame.render_stateful_widget(list, area, &mut state);
}

fn render_selected_day_panel(frame: &mut Frame, area: Rect, app: &App, view: &ViewModel) {
	let mut items = Vec::new();
	for (index, row) in view.day_rows.iter().enumerate() {
		items.push(ListItem::new(render_day_row_line(
			row,
			app.day_field,
			index == app.day_index,
		)));
	}

	if items.is_empty() {
		items.push(ListItem::new("(no sessions for selected day)"));
	}

	let mut state = ListState::default();
	if !view.day_rows.is_empty() {
		state.select(Some(app.day_index.min(view.day_rows.len() - 1)));
	}

	let title = format!(
		"{} | total {}",
		app.selected_day.format("%A, %d %B %Y"),
		format_duration(view.day_total)
	);
	let list = List::new(items)
		.block(
			Block::default()
				.borders(Borders::ALL)
				.title(title)
				.border_style(border_style(app.focus == FocusPane::Day)),
		)
		.highlight_style(Style::default().bg(HIGHLIGHT_BACKGROUND_COLOR).add_modifier(Modifier::BOLD));

	frame.render_stateful_widget(list, area, &mut state);
}

fn render_week_stats_panel(frame: &mut Frame, area: Rect, view: &ViewModel) {
	let week = &view.week_stats;
	let mut lines = Vec::new();
	lines.push(Line::from(format!(
		"Week {} - {}",
		week.week_start.format("%d %b"),
		(week.week_start + Duration::days(6)).format("%d %b")
	)));
	lines.push(Line::from(format!("Total: {}", format_duration(week.total))));
	lines.push(Line::from(format!("Avg/day: {}", format_duration(week.avg_per_day))));
	lines.push(Line::from(format!("Max/day: {}", format_duration(week.max_day))));
	lines.push(Line::from(format!("Active days: {}", week.active_days)));
	lines.push(Line::from(""));
	lines.push(Line::from("Daily Activity"));

	let max_seconds = week
		.daily
		.iter()
		.map(|(_, duration)| duration.num_seconds())
		.max()
		.unwrap_or(0)
		.max(1);
	for (day, duration) in &week.daily {
		let seconds = duration.num_seconds();
		let width = ((seconds as f64 / max_seconds as f64) * 16.0).round() as usize;
		let bar = "=".repeat(width.max(1));
		lines.push(Line::from(format!(
			"{} {:>8} {}",
			day.format("%a"),
			format_duration(*duration),
			if seconds == 0 { "".to_string() } else { bar }
		)));
	}

	lines.push(Line::from(""));
	lines.push(Line::from("Top Projects"));
	if week.top_projects.is_empty() {
		lines.push(Line::from("(none)"));
	} else {
		for project in week.top_projects.iter().take(6) {
			lines.push(Line::from(vec![
				Span::styled(project.name.clone(), project.style),
				Span::raw(format!(" | {}", format_duration(project.duration))),
			]));
		}
	}

	let panel = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Week Stats"));
	frame.render_widget(panel, area);
}

fn render_footer(frame: &mut Frame, area: Rect, app: &App) {
	let footer_lines = match &app.mode {
		InputMode::Normal => vec![
			Line::from(
				"Tab pane | arrows/hjkl navigate | Enter open/collapse (explorer) | q quit",
			),
			Line::from(
				"space start/stop (day+explorer) | d delete interval(day) | o new task | p project | c category | t task | s/x start/stop note | g switch ledger",
			),
			Line::from(format!(
				"{}{}",
				app.status,
				if app.focus == FocusPane::Day {
					format!(" | {}", app.day_edit_hint())
				} else {
					String::new()
				}
			)),
		],
		InputMode::Prompt(prompt) => vec![
			Line::from(prompt.title.clone()),
			Line::from(format!("> {}", prompt.input)),
			Line::from("Enter submit | Esc cancel"),
		],
		InputMode::Select(select) => vec![
			Line::from(select.title.clone()),
			Line::from(format!(
				"Selected: {}",
				select
					.selected_option()
					.map(|option| option.label.as_str())
					.unwrap_or("(none)")
			)),
			Line::from("j/k or arrows move | Enter choose | Esc cancel"),
		],
	};

	let footer = Paragraph::new(footer_lines).block(Block::default().borders(Borders::ALL).title("Shortcuts"));
	frame.render_widget(footer, area);
}

fn render_day_row_line(row: &DaySessionRow, selected_field: DayField, is_selected: bool) -> Line<'static> {
	let lane_text = lane_text(row.lane, row.lane_count);
	let start_text = row.display_start.with_timezone(&Local).format("%H:%M").to_string();
	let end_text = row.display_stop.with_timezone(&Local).format("%H:%M").to_string();

	let start_style = if is_selected && selected_field == DayField::Start {
		Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
	} else if row.start_event_index.is_some() {
		Style::default()
	} else {
		Style::default().fg(Color::DarkGray)
	};
	let end_style = if is_selected && selected_field == DayField::End {
		Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
	} else if row.stop_event_index.is_some() {
		Style::default()
	} else {
		Style::default().fg(Color::DarkGray)
	};

	let mut spans = vec![
		Span::styled(format!("{lane_text} "), Style::default().fg(Color::DarkGray)),
		Span::styled(start_text, start_style),
		Span::raw(" -> "),
		Span::styled(end_text, end_style),
		Span::raw(format!(" {} | ", format_duration(row.display_stop - row.display_start))),
		Span::styled(row.project_name.clone(), row.project_style),
		Span::raw(format!(" | {}", row.task_title)),
	];

	if let Some(note) = &row.note {
		spans.push(Span::raw(format!(" | {note}")));
	}

	Line::from(spans)
}

fn lane_text(lane: usize, lane_count: usize) -> String {
	let width = lane_count.min(5);
	let mut out = String::new();
	for index in 0..width {
		if index == lane {
			out.push('*');
		} else {
			out.push('|');
		}
	}
	if lane_count > width {
		out.push('+');
	}
	out
}

fn render_select_popup(frame: &mut Frame, select: &SelectState) {
	let area = centered_rect(62, 55, frame.area());
	frame.render_widget(Clear, area);

	let items = if select.options.is_empty() {
		vec![ListItem::new("(no choices)")]
	} else {
		select
			.options
			.iter()
			.map(|option| ListItem::new(option.label.clone()).style(option.style))
			.collect::<Vec<_>>()
	};

	let current = if select.options.is_empty() {
		0
	} else {
		select.selected.saturating_add(1)
	};
	let total = select.options.len();
	let list = List::new(items)
		.block(
			Block::default()
				.borders(Borders::ALL)
				.title(format!("{} ({current}/{total})", select.title)),
		)
		.highlight_symbol(">> ")
		.highlight_style(Style::default().bg(HIGHLIGHT_BACKGROUND_COLOR));

	let mut state = ListState::default();
	if !select.options.is_empty() {
		state.select(Some(select.selected.min(select.options.len().saturating_sub(1))));
	}
	frame.render_stateful_widget(list, area, &mut state);
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
	let popup_layout = Layout::default()
		.direction(Direction::Vertical)
		.constraints([
			Constraint::Percentage((100 - percent_y) / 2),
			Constraint::Percentage(percent_y),
			Constraint::Percentage((100 - percent_y) / 2),
		])
		.split(area);
	Layout::default()
		.direction(Direction::Horizontal)
		.constraints([
			Constraint::Percentage((100 - percent_x) / 2),
			Constraint::Percentage(percent_x),
			Constraint::Percentage((100 - percent_x) / 2),
		])
		.split(popup_layout[1])[1]
}

fn handle_normal_key(
	app: &mut App,
	code: KeyCode,
	ledger: &mut Ledger,
	ledger_path: &mut PathBuf,
	snapshot: &LedgerSnapshot,
	view: &ViewModel,
) -> bool {
	match code {
		KeyCode::Char('q') => true,
		KeyCode::Esc => {
			if app.focus == FocusPane::Explorer {
				if let ExplorerMode::ProjectTasks { .. } = app.explorer_mode {
					app.explorer_mode = ExplorerMode::Projects;
					app.explorer_index = 0;
					app.status = "Back to projects".to_string();
					return false;
				}
			}
			true
		}
		KeyCode::Tab => {
			app.focus = app.focus.next();
			app.clear_day_edit_buffer();
			false
		}
		KeyCode::BackTab => {
			app.focus = app.focus.prev();
			app.clear_day_edit_buffer();
			false
		}
		KeyCode::Up | KeyCode::Char('k') => {
			match app.focus {
				FocusPane::Calendar => app.shift_selected_day(-7),
				FocusPane::Day => app.move_day_selection(-1, view),
				FocusPane::Explorer => app.move_explorer_selection(-1, view),
			}
			false
		}
		KeyCode::Down | KeyCode::Char('j') => {
			match app.focus {
				FocusPane::Calendar => app.shift_selected_day(7),
				FocusPane::Day => app.move_day_selection(1, view),
				FocusPane::Explorer => app.move_explorer_selection(1, view),
			}
			false
		}
		KeyCode::Left | KeyCode::Char('h') => {
			match app.focus {
				FocusPane::Calendar => app.shift_selected_day(-1),
				FocusPane::Day => {
					app.day_field = DayField::Start;
					app.clear_day_edit_buffer();
				}
				FocusPane::Explorer => {}
			}
			false
		}
		KeyCode::Right | KeyCode::Char('l') => {
			match app.focus {
				FocusPane::Calendar => app.shift_selected_day(1),
				FocusPane::Day => {
					app.day_field = DayField::End;
					app.clear_day_edit_buffer();
				}
				FocusPane::Explorer => {}
			}
			false
		}
		KeyCode::Char('n') => {
			app.shift_selected_month(1);
			false
		}
		KeyCode::Char('N') => {
			app.shift_selected_month(-1);
			false
		}
		KeyCode::Backspace => {
			if app.focus == FocusPane::Day {
				app.day_edit_buffer.pop();
			}
			false
		}
		KeyCode::Char(value) if value.is_ascii_digit() && app.focus == FocusPane::Day => {
			handle_day_digit_input(app, value, ledger, ledger_path.as_path(), view);
			false
		}
		KeyCode::Char('p') => {
			app.mode = InputMode::Prompt(PromptState::new("Project name", PromptKind::AddProjectName));
			false
		}
		KeyCode::Char('c') => {
			app.mode = InputMode::Prompt(PromptState::new("Category name", PromptKind::AddCategoryName));
			false
		}
		KeyCode::Char('t') => {
			match build_task_project_select(ledger) {
				Ok(select) => app.mode = InputMode::Select(select),
				Err(err) => app.status = err,
			}
			false
		}
		KeyCode::Char('o') => {
			if let Some(project_id) = app.selected_project_for_new_task(view) {
				app.mode = InputMode::Select(build_task_category_select(ledger, project_id));
			} else {
				app.status = "Select a project in Explorer first".to_string();
			}
			false
		}
		KeyCode::Char('g') => {
			match build_ledger_switch_select(ledger_path.as_path()) {
				Ok(select) => app.mode = InputMode::Select(select),
				Err(err) => app.status = err,
			}
			false
		}
		KeyCode::Char('s') => {
			if let Some(task_id) = app.selected_task_id(view) {
				app.mode = InputMode::Prompt(PromptState::new(
					"Start note (optional)",
					PromptKind::StartTaskNote { task_id },
				));
			} else {
				app.status = "Select a task first".to_string();
			}
			false
		}
		KeyCode::Char('x') => {
			if let Some(task_id) = app.selected_active_task_id(view, snapshot) {
				app.mode = InputMode::Prompt(PromptState::new(
					"Stop note (optional)",
					PromptKind::StopTaskNote { task_id },
				));
			} else {
				app.status = "Select a running task to stop".to_string();
			}
			false
		}
		KeyCode::Char('d') => {
			if app.focus != FocusPane::Day {
				app.status = "Focus the Day view to delete an interval".to_string();
				return false;
			}

			let Some(row) = view.day_rows.get(app.day_index) else {
				app.status = "No selected interval to delete".to_string();
				return false;
			};
			let Some(start_event_index) = row.start_event_index else {
				app.status = "Selected interval cannot be deleted".to_string();
				return false;
			};

			app.mode = InputMode::Select(build_delete_interval_select(
				row,
				start_event_index,
			));
			false
		}
		KeyCode::Char(' ') => {
			if let Some(task_id) = app.selected_task_id(view) {
				let result = if snapshot.active_tasks.contains_key(&task_id) {
					stop_task(ledger, ledger_path.as_path(), &task_id, None)
				} else {
					start_task(ledger, ledger_path.as_path(), &task_id, None)
				};
				app.status = match result {
					Ok(message) => message,
					Err(err) => format!("error: {err}"),
				};
			} else if app.focus == FocusPane::Day {
				app.status = "No task selected in day view".to_string();
			} else if app.focus == FocusPane::Explorer {
				app.status = "Select a task row in Explorer first".to_string();
			}
			false
		}
		KeyCode::Enter => {
			if app.focus == FocusPane::Explorer {
				match app.selected_explorer_row_kind(view) {
					Some(ExplorerRowKind::Project {
						project_id,
						project_name,
					}) => {
						app.explorer_mode = ExplorerMode::ProjectTasks {
							project_id,
							project_name,
						};
						app.explorer_index = 0;
					}
					Some(ExplorerRowKind::Category { key }) => {
						if app.explorer_collapsed_categories.contains(&key) {
							app.explorer_collapsed_categories.remove(&key);
						} else {
							app.explorer_collapsed_categories.insert(key);
						}
					}
					Some(ExplorerRowKind::Task { .. }) => {
						app.status = "Press space to start/stop this task".to_string();
					}
					Some(ExplorerRowKind::Empty) | None => {}
				}
				return false;
			}
			false
		}
		_ => false,
	}
}

fn handle_day_digit_input(app: &mut App, digit: char, ledger: &mut Ledger, ledger_path: &Path, view: &ViewModel) {
	if view.day_rows.is_empty() {
		app.status = "No sessions on selected day".to_string();
		return;
	}

	app.day_edit_buffer.push(digit);
	if app.day_edit_buffer.len() < 4 {
		return;
	}

	let buffer = app.day_edit_buffer.clone();
	app.day_edit_buffer.clear();

	let hour = buffer[0..2].parse::<u32>();
	let minute = buffer[2..4].parse::<u32>();
	let (hour, minute) = match (hour, minute) {
		(Ok(hour), Ok(minute)) if hour < 24 && minute < 60 => (hour, minute),
		_ => {
			app.status = format!("invalid time '{buffer}', expected HHMM");
			return;
		}
	};

	let Some(row) = view.day_rows.get(app.day_index) else {
		app.status = "No selected session".to_string();
		return;
	};

	let base_date = match app.day_field {
		DayField::Start => row.start.with_timezone(&Local).date_naive(),
		DayField::End => row.stop.with_timezone(&Local).date_naive(),
	};
	let next_timestamp = match local_clock_on_date_to_utc(base_date, hour, minute) {
		Ok(timestamp) => timestamp,
		Err(err) => {
			app.status = err;
			return;
		}
	};

	match app.day_field {
		DayField::Start => {
			let Some(event_index) = row.start_event_index else {
				app.status = "session start cannot be edited".to_string();
				return;
			};
			if next_timestamp >= row.stop {
				app.status = "start must be before end".to_string();
				return;
			}
			if let Some(previous_stop) = previous_stop_for_task(ledger, &row.task_id, event_index) {
				if next_timestamp < previous_stop {
					app.status = "start cannot be before previous stop for this task".to_string();
					return;
				}
			}

			if !matches!(ledger.events.get(event_index).map(|event| &event.kind), Some(EventKind::Start { .. })) {
				app.status = "unable to edit start: event mismatch".to_string();
				return;
			}

			ledger.events[event_index].timestamp = next_timestamp;
			if let Err(err) = persist(ledger_path, ledger) {
				app.status = format!("error: {err}");
				return;
			}
			app.status = format!(
				"updated start to {}",
				next_timestamp.with_timezone(&Local).format("%H:%M")
			);
		}
		DayField::End => {
			let Some(event_index) = row.stop_event_index else {
				app.status = "session end cannot be edited while task is running".to_string();
				return;
			};
			if next_timestamp <= row.start {
				app.status = "end must be after start".to_string();
				return;
			}
			if next_timestamp > Utc::now() {
				app.status = "end cannot be later than current time".to_string();
				return;
			}
			if let Some(next_start) = next_start_for_task(ledger, &row.task_id, event_index) {
				if next_timestamp > next_start {
					app.status = "end cannot be after following start for this task".to_string();
					return;
				}
			}

			if !matches!(ledger.events.get(event_index).map(|event| &event.kind), Some(EventKind::Stop { .. })) {
				app.status = "unable to edit end: event mismatch".to_string();
				return;
			}

			ledger.events[event_index].timestamp = next_timestamp;
			if let Err(err) = persist(ledger_path, ledger) {
				app.status = format!("error: {err}");
				return;
			}
			app.status = format!(
				"updated end to {}",
				next_timestamp.with_timezone(&Local).format("%H:%M")
			);
		}
	}
}

fn previous_stop_for_task(ledger: &Ledger, task_id: &str, start_event_index: usize) -> Option<DateTime<Utc>> {
	let task_events = sorted_task_events(ledger, task_id);
	let current_position = task_events
		.iter()
		.position(|entry| entry.index == start_event_index && entry.kind == TaskEventKind::Start)?;
	task_events[..current_position]
		.iter()
		.rev()
		.find(|entry| entry.kind == TaskEventKind::Stop)
		.map(|entry| entry.timestamp)
}

fn next_start_for_task(ledger: &Ledger, task_id: &str, stop_event_index: usize) -> Option<DateTime<Utc>> {
	let task_events = sorted_task_events(ledger, task_id);
	let current_position = task_events
		.iter()
		.position(|entry| entry.index == stop_event_index && entry.kind == TaskEventKind::Stop)?;
	task_events
		.iter()
		.skip(current_position + 1)
		.find(|entry| entry.kind == TaskEventKind::Start)
		.map(|entry| entry.timestamp)
}

fn sorted_task_events(ledger: &Ledger, task_id: &str) -> Vec<TaskEventRef> {
	let mut events = ledger
		.events
		.iter()
		.enumerate()
		.filter_map(|(index, event)| match &event.kind {
			EventKind::Start {
				task_id: event_task_id,
				..
			} if event_task_id == task_id => Some(TaskEventRef {
				index,
				timestamp: event.timestamp,
				kind: TaskEventKind::Start,
			}),
			EventKind::Stop {
				task_id: event_task_id,
				..
			} if event_task_id == task_id => Some(TaskEventRef {
				index,
				timestamp: event.timestamp,
				kind: TaskEventKind::Stop,
			}),
			_ => None,
		})
		.collect::<Vec<_>>();
	events.sort_by(|left, right| {
		left
			.timestamp
			.cmp(&right.timestamp)
			.then_with(|| left.index.cmp(&right.index))
	});
	events
}

fn handle_prompt_key(
	app: &mut App,
	code: KeyCode,
	ledger: &mut Ledger,
	ledger_path: &mut PathBuf,
) -> bool {
	match code {
		KeyCode::Esc => {
			app.mode = InputMode::Normal;
			app.status = "Input cancelled".to_string();
		}
		KeyCode::Backspace => {
			if let InputMode::Prompt(prompt) = &mut app.mode {
				prompt.input.pop();
			}
		}
		KeyCode::Char(value) => {
			if let InputMode::Prompt(prompt) = &mut app.mode {
				prompt.input.push(value);
			}
		}
		KeyCode::Enter => {
			let prompt = match std::mem::replace(&mut app.mode, InputMode::Normal) {
				InputMode::Prompt(prompt) => prompt,
				InputMode::Normal | InputMode::Select(_) => return false,
			};

			match submit_prompt(prompt.clone(), ledger, ledger_path.as_path()) {
				Ok(PromptOutcome::NextPrompt(next_prompt)) => app.mode = InputMode::Prompt(next_prompt),
				Ok(PromptOutcome::Select(select)) => app.mode = InputMode::Select(select),
				Ok(PromptOutcome::Done(message)) => {
					app.mode = InputMode::Normal;
					app.status = message;
				}
				Err(err) => {
					app.mode = InputMode::Prompt(prompt);
					app.status = format!("error: {err}");
				}
			}
		}
		_ => {}
	}

	false
}

fn handle_select_key(
	app: &mut App,
	code: KeyCode,
	ledger: &mut Ledger,
	ledger_path: &mut PathBuf,
) -> bool {
	match code {
		KeyCode::Esc => {
			app.mode = InputMode::Normal;
			app.status = "Selection cancelled".to_string();
		}
		KeyCode::Up | KeyCode::Char('k') => {
			if let InputMode::Select(select) = &mut app.mode {
				select.move_selection(-1);
			}
		}
		KeyCode::Down | KeyCode::Char('j') => {
			if let InputMode::Select(select) = &mut app.mode {
				select.move_selection(1);
			}
		}
		KeyCode::Enter => {
			let select = match std::mem::replace(&mut app.mode, InputMode::Normal) {
				InputMode::Select(select) => select,
				_ => return false,
			};

			match submit_select(select.clone(), ledger, ledger_path) {
				Ok(SelectOutcome::NextPrompt(prompt)) => app.mode = InputMode::Prompt(prompt),
				Ok(SelectOutcome::NextSelect(next_select)) => app.mode = InputMode::Select(next_select),
				Ok(SelectOutcome::Done(message)) => {
					app.mode = InputMode::Normal;
					app.status = message;
				}
				Err(err) => {
					app.mode = InputMode::Select(select);
					app.status = format!("error: {err}");
				}
			}
		}
		_ => {}
	}

	false
}

fn submit_prompt(
	prompt: PromptState,
	ledger: &mut Ledger,
	ledger_path: &Path,
) -> Result<PromptOutcome, String> {
	match prompt.kind {
		PromptKind::AddProjectName => {
			let name = required_text(&prompt.input, "project name")?;
			Ok(PromptOutcome::Select(build_project_color_select(name)))
		}
		PromptKind::AddCategoryName => {
			let name = required_text(&prompt.input, "category name")?;
			Ok(PromptOutcome::NextPrompt(PromptState::new(
				"Category description (optional)",
				PromptKind::AddCategoryDescription { name },
			)))
		}
		PromptKind::AddCategoryDescription { name } => {
			let description = optional_text(&prompt.input);
			let created_name = name.clone();
			ledger.add_category(name, description);
			persist(ledger_path, ledger)?;
			Ok(PromptOutcome::Done(format!("created category: {created_name}")))
		}
		PromptKind::AddTaskDescription {
			project_id,
			category_id,
		} => {
			let description = required_text(&prompt.input, "task description")?;
			let task_label = description.lines().next().unwrap_or("(no description)").to_string();
			ledger.add_task(project_id, category_id, description)?;
			persist(ledger_path, ledger)?;
			Ok(PromptOutcome::Done(format!("created task: {task_label}")))
		}
		PromptKind::StartTaskNote { task_id } => {
			let note = optional_text(&prompt.input);
			start_task(ledger, ledger_path, &task_id, note).map(PromptOutcome::Done)
		}
		PromptKind::StopTaskNote { task_id } => {
			let note = optional_text(&prompt.input);
			stop_task(ledger, ledger_path, &task_id, note).map(PromptOutcome::Done)
		}
	}
}

fn submit_select(
	select: SelectState,
	ledger: &mut Ledger,
	ledger_path: &mut PathBuf,
) -> Result<SelectOutcome, String> {
	let selected_value = select
		.selected_option()
		.map(|option| option.value.clone())
		.ok_or_else(|| "no option selected".to_string())?;

	match select.kind {
		SelectKind::ProjectColor { name } => {
			let created_name = name.clone();
			ledger.add_project(name, selected_value);
			persist(ledger_path.as_path(), ledger)?;
			Ok(SelectOutcome::Done(format!("created project: {created_name}")))
		}
		SelectKind::TaskProject => {
			let project_id = selected_value.ok_or_else(|| "selected project is missing".to_string())?;
			Ok(SelectOutcome::NextSelect(build_task_category_select(ledger, project_id)))
		}
		SelectKind::TaskCategory { project_id } => Ok(SelectOutcome::NextPrompt(PromptState::new(
			"Task description",
			PromptKind::AddTaskDescription {
				project_id,
				category_id: selected_value,
			},
		))),
		SelectKind::LedgerSwitch => {
			let selected_path = selected_value
				.map(PathBuf::from)
				.ok_or_else(|| "selected ledger path is missing".to_string())?;
			switch_ledger(ledger, ledger_path, selected_path).map(SelectOutcome::Done)
		}
		SelectKind::DeleteIntervalConfirm {
			start_event_index,
			stop_event_index,
			task_title,
		} => {
			let action = selected_value
				.as_deref()
				.ok_or_else(|| "selected action is missing".to_string())?;
			if action == "delete" {
				delete_interval(
					ledger,
					ledger_path.as_path(),
					start_event_index,
					stop_event_index,
					task_title.as_str(),
				)
				.map(SelectOutcome::Done)
			} else {
				Ok(SelectOutcome::Done("Delete cancelled".to_string()))
			}
		}
	}
}

fn build_project_color_select(name: String) -> SelectState {
	let mut options = vec![SelectOption::new(
		"No color",
		None,
		Style::default().fg(Color::Gray),
	)];
	for color in TERMINAL_COLORS {
		let swatch = "████████████████".to_string();
		options.push(SelectOption::new(
			swatch,
			Some(color.to_string()),
			color_block_style(color),
		));
	}

	SelectState::new("Select project color", SelectKind::ProjectColor { name }, options)
}

fn build_task_project_select(ledger: &Ledger) -> Result<SelectState, String> {
	let mut projects = ledger
		.header
		.projects
		.iter()
		.filter(|project| !project.archived)
		.collect::<Vec<_>>();
	projects.sort_by(|left, right| left.name.cmp(&right.name).then_with(|| left.id.cmp(&right.id)));

	if projects.is_empty() {
		return Err("no active projects found. Press 'p' to create one first".to_string());
	}

	let options = projects
		.into_iter()
		.map(|project| {
			SelectOption::new(
				project.name.clone(),
				Some(project.id.clone()),
				style_from_project_color(project.color.as_deref()),
			)
		})
		.collect::<Vec<_>>();

	Ok(SelectState::new("Select project", SelectKind::TaskProject, options))
}

fn build_task_category_select(ledger: &Ledger, project_id: String) -> SelectState {
	let mut categories = ledger
		.header
		.categories
		.iter()
		.filter(|category| !category.archived)
		.collect::<Vec<_>>();
	categories.sort_by(|left, right| left.name.cmp(&right.name).then_with(|| left.id.cmp(&right.id)));

	let mut options = vec![SelectOption::new("Uncategorized", None, Style::default())];
	for category in categories {
		options.push(SelectOption::new(
			category.name.clone(),
			Some(category.id.clone()),
			Style::default(),
		));
	}

	SelectState::new(
		"Select category",
		SelectKind::TaskCategory { project_id },
		options,
	)
}

fn build_ledger_switch_select(current_path: &Path) -> Result<SelectState, String> {
	let mut paths = recent_ledgers(100).map_err(|err| format!("failed to load recent ledgers: {err}"))?;
	let current_path = current_path.to_path_buf();
	if !paths.iter().any(|path| path == &current_path) {
		paths.insert(0, current_path.clone());
	}

	if paths.is_empty() {
		return Err("no known ledgers. run once with --ledger <path> first".to_string());
	}

	let current_value = current_path.display().to_string();
	let options = paths
		.into_iter()
		.map(|path| {
			let value = path.display().to_string();
			let is_current = value == current_value;
			let exists = path.exists();
			let mut label = value.clone();
			if is_current {
				label = format!("* {label}");
			}
			if !exists {
				label = format!("[missing] {label}");
			}

			let style = if is_current {
				Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
			} else if exists {
				Style::default()
			} else {
				Style::default().fg(Color::DarkGray)
			};

			SelectOption::new(label, Some(value), style)
		})
		.collect::<Vec<_>>();

	let mut select = SelectState::new("Switch ledger", SelectKind::LedgerSwitch, options);
	select.selected = select
		.options
		.iter()
		.position(|option| option.value.as_deref() == Some(current_value.as_str()))
		.unwrap_or(0);
	Ok(select)
}

fn build_delete_interval_select(row: &DaySessionRow, start_event_index: usize) -> SelectState {
	let title = format!(
		"Delete interval? {} {}-{}",
		row.task_title,
		row.display_start.with_timezone(&Local).format("%H:%M"),
		row.display_stop.with_timezone(&Local).format("%H:%M")
	);
	let options = vec![
		SelectOption::new(
			"Delete",
			Some("delete".to_string()),
			Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD),
		),
		SelectOption::new("Cancel", Some("cancel".to_string()), Style::default()),
	];

	let mut select = SelectState::new(
		title,
		SelectKind::DeleteIntervalConfirm {
			start_event_index,
			stop_event_index: row.stop_event_index,
			task_title: row.task_title.clone(),
		},
		options,
	);
	// Default to cancel to prevent accidental deletions.
	select.selected = 1;
	select
}

fn build_view(app: &App, ledger: &Ledger, snapshot: &LedgerSnapshot, now: DateTime<Utc>) -> ViewModel {
	let sessions = collect_sessions(ledger, now);
	let daily_task_totals = build_local_daily_task_totals(&sessions);
	let calendar_active_days = daily_task_totals.keys().copied().collect::<HashSet<_>>();
	let (day_rows, day_total) = build_day_rows(app.selected_day, ledger, &sessions);
	let week_stats = build_week_stats(app.selected_day, ledger, &daily_task_totals);
	let explorer_rows = build_explorer_rows(app, ledger, snapshot, &week_stats);

	ViewModel {
		calendar_active_days,
		day_rows,
		day_total,
		week_stats,
		explorer_rows,
	}
}

fn collect_sessions(ledger: &Ledger, now: DateTime<Utc>) -> Vec<SessionRecord> {
	let mut indexed_events = ledger
		.events
		.iter()
		.enumerate()
		.collect::<Vec<(usize, &TimeEvent)>>();
	indexed_events.sort_by(|left, right| {
		left.1
			.timestamp
			.cmp(&right.1.timestamp)
			.then_with(|| left.0.cmp(&right.0))
	});

	let mut active: HashMap<String, ActiveSessionRef> = HashMap::new();
	let mut sessions = Vec::new();

	for (index, event) in indexed_events {
		match &event.kind {
			EventKind::Start { task_id, note } => {
				active.insert(
					task_id.clone(),
					ActiveSessionRef {
						started_at: event.timestamp,
						note: note.clone(),
						start_event_index: index,
					},
				);
			}
			EventKind::Stop { task_id, .. } => {
				if let Some(active_session) = active.remove(task_id) {
					if event.timestamp > active_session.started_at {
						sessions.push(SessionRecord {
							task_id: task_id.clone(),
							start: active_session.started_at,
							stop: event.timestamp,
							note: active_session.note,
							start_event_index: Some(active_session.start_event_index),
							stop_event_index: Some(index),
						});
					}
				}
			}
		}
	}

	for (task_id, active_session) in active {
		if now > active_session.started_at {
			sessions.push(SessionRecord {
				task_id,
				start: active_session.started_at,
				stop: now,
				note: active_session.note,
				start_event_index: Some(active_session.start_event_index),
				stop_event_index: None,
			});
		}
	}

	sessions
}

fn build_day_rows(selected_day: NaiveDate, ledger: &Ledger, sessions: &[SessionRecord]) -> (Vec<DaySessionRow>, Duration) {
	let (day_start, day_end) = local_day_bounds_utc(selected_day);

	let mut rows = sessions
		.iter()
		.filter_map(|session| {
			if session.stop <= day_start || session.start >= day_end {
				return None;
			}

			let display_start = if session.start < day_start {
				day_start
			} else {
				session.start
			};
			let display_stop = if session.stop > day_end { day_end } else { session.stop };
			if display_stop <= display_start {
				return None;
			}

			let (project_name, task_title) = task_project_and_title(ledger, &session.task_id);
			let project_style = task_style_for_id(ledger, &session.task_id);

			Some(DaySessionRow {
				task_id: session.task_id.clone(),
				project_name,
				task_title,
				project_style,
				note: session.note.clone(),
				start: session.start,
				stop: session.stop,
				display_start,
				display_stop,
				start_event_index: session.start_event_index,
				stop_event_index: session.stop_event_index,
				lane: 0,
				lane_count: 1,
			})
		})
		.collect::<Vec<_>>();

	rows.sort_by(|left, right| {
		left.display_start
			.cmp(&right.display_start)
			.then_with(|| left.display_stop.cmp(&right.display_stop))
			.then_with(|| left.task_title.cmp(&right.task_title))
	});

	let mut lane_ends = Vec::<DateTime<Utc>>::new();
	for row in &mut rows {
		let lane = lane_ends
			.iter()
			.position(|lane_end| *lane_end <= row.display_start)
			.unwrap_or_else(|| {
				lane_ends.push(row.display_start);
				lane_ends.len() - 1
			});
		lane_ends[lane] = row.display_stop;
		row.lane = lane;
	}
	let lane_count = lane_ends.len().max(1);
	for row in &mut rows {
		row.lane_count = lane_count;
	}

	let day_total = rows
		.iter()
		.fold(Duration::zero(), |acc, row| acc + (row.display_stop - row.display_start));

	(rows, day_total)
}

fn build_local_daily_task_totals(sessions: &[SessionRecord]) -> BTreeMap<NaiveDate, HashMap<String, Duration>> {
	let mut daily_task_totals = BTreeMap::<NaiveDate, HashMap<String, Duration>>::new();

	for session in sessions {
		if session.stop <= session.start {
			continue;
		}

		let mut day = session.start.with_timezone(&Local).date_naive();
		let last_moment = session.stop - Duration::seconds(1);
		let last_day = last_moment.with_timezone(&Local).date_naive();
		while day <= last_day {
			let (day_start, day_end) = local_day_bounds_utc(day);
			let slice_start = if session.start > day_start {
				session.start
			} else {
				day_start
			};
			let slice_end = if session.stop < day_end {
				session.stop
			} else {
				day_end
			};

			if slice_end > slice_start {
				let task_totals = daily_task_totals.entry(day).or_default();
				*task_totals
					.entry(session.task_id.clone())
					.or_insert_with(Duration::zero) += slice_end - slice_start;
			}

			day = day.succ_opt().expect("next day should exist");
		}
	}

	daily_task_totals
}

fn build_week_stats(
	selected_day: NaiveDate,
	ledger: &Ledger,
	daily_task_totals: &BTreeMap<NaiveDate, HashMap<String, Duration>>,
) -> WeekStatsView {
	let week_start = start_of_week(selected_day);
	let mut daily = Vec::new();
	let mut total = Duration::zero();
	let mut max_day = Duration::zero();
	let mut active_days = 0usize;
	let mut project_totals: HashMap<String, Duration> = HashMap::new();

	for offset in 0..7 {
		let day = week_start + Duration::days(offset);
		let durations = daily_task_totals
			.get(&day)
			.cloned()
			.unwrap_or_default();
		let day_total = durations
			.values()
			.fold(Duration::zero(), |acc, value| acc + *value);

		if day_total > Duration::zero() {
			active_days += 1;
		}
		if day_total > max_day {
			max_day = day_total;
		}
		total += day_total;
		daily.push((day, day_total));

		for (task_id, duration) in durations {
			if let Some(task) = ledger.task(&task_id) {
				*project_totals
					.entry(task.project_id.clone())
					.or_insert_with(Duration::zero) += duration;
			}
		}
	}

	let avg_per_day = Duration::seconds(total.num_seconds() / 7);

	let mut top_projects = project_totals
		.iter()
		.map(|(project_id, duration)| {
			let project = ledger.project(project_id);
			let name = project
				.map(|project| project.name.clone())
				.unwrap_or_else(|| "Unknown project".to_string());
			let style = style_from_project_color(project.and_then(|project| project.color.as_deref()));
			ProjectSummaryRow {
				name,
				style,
				duration: *duration,
			}
		})
		.collect::<Vec<_>>();
	top_projects.sort_by(|left, right| right.duration.cmp(&left.duration).then_with(|| left.name.cmp(&right.name)));

	WeekStatsView {
		week_start,
		daily,
		total,
		avg_per_day,
		max_day,
		active_days,
		project_totals,
		top_projects,
	}
}

fn build_explorer_rows(
	app: &App,
	ledger: &Ledger,
	snapshot: &LedgerSnapshot,
	week_stats: &WeekStatsView,
) -> Vec<ExplorerRow> {
	match &app.explorer_mode {
		ExplorerMode::Projects => {
			let mut projects = ledger
				.header
				.projects
				.iter()
				.filter(|project| !project.archived)
				.collect::<Vec<_>>();
			projects.sort_by(|left, right| left.name.cmp(&right.name).then_with(|| left.id.cmp(&right.id)));

			if projects.is_empty() {
				return vec![ExplorerRow::empty("(no active projects)")];
			}

			projects
				.into_iter()
				.map(|project| {
					let task_count = ledger
						.header
						.tasks
						.iter()
						.filter(|task| !task.archived && task.project_id == project.id)
						.count();
					let week_total = week_stats
						.project_totals
						.get(&project.id)
						.copied()
						.unwrap_or_else(Duration::zero);
					let style = style_from_project_color(project.color.as_deref());
					ExplorerRow {
						line: Line::from(vec![
							Span::styled(project.name.clone(), style),
							Span::raw(format!(" | tasks {} | week {}", task_count, format_duration(week_total))),
						]),
						kind: ExplorerRowKind::Project {
							project_id: project.id.clone(),
							project_name: project.name.clone(),
						},
					}
				})
				.collect::<Vec<_>>()
		}
		ExplorerMode::ProjectTasks {
			project_id,
			project_name: _,
		} => {
			let mut tasks = ledger
				.header
				.tasks
				.iter()
				.filter(|task| !task.archived && task.project_id == *project_id)
				.collect::<Vec<&Task>>();

			if tasks.is_empty() {
				return vec![ExplorerRow::empty("(no tasks in this project)")];
			}

			tasks.sort_by(|left, right| {
				left.category_id
					.cmp(&right.category_id)
					.then_with(|| left.short_description().cmp(&right.short_description()))
					.then_with(|| left.id.cmp(&right.id))
			});

			let mut grouped: BTreeMap<String, (String, Vec<&Task>)> = BTreeMap::new();
			for task in tasks {
				let label = task
					.category_id
					.as_ref()
					.and_then(|id| ledger.category(id))
					.map(|category| category.name.clone())
					.unwrap_or_else(|| "Uncategorized".to_string());
				let key = explorer_category_key(project_id, task.category_id.as_deref());
				grouped
					.entry(key)
					.and_modify(|(_, entries)| entries.push(task))
					.or_insert_with(|| (label, vec![task]));
			}

			let mut rows = Vec::new();
			for (key, (label, mut category_tasks)) in grouped {
				category_tasks.sort_by(|left, right| {
					left
						.short_description()
						.cmp(&right.short_description())
						.then_with(|| left.id.cmp(&right.id))
				});

				let is_collapsed = app.explorer_collapsed_categories.contains(&key);
				rows.push(ExplorerRow {
					line: Line::from(format!(
						"{} {} ({})",
						if is_collapsed { "[+]" } else { "[-]" },
						label,
						category_tasks.len()
					)),
					kind: ExplorerRowKind::Category { key: key.clone() },
				});

				if is_collapsed {
					continue;
				}

				for task in category_tasks {
					let is_running = snapshot.active_tasks.contains_key(&task.id);
					rows.push(ExplorerRow {
						line: Line::from(format!(
							"  {} {}",
							if is_running { "RUN" } else { "   " },
							task.short_description()
						)),
						kind: ExplorerRowKind::Task {
							task_id: task.id.clone(),
							project_id: project_id.clone(),
						},
					});
				}
			}

			if rows.is_empty() {
				vec![ExplorerRow::empty("(no tasks)")]
			} else {
				rows
			}
		}
	}
}

fn start_task(
	ledger: &mut Ledger,
	ledger_path: &Path,
	task_id: &str,
	note: Option<String>,
) -> Result<String, String> {
	let task = task_label(ledger, task_id);
	ledger.start_task(task_id, Utc::now(), note)?;
	persist(ledger_path, ledger)?;
	Ok(format!("started: {task}"))
}

fn stop_task(
	ledger: &mut Ledger,
	ledger_path: &Path,
	task_id: &str,
	note: Option<String>,
) -> Result<String, String> {
	let task = task_label(ledger, task_id);
	ledger.stop_task(task_id, Utc::now(), note)?;
	persist(ledger_path, ledger)?;
	Ok(format!("stopped: {task}"))
}

fn delete_interval(
	ledger: &mut Ledger,
	ledger_path: &Path,
	start_event_index: usize,
	stop_event_index: Option<usize>,
	task_title: &str,
) -> Result<String, String> {
	if start_event_index >= ledger.events.len() {
		return Err("interval start event no longer exists".to_string());
	}
	if !matches!(
		ledger.events.get(start_event_index).map(|event| &event.kind),
		Some(EventKind::Start { .. })
	) {
		return Err("interval start event mismatch".to_string());
	}

	if let Some(stop_index) = stop_event_index {
		if stop_index >= ledger.events.len() {
			return Err("interval stop event no longer exists".to_string());
		}
		if !matches!(
			ledger.events.get(stop_index).map(|event| &event.kind),
			Some(EventKind::Stop { .. })
		) {
			return Err("interval stop event mismatch".to_string());
		}
	}

	let mut indices = vec![start_event_index];
	if let Some(stop_index) = stop_event_index {
		if stop_index != start_event_index {
			indices.push(stop_index);
		}
	}
	indices.sort_unstable_by(|left, right| right.cmp(left));
	for index in indices {
		ledger.events.remove(index);
	}

	persist(ledger_path, ledger)?;
	Ok(format!("deleted interval: {task_title}"))
}

fn switch_ledger(ledger: &mut Ledger, ledger_path: &mut PathBuf, next_path: PathBuf) -> Result<String, String> {
	if &next_path == ledger_path {
		return Ok(format!("already using ledger: {}", ledger_path.display()));
	}

	if !next_path.exists() {
		return Err(format!("ledger does not exist: {}", next_path.display()));
	}

	let next_ledger = load_ledger(&next_path).map_err(|err| err.to_string())?;
	*ledger = next_ledger;
	*ledger_path = next_path;

	match remember_ledger(ledger_path.as_path()) {
		Ok(()) => Ok(format!("switched ledger: {}", ledger_path.display())),
		Err(err) => Ok(format!(
			"switched ledger: {} (warning: failed to store recents: {err})",
			ledger_path.display()
		)),
	}
}

fn persist(path: &Path, ledger: &Ledger) -> Result<(), String> {
	save_ledger(path, ledger).map_err(|err| err.to_string())
}

fn required_text(input: &str, field_name: &str) -> Result<String, String> {
	let value = input.trim();
	if value.is_empty() {
		Err(format!("{field_name} is required"))
	} else {
		Ok(value.to_string())
	}
}

fn optional_text(input: &str) -> Option<String> {
	let value = input.trim();
	if value.is_empty() {
		None
	} else {
		Some(value.to_string())
	}
}

fn task_label(ledger: &Ledger, task_id: &str) -> String {
	ledger
		.task(task_id)
		.map(|task| task.short_description())
		.unwrap_or_else(|| "Unknown task".to_string())
}

fn task_project_and_title(ledger: &Ledger, task_id: &str) -> (String, String) {
	if let Some(task) = ledger.task(task_id) {
		let project = ledger
			.project(&task.project_id)
			.map(|project| project.name.clone())
			.unwrap_or_else(|| "Unknown project".to_string());
		return (project, task.short_description());
	}

	("Unknown project".to_string(), "Unknown task".to_string())
}

fn task_style_for_id(ledger: &Ledger, task_id: &str) -> Style {
	let color_name = ledger
		.task(task_id)
		.and_then(|task| ledger.project(&task.project_id))
		.and_then(|project| project.color.as_deref());
	style_from_project_color(color_name)
}

fn style_from_project_color(color_name: Option<&str>) -> Style {
	color_name
		.and_then(color_from_name)
		.map(|color| Style::default().fg(color))
		.unwrap_or_default()
}

fn color_block_style(color_name: &str) -> Style {
	color_from_name(color_name)
		.map(|color| Style::default().fg(color))
		.unwrap_or_default()
}

fn color_from_name(color_name: &str) -> Option<Color> {
	match color_name {
		"black" => Some(Color::Black),
		"red" => Some(Color::Red),
		"green" => Some(Color::Green),
		"yellow" => Some(Color::Yellow),
		"blue" => Some(Color::Blue),
		"magenta" => Some(Color::Magenta),
		"cyan" => Some(Color::Cyan),
		"gray" => Some(Color::Gray),
		"dark_gray" => Some(Color::DarkGray),
		"light_red" => Some(Color::LightRed),
		"light_green" => Some(Color::LightGreen),
		"light_yellow" => Some(Color::LightYellow),
		"light_blue" => Some(Color::LightBlue),
		"light_magenta" => Some(Color::LightMagenta),
		"light_cyan" => Some(Color::LightCyan),
		"white" => Some(Color::White),
		_ => None,
	}
}

fn border_style(focused: bool) -> Style {
	if focused {
		Style::default()
			.fg(FOCUSED_PANEL_BORDER_COLOR)
			.add_modifier(Modifier::BOLD)
	} else {
		Style::default().fg(INACTIVE_PANEL_BORDER_COLOR)
	}
}

fn days_in_month(year: i32, month: u32) -> u32 {
	let first_of_next = if month == 12 {
		NaiveDate::from_ymd_opt(year + 1, 1, 1).expect("next year date should be valid")
	} else {
		NaiveDate::from_ymd_opt(year, month + 1, 1).expect("next month date should be valid")
	};
	(first_of_next - Duration::days(1)).day()
}

fn first_day_of_month(day: NaiveDate) -> NaiveDate {
	NaiveDate::from_ymd_opt(day.year(), day.month(), 1).expect("first day of month must be valid")
}

fn start_of_week(day: NaiveDate) -> NaiveDate {
	let days_from_monday = day.weekday().number_from_monday() as i64 - 1;
	day - Duration::days(days_from_monday)
}

fn local_naive_to_utc(naive: chrono::NaiveDateTime) -> Option<DateTime<Utc>> {
	match Local.from_local_datetime(&naive) {
		LocalResult::Single(local_datetime) => Some(local_datetime.with_timezone(&Utc)),
		LocalResult::Ambiguous(first, second) => Some(first.min(second).with_timezone(&Utc)),
		LocalResult::None => None,
	}
}

fn local_day_bounds_utc(day: NaiveDate) -> (DateTime<Utc>, DateTime<Utc>) {
	let start_naive = day.and_hms_opt(0, 0, 0).expect("midnight must be valid");
	let next_day = day.succ_opt().expect("next day should exist");
	let end_naive = next_day.and_hms_opt(0, 0, 0).expect("midnight must be valid");
	let start = local_naive_to_utc(start_naive).expect("local day start should be valid");
	let end = local_naive_to_utc(end_naive).expect("local day end should be valid");
	(start, end)
}

fn local_clock_on_date_to_utc(day: NaiveDate, hour: u32, minute: u32) -> Result<DateTime<Utc>, String> {
	let naive = day
		.and_hms_opt(hour, minute, 0)
		.ok_or_else(|| "invalid clock time".to_string())?;
	local_naive_to_utc(naive).ok_or_else(|| "selected local time does not exist".to_string())
}

fn shift_month(day: NaiveDate, delta: i32) -> NaiveDate {
	let mut year = day.year();
	let mut month = day.month() as i32 + delta;
	while month > 12 {
		year += 1;
		month -= 12;
	}
	while month < 1 {
		year -= 1;
		month += 12;
	}
	let month_u32 = month as u32;
	let max_day = days_in_month(year, month_u32);
	let target_day = day.day().min(max_day);
	NaiveDate::from_ymd_opt(year, month_u32, target_day).expect("shifted month date must be valid")
}

fn explorer_category_key(project_id: &str, category_id: Option<&str>) -> String {
	match category_id {
		Some(category_id) => format!("{project_id}:{category_id}"),
		None => format!("{project_id}:__uncategorized"),
	}
}

#[derive(Debug, Clone)]
enum PromptOutcome {
	NextPrompt(PromptState),
	Select(SelectState),
	Done(String),
}

#[derive(Debug, Clone)]
enum SelectOutcome {
	NextPrompt(PromptState),
	NextSelect(SelectState),
	Done(String),
}

#[derive(Debug, Clone)]
struct PromptState {
	title: String,
	input: String,
	kind: PromptKind,
}

impl PromptState {
	fn new(title: impl Into<String>, kind: PromptKind) -> Self {
		Self {
			title: title.into(),
			input: String::new(),
			kind,
		}
	}
}

#[derive(Debug, Clone)]
struct SelectState {
	title: String,
	options: Vec<SelectOption>,
	selected: usize,
	kind: SelectKind,
}

impl SelectState {
	fn new(title: impl Into<String>, kind: SelectKind, options: Vec<SelectOption>) -> Self {
		Self {
			title: title.into(),
			options,
			selected: 0,
			kind,
		}
	}

	fn move_selection(&mut self, delta: i32) {
		if self.options.is_empty() {
			self.selected = 0;
			return;
		}

		if delta > 0 {
			self.selected = (self.selected + delta as usize).min(self.options.len() - 1);
		} else {
			self.selected = self.selected.saturating_sub(delta.unsigned_abs() as usize);
		}
	}

	fn selected_option(&self) -> Option<&SelectOption> {
		self.options.get(self.selected)
	}
}

#[derive(Debug, Clone)]
struct SelectOption {
	label: String,
	value: Option<String>,
	style: Style,
}

impl SelectOption {
	fn new(label: impl Into<String>, value: Option<String>, style: Style) -> Self {
		Self {
			label: label.into(),
			value,
			style,
		}
	}
}

#[derive(Debug, Clone)]
enum PromptKind {
	AddProjectName,
	AddCategoryName,
	AddCategoryDescription {
		name: String,
	},
	AddTaskDescription {
		project_id: String,
		category_id: Option<String>,
	},
	StartTaskNote {
		task_id: String,
	},
	StopTaskNote {
		task_id: String,
	},
}

#[derive(Debug, Clone)]
enum SelectKind {
	ProjectColor {
		name: String,
	},
	TaskProject,
	TaskCategory {
		project_id: String,
	},
	LedgerSwitch,
	DeleteIntervalConfirm {
		start_event_index: usize,
		stop_event_index: Option<usize>,
		task_title: String,
	},
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusPane {
	Calendar,
	Day,
	Explorer,
}

impl FocusPane {
	fn next(self) -> Self {
		match self {
			FocusPane::Calendar => FocusPane::Day,
			FocusPane::Day => FocusPane::Explorer,
			FocusPane::Explorer => FocusPane::Calendar,
		}
	}

	fn prev(self) -> Self {
		match self {
			FocusPane::Calendar => FocusPane::Explorer,
			FocusPane::Day => FocusPane::Calendar,
			FocusPane::Explorer => FocusPane::Day,
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DayField {
	Start,
	End,
}

#[derive(Debug, Clone)]
enum ExplorerMode {
	Projects,
	ProjectTasks {
		project_id: String,
		project_name: String,
	},
}

#[derive(Debug, Clone)]
enum InputMode {
	Normal,
	Prompt(PromptState),
	Select(SelectState),
}

#[derive(Debug, Clone)]
struct App {
	focus: FocusPane,
	selected_day: NaiveDate,
	calendar_month: NaiveDate,
	day_index: usize,
	day_field: DayField,
	day_edit_buffer: String,
	explorer_mode: ExplorerMode,
	explorer_index: usize,
	explorer_collapsed_categories: HashSet<String>,
	mode: InputMode,
	status: String,
}

impl Default for App {
	fn default() -> Self {
		let today = Local::now().date_naive();
		Self {
			focus: FocusPane::Explorer,
			selected_day: today,
			calendar_month: first_day_of_month(today),
			day_index: 0,
			day_field: DayField::Start,
			day_edit_buffer: String::new(),
			explorer_mode: ExplorerMode::Projects,
			explorer_index: 0,
			explorer_collapsed_categories: HashSet::new(),
			mode: InputMode::Normal,
			status: "Ready".to_string(),
		}
	}
}

impl App {
	fn clamp_selection(&mut self, view: &ViewModel) {
		if view.day_rows.is_empty() {
			self.day_index = 0;
		} else {
			self.day_index = self.day_index.min(view.day_rows.len() - 1);
		}

		if view.explorer_rows.is_empty() {
			self.explorer_index = 0;
		} else {
			self.explorer_index = self.explorer_index.min(view.explorer_rows.len() - 1);
		}
	}

	fn shift_selected_day(&mut self, delta_days: i64) {
		self.selected_day += Duration::days(delta_days);
		self.calendar_month = first_day_of_month(self.selected_day);
		self.day_index = 0;
		self.clear_day_edit_buffer();
	}

	fn shift_selected_month(&mut self, delta_months: i32) {
		self.selected_day = shift_month(self.selected_day, delta_months);
		self.calendar_month = first_day_of_month(self.selected_day);
		self.day_index = 0;
		self.clear_day_edit_buffer();
	}

	fn move_day_selection(&mut self, delta: i32, view: &ViewModel) {
		if view.day_rows.is_empty() {
			self.day_index = 0;
			return;
		}

		if delta > 0 {
			self.day_index = (self.day_index + delta as usize).min(view.day_rows.len() - 1);
		} else {
			self.day_index = self.day_index.saturating_sub(delta.unsigned_abs() as usize);
		}
		self.clear_day_edit_buffer();
	}

	fn move_explorer_selection(&mut self, delta: i32, view: &ViewModel) {
		if view.explorer_rows.is_empty() {
			self.explorer_index = 0;
			return;
		}

		if delta > 0 {
			self.explorer_index = (self.explorer_index + delta as usize).min(view.explorer_rows.len() - 1);
		} else {
			self.explorer_index = self.explorer_index.saturating_sub(delta.unsigned_abs() as usize);
		}
	}

	fn selected_task_id(&self, view: &ViewModel) -> Option<String> {
		match self.focus {
			FocusPane::Calendar => None,
			FocusPane::Day => view.day_rows.get(self.day_index).map(|row| row.task_id.clone()),
			FocusPane::Explorer => match self.selected_explorer_row_kind(view) {
				Some(ExplorerRowKind::Task { task_id, .. }) => Some(task_id),
				_ => None,
			},
		}
	}

	fn selected_active_task_id(&self, view: &ViewModel, snapshot: &LedgerSnapshot) -> Option<String> {
		let task_id = self.selected_task_id(view)?;
		if snapshot.active_tasks.contains_key(&task_id) {
			Some(task_id)
		} else {
			None
		}
	}

	fn selected_project_for_new_task(&self, view: &ViewModel) -> Option<String> {
		if let ExplorerMode::ProjectTasks { project_id, .. } = &self.explorer_mode {
			return Some(project_id.clone());
		}

		match self.selected_explorer_row_kind(view) {
			Some(ExplorerRowKind::Project { project_id, .. }) => Some(project_id),
			Some(ExplorerRowKind::Task { project_id, .. }) => Some(project_id),
			_ => None,
		}
	}

	fn selected_explorer_row_kind(&self, view: &ViewModel) -> Option<ExplorerRowKind> {
		view.explorer_rows
			.get(self.explorer_index)
			.map(|row| row.kind.clone())
	}

	fn clear_day_edit_buffer(&mut self) {
		self.day_edit_buffer.clear();
	}

	fn day_edit_hint(&self) -> String {
		let field = if self.day_field == DayField::Start {
			"start"
		} else {
			"end"
		};
		if self.day_edit_buffer.is_empty() {
			return format!("Edit {field}: type HHMM");
		}

		let mut pending = self.day_edit_buffer.clone();
		while pending.len() < 4 {
			pending.push('_');
		}
		format!("Edit {field}: {pending}")
	}
}

struct ViewModel {
	calendar_active_days: HashSet<NaiveDate>,
	day_rows: Vec<DaySessionRow>,
	day_total: Duration,
	week_stats: WeekStatsView,
	explorer_rows: Vec<ExplorerRow>,
}

#[derive(Clone)]
struct DaySessionRow {
	task_id: String,
	project_name: String,
	task_title: String,
	project_style: Style,
	note: Option<String>,
	start: DateTime<Utc>,
	stop: DateTime<Utc>,
	display_start: DateTime<Utc>,
	display_stop: DateTime<Utc>,
	start_event_index: Option<usize>,
	stop_event_index: Option<usize>,
	lane: usize,
	lane_count: usize,
}

#[derive(Clone)]
struct WeekStatsView {
	week_start: NaiveDate,
	daily: Vec<(NaiveDate, Duration)>,
	total: Duration,
	avg_per_day: Duration,
	max_day: Duration,
	active_days: usize,
	project_totals: HashMap<String, Duration>,
	top_projects: Vec<ProjectSummaryRow>,
}

#[derive(Clone)]
struct ProjectSummaryRow {
	name: String,
	style: Style,
	duration: Duration,
}

#[derive(Clone)]
struct ExplorerRow {
	line: Line<'static>,
	kind: ExplorerRowKind,
}

impl ExplorerRow {
	fn empty(text: impl Into<String>) -> Self {
		Self {
			line: Line::from(text.into()),
			kind: ExplorerRowKind::Empty,
		}
	}
}

#[derive(Debug, Clone)]
enum ExplorerRowKind {
	Empty,
	Project {
		project_id: String,
		project_name: String,
	},
	Category {
		key: String,
	},
	Task {
		task_id: String,
		project_id: String,
	},
}

struct ActiveSessionRef {
	started_at: DateTime<Utc>,
	note: Option<String>,
	start_event_index: usize,
}

struct SessionRecord {
	task_id: String,
	start: DateTime<Utc>,
	stop: DateTime<Utc>,
	note: Option<String>,
	start_event_index: Option<usize>,
	stop_event_index: Option<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TaskEventKind {
	Start,
	Stop,
}

struct TaskEventRef {
	index: usize,
	timestamp: DateTime<Utc>,
	kind: TaskEventKind,
}

pub fn print_event_log(ledger: &Ledger, limit: usize) {
	for event in ledger.events.iter().rev().take(limit) {
		let line = match &event.kind {
			EventKind::Start { task_id, note } => format!(
				"{} start {}{}",
				event.timestamp.to_rfc3339(),
				task_label(ledger, task_id),
				note
					.as_ref()
					.map(|value| format!(" note={value}"))
					.unwrap_or_default()
			),
			EventKind::Stop { task_id, note } => format!(
				"{} stop {}{}",
				event.timestamp.to_rfc3339(),
				task_label(ledger, task_id),
				note
					.as_ref()
					.map(|value| format!(" note={value}"))
					.unwrap_or_default()
			),
		};
		println!("{line}");
	}
}
