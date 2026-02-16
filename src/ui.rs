use std::error::Error;
use std::io;
use std::path::Path;
use std::time::Duration as StdDuration;

use chrono::{DateTime, Utc};
use crossterm::event::{self, Event as CEvent, KeyCode, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{execute, ExecutableCommand};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::{Frame, Terminal};

use crate::domain::{format_duration, EventKind, Ledger, LedgerSnapshot};
use crate::storage::save_ledger;

pub fn run_dashboard(ledger: &mut Ledger, ledger_path: &Path) -> Result<(), Box<dyn Error>> {
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
	ledger_path: &Path,
) -> Result<(), Box<dyn Error>> {
	let mut app = App::default();

	loop {
		let now = Utc::now();
		let snapshot = ledger.snapshot(now);
		let view = build_view(ledger, &snapshot, now);
		app.clamp_selection(&view);
		terminal.draw(|frame| draw_dashboard(frame, &app, ledger, &snapshot, &view, now))?;

		if event::poll(StdDuration::from_millis(250))? {
			if let CEvent::Key(key) = event::read()? {
				if key.kind != KeyEventKind::Press {
					continue;
				}

				let should_quit = if matches!(app.mode, InputMode::Prompt(_)) {
					handle_prompt_key(&mut app, key.code, ledger, ledger_path)
				} else {
					handle_normal_key(&mut app, key.code, ledger, ledger_path, &snapshot, &view)
				};

				if should_quit {
					break;
				}
			}
		}
	}

	Ok(())
}

fn draw_dashboard(
	frame: &mut Frame,
	app: &App,
	ledger: &Ledger,
	snapshot: &LedgerSnapshot,
	view: &ViewModel,
	now: DateTime<Utc>,
) {
	let layout = Layout::default()
		.direction(Direction::Vertical)
		.constraints([
			Constraint::Length(3),
			Constraint::Min(12),
			Constraint::Length(8),
			Constraint::Length(4),
		])
		.split(frame.area());

	let header = Paragraph::new(format!(
		"chronos-timeledger | {} | projects={} tasks={} active={} tracked={}",
		now.format("%Y-%m-%d %H:%M:%S UTC"),
		ledger.header.projects.len(),
		ledger.header.tasks.len(),
		snapshot.active_tasks.len(),
		format_duration(snapshot.total_tracked()),
	))
	.block(Block::default().borders(Borders::ALL).title("Dashboard"))
	.style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
	frame.render_widget(header, layout[0]);

	let middle = Layout::default()
		.direction(Direction::Horizontal)
		.constraints([
			Constraint::Percentage(34),
			Constraint::Percentage(33),
			Constraint::Percentage(33),
		])
		.split(layout[1]);

	render_list_panel(
		frame,
		middle[0],
		"Running",
		app.focus == FocusPane::Running,
		&view.running_rows,
		app.running_index,
	);
	render_list_panel(
		frame,
		middle[1],
		"Recent",
		app.focus == FocusPane::Recent,
		&view.recent_rows,
		app.recent_index,
	);
	render_list_panel(
		frame,
		middle[2],
		"Tasks",
		app.focus == FocusPane::Tasks,
		&view.task_rows,
		app.task_index,
	);

	let lower = Layout::default()
		.direction(Direction::Horizontal)
		.constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
		.split(layout[2]);

	let day_lines = if view.day_rows.is_empty() {
		vec![Line::from("No tracked sessions today")]
	} else {
		view.day_rows.iter().map(|line| Line::from(line.clone())).collect()
	};
	let day = Paragraph::new(day_lines)
		.block(Block::default().borders(Borders::ALL).title("Day Summary"));
	frame.render_widget(day, lower[0]);

	let event_items = if view.event_rows.is_empty() {
		vec![ListItem::new("No events yet")]
	} else {
		view.event_rows
			.iter()
			.map(|line| ListItem::new(line.clone()))
			.collect::<Vec<_>>()
	};
	let events = List::new(event_items).block(Block::default().borders(Borders::ALL).title("Recent Events"));
	frame.render_widget(events, lower[1]);

	let footer_lines = match &app.mode {
		InputMode::Normal => vec![
			Line::from("Tab focus | j/k or arrows move | Enter start/stop | q quit"),
			Line::from("p project | c category | t task | s start(note) | x stop(note) | l manual log"),
			Line::from(app.status.clone()),
		],
		InputMode::Prompt(prompt) => vec![
			Line::from(prompt.title.clone()),
			Line::from(format!("> {}", prompt.input)),
			Line::from("Enter submit | Esc cancel"),
		],
	};

	let footer = Paragraph::new(footer_lines).block(Block::default().borders(Borders::ALL).title("Input"));
	frame.render_widget(footer, layout[3]);
}

fn render_list_panel(
	frame: &mut Frame,
	area: ratatui::layout::Rect,
	title: &str,
	focused: bool,
	rows: &[String],
	selected_index: usize,
) {
	let items = if rows.is_empty() {
		vec![ListItem::new("(empty)")]
	} else {
		rows.iter().map(|row| ListItem::new(row.clone())).collect::<Vec<_>>()
	};

	let block = Block::default()
		.borders(Borders::ALL)
		.title(title)
		.border_style(if focused {
			Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
		} else {
			Style::default()
		});

	let list = List::new(items)
		.block(block)
		.highlight_style(Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD));

	let mut state = ListState::default();
	if !rows.is_empty() {
		state.select(Some(selected_index.min(rows.len().saturating_sub(1))));
	}
	frame.render_stateful_widget(list, area, &mut state);
}

