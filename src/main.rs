mod domain;
mod ledgers;
mod storage;
mod ui;

use std::collections::HashMap;
use std::error::Error;
use std::path::PathBuf;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use clap::{Parser, Subcommand};

use crate::domain::{format_duration, Ledger};
use crate::ledgers::{recent_ledgers, remember_ledger, resolve_ledger_path};
use crate::storage::{load_ledger, save_ledger};
use crate::ui::{print_event_log, run_dashboard};

#[derive(Debug, Parser)]
#[command(name = "chronos-timeledger", about = "Terminal-first time tracker")]
struct Cli {
	#[arg(long)]
	ledger: Option<PathBuf>,
	#[command(subcommand)]
	command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
	Init,
	Dashboard,
	AddProject {
		#[arg(long)]
		name: String,
		#[arg(long)]
		color: Option<String>,
	},
	AddCategory {
		#[arg(long)]
		name: String,
		#[arg(long)]
		description: Option<String>,
	},
	AddTask {
		#[arg(long)]
		project: String,
		#[arg(long)]
		description: String,
		#[arg(long)]
		category: Option<String>,
	},
	Start {
		#[arg(long)]
		task: String,
		#[arg(long)]
		note: Option<String>,
	},
	Stop {
		#[arg(long)]
		task: String,
		#[arg(long)]
		note: Option<String>,
	},
	Log {
		#[arg(long)]
		task: String,
		#[arg(long)]
		start: String,
		#[arg(long)]
		stop: String,
		#[arg(long)]
		note: Option<String>,
	},
	ListTasks,
	Summary {
		#[arg(long)]
		day: Option<String>,
	},
	Events {
		#[arg(long, default_value_t = 20)]
		limit: usize,
	},
	Ledgers {
		#[arg(long, default_value_t = 20)]
		limit: usize,
	},
}

fn main() {
	if let Err(err) = run() {
		eprintln!("error: {err}");
		std::process::exit(1);
	}
}

fn run() -> Result<(), Box<dyn Error>> {
	let cli = Cli::parse();

	if let Some(Command::Ledgers { limit }) = &cli.command {
		print_recent_ledgers(*limit)?;
		return Ok(());
	}

	let mut ledger_path = resolve_ledger_path(cli.ledger)?;
	let mut ledger = load_ledger(&ledger_path)?;
	if let Err(err) = remember_ledger(&ledger_path) {
		eprintln!("warning: failed to store recent ledger: {err}");
	}

	match cli.command.unwrap_or(Command::Dashboard) {
		Command::Init => {
			save_ledger(&ledger_path, &ledger)?;
			println!("initialized ledger at {}", ledger_path.display());
		}
		Command::Dashboard => {
			run_dashboard(&mut ledger, &mut ledger_path)?;
		}
		Command::AddProject { name, color } => {
			let project_id = ledger.add_project(name, color);
			save_ledger(&ledger_path, &ledger)?;
			println!("created project {project_id}");
		}
		Command::AddCategory { name, description } => {
			let category_id = ledger.add_category(name, description);
			save_ledger(&ledger_path, &ledger)?;
			println!("created category {category_id}");
		}
		Command::AddTask {
			project,
			description,
			category,
		} => {
			let task_id = ledger.add_task(project, category, description)?;
			save_ledger(&ledger_path, &ledger)?;
			println!("created task {task_id}");
		}
		Command::Start { task, note } => {
			ledger.start_task(&task, Utc::now(), note)?;
			save_ledger(&ledger_path, &ledger)?;
			println!("started {task}");
		}
		Command::Stop { task, note } => {
			ledger.stop_task(&task, Utc::now(), note)?;
			save_ledger(&ledger_path, &ledger)?;
			println!("stopped {task}");
		}
		Command::Log {
			task,
			start,
			stop,
			note,
		} => {
			let start = parse_datetime(&start)?;
			let stop = parse_datetime(&stop)?;
			ledger.add_manual_session(&task, start, stop, note)?;
			save_ledger(&ledger_path, &ledger)?;
			println!("recorded manual session for {task}");
		}
		Command::ListTasks => {
			print_tasks(&ledger);
		}
		Command::Summary { day } => {
			print_summary(&ledger, day.as_deref())?;
		}
		Command::Events { limit } => {
			print_event_log(&ledger, limit);
		}
		Command::Ledgers { .. } => {}
	}

	Ok(())
}

