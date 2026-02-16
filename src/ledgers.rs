use std::env;
use std::fs;
use std::io::{Error, ErrorKind, Write};
use std::path::{Path, PathBuf};

const RECENT_LEDGERS_FILE: &str = "recent_ledgers.txt";
const MAX_RECENT_LEDGERS: usize = 50;

pub fn resolve_ledger_path(cli_path: Option<PathBuf>) -> Result<PathBuf, Error> {
	if let Some(path) = cli_path {
		return Ok(absolutize(path));
	}

	if let Some(path) = env::var_os("CHRONOS_LEDGER") {
		let path = PathBuf::from(path);
		if !path.as_os_str().is_empty() {
			return Ok(absolutize(path));
		}
	}

	if let Ok(mut recent) = recent_ledgers(MAX_RECENT_LEDGERS) {
		if let Some(path) = recent.drain(..).next() {
			return Ok(path);
		}
	}

	Err(Error::new(
		ErrorKind::NotFound,
		"no ledger selected: pass --ledger <path>, set CHRONOS_LEDGER, or pick one from `ledgers`",
	))
}

pub fn remember_ledger(path: &Path) -> Result<(), std::io::Error> {
	let path = absolutize(path.to_path_buf());
	let mut entries = recent_ledgers(MAX_RECENT_LEDGERS)?;
	entries.retain(|entry| entry != &path);
	entries.insert(0, path);
	entries.truncate(MAX_RECENT_LEDGERS);
	save_recent_ledgers(&entries)
}

pub fn recent_ledgers(limit: usize) -> Result<Vec<PathBuf>, std::io::Error> {
	let path = recent_ledgers_path();
	let raw = match fs::read_to_string(path) {
		Ok(raw) => raw,
		Err(err) if err.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
		Err(err) => return Err(err),
	};

	let mut rows = Vec::new();
	for line in raw.lines() {
		let trimmed = line.trim();
		if trimmed.is_empty() {
			continue;
		}
		rows.push(PathBuf::from(trimmed));
		if rows.len() >= limit {
			break;
		}
	}

	Ok(rows)
}

fn save_recent_ledgers(entries: &[PathBuf]) -> Result<(), std::io::Error> {
	let state_dir = state_dir();
	fs::create_dir_all(&state_dir)?;

	let mut file = fs::File::create(recent_ledgers_path())?;
	for path in entries {
		writeln!(file, "{}", path.display())?;
	}

	Ok(())
}

fn recent_ledgers_path() -> PathBuf {
	state_dir().join(RECENT_LEDGERS_FILE)
}

fn state_dir() -> PathBuf {
	if let Some(path) = env::var_os("CHRONOS_STATE_DIR") {
		return PathBuf::from(path);
	}

	#[cfg(target_os = "windows")]
	{
		if let Some(path) = env::var_os("LOCALAPPDATA") {
			return PathBuf::from(path).join("chronos_timeledger");
		}
	}

	if let Some(path) = env::var_os("XDG_STATE_HOME") {
		return PathBuf::from(path).join("chronos_timeledger");
	}

	if let Some(path) = env::var_os("HOME") {
		return PathBuf::from(path)
			.join(".local")
			.join("state")
			.join("chronos_timeledger");
	}

	PathBuf::from(".chronos_timeledger")
}

fn absolutize(path: PathBuf) -> PathBuf {
	let path = if path.is_absolute() {
		path
	} else if let Ok(cwd) = env::current_dir() {
		cwd.join(path)
	} else {
		path
	};

	if path.exists() {
		fs::canonicalize(&path).unwrap_or(path)
	} else {
		path
	}
}