fn handle_normal_key(
	app: &mut App,
	code: KeyCode,
	ledger: &mut Ledger,
	ledger_path: &Path,
	snapshot: &LedgerSnapshot,
	view: &ViewModel,
) -> bool {
	match code {
		KeyCode::Char('q') | KeyCode::Esc => true,
		KeyCode::Tab => {
			app.focus = app.focus.next();
			false
		}
		KeyCode::BackTab => {
			app.focus = app.focus.prev();
			false
		}
		KeyCode::Up | KeyCode::Char('k') => {
			app.move_selection(-1, view);
			false
		}
		KeyCode::Down | KeyCode::Char('j') => {
			app.move_selection(1, view);
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
			app.mode = InputMode::Prompt(PromptState::new("Task project id", PromptKind::AddTaskProject));
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
		KeyCode::Char('l') => {
			if let Some(task_id) = app.selected_task_id(view) {
				app.mode = InputMode::Prompt(PromptState::new(
					"Manual log start (RFC3339)",
					PromptKind::ManualLogStart { task_id },
				));
			} else {
				app.status = "Select a task first".to_string();
			}
			false
		}
		KeyCode::Enter => {
			if let Some(task_id) = app.selected_task_id(view) {
				let result = if snapshot.active_tasks.contains_key(&task_id) {
					stop_task(ledger, ledger_path, &task_id, None)
				} else {
					start_task(ledger, ledger_path, &task_id, None)
				};
				app.status = match result {
					Ok(message) => message,
					Err(err) => format!("error: {err}"),
				};
			} else {
				app.status = "Select a task first".to_string();
			}
			false
		}
		_ => false,
	}
}

fn handle_prompt_key(
	app: &mut App,
	code: KeyCode,
	ledger: &mut Ledger,
	ledger_path: &Path,
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
				InputMode::Normal => return false,
			};

			match submit_prompt(prompt.clone(), ledger, ledger_path) {
				Ok(PromptOutcome::Next(next_prompt)) => app.mode = InputMode::Prompt(next_prompt),
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

fn submit_prompt(
	prompt: PromptState,
	ledger: &mut Ledger,
	ledger_path: &Path,
) -> Result<PromptOutcome, String> {
	match prompt.kind {
		PromptKind::AddProjectName => {
			let name = required_text(&prompt.input, "project name")?;
			Ok(PromptOutcome::Next(PromptState::new(
				"Project color (optional)",
				PromptKind::AddProjectColor { name },
			)))
		}
		PromptKind::AddProjectColor { name } => {
			let color = optional_text(&prompt.input);
			let project_id = ledger.add_project(name, color);
			persist(ledger_path, ledger)?;
			Ok(PromptOutcome::Done(format!("created project {project_id}")))
		}
		PromptKind::AddCategoryName => {
			let name = required_text(&prompt.input, "category name")?;
			Ok(PromptOutcome::Next(PromptState::new(
				"Category description (optional)",
				PromptKind::AddCategoryDescription { name },
			)))
		}
		PromptKind::AddCategoryDescription { name } => {
			let description = optional_text(&prompt.input);
			let category_id = ledger.add_category(name, description);
			persist(ledger_path, ledger)?;
			Ok(PromptOutcome::Done(format!("created category {category_id}")))
		}
		PromptKind::AddTaskProject => {
			let project_id = required_text(&prompt.input, "project id")?;
			if ledger.project(&project_id).is_none() {
				return Err(format!("project not found: {project_id}"));
			}
			Ok(PromptOutcome::Next(PromptState::new(
				"Task category id (optional)",
				PromptKind::AddTaskCategory { project_id },
			)))
		}
		PromptKind::AddTaskCategory { project_id } => {
			let category_id = optional_text(&prompt.input);
			if let Some(category_id) = &category_id {
				if ledger.category(category_id).is_none() {
					return Err(format!("category not found: {category_id}"));
				}
			}
			Ok(PromptOutcome::Next(PromptState::new(
				"Task description",
				PromptKind::AddTaskDescription {
					project_id,
					category_id,
				},
			)))
		}
		PromptKind::AddTaskDescription {
			project_id,
			category_id,
		} => {
			let description = required_text(&prompt.input, "task description")?;
			let task_id = ledger.add_task(project_id, category_id, description)?;
			persist(ledger_path, ledger)?;
			Ok(PromptOutcome::Done(format!("created task {task_id}")))
		}
		PromptKind::StartTaskNote { task_id } => {
			let note = optional_text(&prompt.input);
			start_task(ledger, ledger_path, &task_id, note).map(PromptOutcome::Done)
		}
		PromptKind::StopTaskNote { task_id } => {
			let note = optional_text(&prompt.input);
			stop_task(ledger, ledger_path, &task_id, note).map(PromptOutcome::Done)
		}
		PromptKind::ManualLogStart { task_id } => {
			let start = parse_datetime(required_text(&prompt.input, "start timestamp")?.as_str())?;
			Ok(PromptOutcome::Next(PromptState::new(
				"Manual log stop (RFC3339)",
				PromptKind::ManualLogStop { task_id, start },
			)))
		}
		PromptKind::ManualLogStop { task_id, start } => {
			let stop = parse_datetime(required_text(&prompt.input, "stop timestamp")?.as_str())?;
			Ok(PromptOutcome::Next(PromptState::new(
				"Manual log note (optional)",
				PromptKind::ManualLogNote {
					task_id,
					start,
					stop,
				},
			)))
		}
		PromptKind::ManualLogNote {
			task_id,
			start,
			stop,
		} => {
			let note = optional_text(&prompt.input);
			ledger.add_manual_session(&task_id, start, stop, note)?;
			persist(ledger_path, ledger)?;
			Ok(PromptOutcome::Done(format!("recorded manual session for {task_id}")))
		}
	}
}

fn build_view(ledger: &Ledger, snapshot: &LedgerSnapshot, now: DateTime<Utc>) -> ViewModel {
	let mut running_rows = Vec::new();
	let mut running_ids = Vec::new();
	let mut running_entries = snapshot
		.active_tasks
		.iter()
		.map(|(task_id, active)| (task_id.clone(), active.started_at, active.note.clone()))
		.collect::<Vec<_>>();
	running_entries.sort_by_key(|(_, started_at, _)| *started_at);
	for (task_id, started_at, note) in running_entries {
		let task = ledger.task(&task_id);
		let title = task
			.map(|task| task.short_description())
			.unwrap_or_else(|| "Unknown task".to_string());
		let elapsed = format_duration(now - started_at);
		let note = note.map(|note| format!(" note={note}")).unwrap_or_default();
		running_rows.push(format!("{elapsed} | {task_id} | {title}{note}"));
		running_ids.push(task_id);
	}

	let mut recent_rows = Vec::new();
	let mut recent_ids = Vec::new();
	for task_id in snapshot.recent_tasks.iter().take(30) {
		let task = ledger.task(task_id);
		let title = task
			.map(|task| task.short_description())
			.unwrap_or_else(|| "Unknown task".to_string());
		let today_total = format_duration(snapshot.total_for_day(now.date_naive(), task_id));
		recent_rows.push(format!("{task_id} | {title} | today {today_total}"));
		recent_ids.push(task_id.clone());
	}

	let mut tasks = ledger
		.header
		.tasks
		.iter()
		.filter(|task| !task.archived)
		.collect::<Vec<_>>();
	tasks.sort_by(|left, right| {
		left.project_id
			.cmp(&right.project_id)
			.then_with(|| left.short_description().cmp(&right.short_description()))
	});

	let mut task_rows = Vec::new();
	let mut task_ids = Vec::new();
	for task in tasks {
		let project_name = ledger
			.project(&task.project_id)
			.map(|project| project.name.clone())
			.unwrap_or_else(|| "Unknown project".to_string());
		let running = if snapshot.active_tasks.contains_key(&task.id) {
			"RUN"
		} else {
			"   "
		};
		task_rows.push(format!(
			"{running} | {} | {} | {}",
			task.id,
			project_name,
			task.short_description()
		));
		task_ids.push(task.id.clone());
	}

	let mut day_rows = Vec::new();
	for (task_id, duration) in snapshot.totals_for_day(now.date_naive()).into_iter().take(8) {
		let title = ledger
			.task(&task_id)
			.map(|task| task.short_description())
			.unwrap_or_else(|| "Unknown task".to_string());
		day_rows.push(format!("{} | {} | {}", format_duration(duration), task_id, title));
	}

	let event_rows = ledger
		.events
		.iter()
		.rev()
		.take(12)
		.map(|event| match &event.kind {
			EventKind::Start { task_id, note } => format!(
				"{} start {}{}",
				event.timestamp.format("%H:%M:%S"),
				task_id,
				note
					.as_ref()
					.map(|value| format!(" note={value}"))
					.unwrap_or_default()
			),
			EventKind::Stop { task_id, note } => format!(
				"{} stop {}{}",
				event.timestamp.format("%H:%M:%S"),
				task_id,
				note
					.as_ref()
					.map(|value| format!(" note={value}"))
					.unwrap_or_default()
			),
		})
		.collect::<Vec<_>>();

	ViewModel {
		running_rows,
		running_ids,
		recent_rows,
		recent_ids,
		task_rows,
		task_ids,
		day_rows,
		event_rows,
	}
}

fn start_task(
	ledger: &mut Ledger,
	ledger_path: &Path,
	task_id: &str,
	note: Option<String>,
) -> Result<String, String> {
	ledger.start_task(task_id, Utc::now(), note)?;
	persist(ledger_path, ledger)?;
	Ok(format!("started {task_id}"))
}

fn stop_task(
	ledger: &mut Ledger,
	ledger_path: &Path,
	task_id: &str,
	note: Option<String>,
) -> Result<String, String> {
	ledger.stop_task(task_id, Utc::now(), note)?;
	persist(ledger_path, ledger)?;
	Ok(format!("stopped {task_id}"))
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

fn parse_datetime(input: &str) -> Result<DateTime<Utc>, String> {
	DateTime::parse_from_rfc3339(input)
		.map(|datetime| datetime.with_timezone(&Utc))
		.map_err(|_| "invalid datetime, expected RFC3339".to_string())
}

#[derive(Debug, Clone)]
enum PromptOutcome {
	Next(PromptState),
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
enum PromptKind {
	AddProjectName,
	AddProjectColor {
		name: String,
	},
	AddCategoryName,
	AddCategoryDescription {
		name: String,
	},
	AddTaskProject,
	AddTaskCategory {
		project_id: String,
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
	ManualLogStart {
		task_id: String,
	},
	ManualLogStop {
		task_id: String,
		start: DateTime<Utc>,
	},
	ManualLogNote {
		task_id: String,
		start: DateTime<Utc>,
		stop: DateTime<Utc>,
	},
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusPane {
	Running,
	Recent,
	Tasks,
}

impl FocusPane {
	fn next(self) -> Self {
		match self {
			FocusPane::Running => FocusPane::Recent,
			FocusPane::Recent => FocusPane::Tasks,
			FocusPane::Tasks => FocusPane::Running,
		}
	}

	fn prev(self) -> Self {
		match self {
			FocusPane::Running => FocusPane::Tasks,
			FocusPane::Recent => FocusPane::Running,
			FocusPane::Tasks => FocusPane::Recent,
		}
	}
}

#[derive(Debug, Clone)]
enum InputMode {
	Normal,
	Prompt(PromptState),
}

#[derive(Debug, Clone)]
struct App {
	focus: FocusPane,
	running_index: usize,
	recent_index: usize,
	task_index: usize,
	mode: InputMode,
	status: String,
}

impl Default for App {
	fn default() -> Self {
		Self {
			focus: FocusPane::Tasks,
			running_index: 0,
			recent_index: 0,
			task_index: 0,
			mode: InputMode::Normal,
			status: "Ready".to_string(),
		}
	}
}

impl App {
	fn clamp_selection(&mut self, view: &ViewModel) {
		if view.running_ids.is_empty() {
			self.running_index = 0;
		} else {
			self.running_index = self.running_index.min(view.running_ids.len() - 1);
		}

		if view.recent_ids.is_empty() {
			self.recent_index = 0;
		} else {
			self.recent_index = self.recent_index.min(view.recent_ids.len() - 1);
		}

		if view.task_ids.is_empty() {
			self.task_index = 0;
		} else {
			self.task_index = self.task_index.min(view.task_ids.len() - 1);
		}
	}

	fn move_selection(&mut self, delta: i32, view: &ViewModel) {
		let (index, len) = match self.focus {
			FocusPane::Running => (&mut self.running_index, view.running_ids.len()),
			FocusPane::Recent => (&mut self.recent_index, view.recent_ids.len()),
			FocusPane::Tasks => (&mut self.task_index, view.task_ids.len()),
		};

		if len == 0 {
			*index = 0;
			return;
		}

		if delta > 0 {
			*index = (*index + delta as usize).min(len - 1);
		} else {
			*index = index.saturating_sub(delta.unsigned_abs() as usize);
		}
	}

	fn selected_task_id(&self, view: &ViewModel) -> Option<String> {
		match self.focus {
			FocusPane::Running => view.running_ids.get(self.running_index).cloned(),
			FocusPane::Recent => view.recent_ids.get(self.recent_index).cloned(),
			FocusPane::Tasks => view.task_ids.get(self.task_index).cloned(),
		}
	}

	fn selected_active_task_id(&self, view: &ViewModel, snapshot: &LedgerSnapshot) -> Option<String> {
		if self.focus == FocusPane::Running {
			return view.running_ids.get(self.running_index).cloned();
		}

		let task_id = self.selected_task_id(view)?;
		if snapshot.active_tasks.contains_key(&task_id) {
			Some(task_id)
		} else {
			None
		}
	}
}

struct ViewModel {
	running_rows: Vec<String>,
	running_ids: Vec<String>,
	recent_rows: Vec<String>,
	recent_ids: Vec<String>,
	task_rows: Vec<String>,
	task_ids: Vec<String>,
	day_rows: Vec<String>,
	event_rows: Vec<String>,
}

pub fn print_event_log(ledger: &Ledger, limit: usize) {
	for event in ledger.events.iter().rev().take(limit) {
		let line = match &event.kind {
			EventKind::Start { task_id, note } => format!(
				"{} start {}{}",
				event.timestamp.to_rfc3339(),
				task_id,
				note
					.as_ref()
					.map(|value| format!(" note={value}"))
					.unwrap_or_default()
			),
			EventKind::Stop { task_id, note } => format!(
				"{} stop {}{}",
				event.timestamp.to_rfc3339(),
				task_id,
				note
					.as_ref()
					.map(|value| format!(" note={value}"))
					.unwrap_or_default()
			),
		};
		println!("{line}");
	}
}