fn print_recent_ledgers(limit: usize) -> Result<(), Box<dyn Error>> {
	let rows = recent_ledgers(limit)?;
	if rows.is_empty() {
		println!("no recent ledgers");
		return Ok(());
	}

	for (index, path) in rows.iter().enumerate() {
		println!("{:>2}. {}", index + 1, path.display());
	}

	Ok(())
}

fn parse_datetime(input: &str) -> Result<DateTime<Utc>, Box<dyn Error>> {
	Ok(DateTime::parse_from_rfc3339(input)?.with_timezone(&Utc))
}

fn parse_day(input: Option<&str>) -> Result<NaiveDate, Box<dyn Error>> {
	if let Some(raw) = input {
		Ok(NaiveDate::parse_from_str(raw, "%Y-%m-%d")?)
	} else {
		Ok(Utc::now().date_naive())
	}
}

fn print_tasks(ledger: &Ledger) {
	if ledger.header.tasks.is_empty() {
		println!("no tasks yet");
		return;
	}

	for task in &ledger.header.tasks {
		let project = ledger
			.project(&task.project_id)
			.map(|project| project.name.clone())
			.unwrap_or_else(|| "Unknown project".to_string());
		let category = task
			.category_id
			.as_ref()
			.and_then(|id| ledger.category(id))
			.map(|category| category.name.clone())
			.unwrap_or_else(|| "Uncategorized".to_string());
		println!(
			"{} | {} | {} | {}",
			task.id,
			project,
			category,
			task.short_description()
		);
	}
}

fn print_summary(ledger: &Ledger, day: Option<&str>) -> Result<(), Box<dyn Error>> {
	let day = parse_day(day)?;
	let snapshot = ledger.snapshot(Utc::now());
	let task_totals = snapshot.totals_for_day(day);

	println!("summary for {}", day.format("%Y-%m-%d"));
	if task_totals.is_empty() {
		println!("no tracked sessions for this day");
		return Ok(());
	}

	println!("\nby task:");
	for (task_id, duration) in &task_totals {
		let task_name = ledger
			.task(task_id)
			.map(|task| task.short_description())
			.unwrap_or_else(|| "Unknown task".to_string());
		println!("{} | {} | {}", format_duration(*duration), task_id, task_name);
	}

	let mut by_project: HashMap<String, Duration> = HashMap::new();
	let mut by_category: HashMap<String, Duration> = HashMap::new();

	for (task_id, duration) in &task_totals {
		if let Some(task) = ledger.task(task_id) {
			let project = ledger
				.project(&task.project_id)
				.map(|project| project.name.clone())
				.unwrap_or_else(|| "Unknown project".to_string());
			*by_project.entry(project).or_insert_with(Duration::zero) += *duration;

			let category = task
				.category_id
				.as_ref()
				.and_then(|id| ledger.category(id))
				.map(|category| category.name.clone())
				.unwrap_or_else(|| "Uncategorized".to_string());
			*by_category.entry(category).or_insert_with(Duration::zero) += *duration;
		}
	}

	println!("\nby project:");
	for (name, duration) in sort_duration_map(by_project) {
		println!("{} | {}", format_duration(duration), name);
	}

	println!("\nby category:");
	for (name, duration) in sort_duration_map(by_category) {
		println!("{} | {}", format_duration(duration), name);
	}

	Ok(())
}

fn sort_duration_map(map: HashMap<String, Duration>) -> Vec<(String, Duration)> {
	let mut rows = map.into_iter().collect::<Vec<_>>();
	rows.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
	rows
}
