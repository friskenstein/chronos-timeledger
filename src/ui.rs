use std::error::Error;
use std::io;
use std::time::Duration as StdDuration;

use chrono::Utc;
use crossterm::event::{self, Event as CEvent, KeyCode, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{execute, ExecutableCommand};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::{Frame, Terminal};

use crate::domain::{format_duration, EventKind, Ledger, LedgerSnapshot};

pub fn run_dashboard(ledger: &Ledger) -> Result<(), Box<dyn Error>> {
	enable_raw_mode()?;
	let mut stdout = io::stdout();
	stdout.execute(EnterAlternateScreen)?;
	let backend = CrosstermBackend::new(stdout);
	let mut terminal = Terminal::new(backend)?;

	let result = run_event_loop(&mut terminal, ledger);

	disable_raw_mode()?;
	execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
	terminal.show_cursor()?;

	result
}

fn run_event_loop(
	terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
	ledger: &Ledger,
) -> Result<(), Box<dyn Error>> {
	loop {
		let now = Utc::now();
		let snapshot = ledger.snapshot(now);
		terminal.draw(|frame| draw_dashboard(frame, ledger, &snapshot, now))?;

		if event::poll(StdDuration::from_millis(250))? {
			if let CEvent::Key(key) = event::read()? {
				if key.kind == KeyEventKind::Press {
					match key.code {
						KeyCode::Char('q') | KeyCode::Esc => break,
						_ => {}
					}
				}
			}
		}
	}

	Ok(())
}

fn draw_dashboard(frame: &mut Frame, ledger: &Ledger, snapshot: &LedgerSnapshot, now: chrono::DateTime<Utc>) {
	let layout = Layout::default()
		.direction(Direction::Vertical)
		.constraints([
			Constraint::Length(3),
			Constraint::Min(12),
			Constraint::Length(10),
		])
		.split(frame.area());

	let header = Paragraph::new("chronos-timeledger | q to quit")
		.block(Block::default().borders(Borders::ALL).title("Dashboard"))
		.style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
	frame.render_widget(header, layout[0]);

	let top = Layout::default()
		.direction(Direction::Horizontal)
		.constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
		.split(layout[1]);

	let mut running_rows = snapshot
		.active_tasks
		.iter()
		.map(|(task_id, active)| {
			let task = ledger.task(task_id);
			let title = task
				.map(|task| task.short_description())
				.unwrap_or_else(|| "Unknown task".to_string());
			let project_name = task
				.and_then(|task| ledger.project(&task.project_id))
				.map(|project| project.name.clone())
				.unwrap_or_else(|| "No project".to_string());
			let elapsed = format_duration(now - active.started_at);
			let note = active
				.note
				.as_ref()
				.map(|note| format!(" | note: {note}"))
				.unwrap_or_default();
			format!("[{task_id}] {title} ({project_name}) | {elapsed}{note}")
		})
		.collect::<Vec<_>>();

	running_rows.sort();

	let mut running_items = running_rows
		.into_iter()
		.map(ListItem::new)
		.collect::<Vec<_>>();

	if running_items.is_empty() {
		running_items.push(ListItem::new("No active tasks"));
	}

	let running_list = List::new(running_items)
		.block(Block::default().borders(Borders::ALL).title("Running Tasks"));
	frame.render_widget(running_list, top[0]);

	let today = now.date_naive();
	let mut recent_items = snapshot
		.recent_tasks
		.iter()
		.take(12)
		.map(|task_id| {
			let task = ledger.task(task_id);
			let title = task
				.map(|task| task.short_description())
				.unwrap_or_else(|| "Unknown task".to_string());
			let today_total = format_duration(snapshot.total_for_day(today, task_id));
			ListItem::new(format!("[{task_id}] {title} | today {today_total}"))
		})
		.collect::<Vec<_>>();

	if recent_items.is_empty() {
		recent_items.push(ListItem::new("No recent tasks"));
	}

	let recent_list = List::new(recent_items)
		.block(Block::default().borders(Borders::ALL).title("Recent Tasks"));
	frame.render_widget(recent_list, top[1]);

	let mut summary_lines = vec![Line::from(format!("Date: {}", today.format("%Y-%m-%d")))];
	for (task_id, duration) in snapshot.totals_for_day(today).into_iter().take(8) {
		let title = ledger
			.task(&task_id)
			.map(|task| task.short_description())
			.unwrap_or_else(|| "Unknown task".to_string());
		summary_lines.push(Line::from(format!(
			"{} | {} | {}",
			format_duration(duration),
			task_id,
			title
		)));
	}

	if summary_lines.len() == 1 {
		summary_lines.push(Line::from("No tracked sessions today"));
	}

	let running_count = snapshot.active_tasks.len();
	let total_events = ledger.events.len();
	let total_tracked = snapshot
		.task_totals
		.values()
		.fold(chrono::Duration::zero(), |acc, value| acc + *value);
	summary_lines.push(Line::from(""));
	summary_lines.push(Line::from(format!(
		"Active tasks: {running_count} | Events: {total_events} | Tracked: {}",
		format_duration(total_tracked)
	)));

	let day_view = Paragraph::new(summary_lines)
		.block(Block::default().borders(Borders::ALL).title("Day View"));
	frame.render_widget(day_view, layout[2]);
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
