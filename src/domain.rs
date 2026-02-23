use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Duration, Local, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Utc};
use rand::{Rng, distributions::Alphanumeric, thread_rng};
use serde::{Deserialize, Serialize};

const ID_LEN: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
    pub archived: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub archived: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub project_id: String,
    pub category_id: Option<String>,
    pub description: String,
    pub archived: bool,
}

impl Task {
    pub fn short_description(&self) -> String {
        self.description
            .lines()
            .next()
            .unwrap_or("(no description)")
            .to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventKind {
    Start {
        task_id: String,
        note: Option<String>,
    },
    Stop {
        task_id: String,
        note: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeEvent {
    pub timestamp: DateTime<Utc>,
    #[serde(flatten)]
    pub kind: EventKind,
}

impl TimeEvent {
    pub fn start(
        task_id: impl Into<String>,
        timestamp: DateTime<Utc>,
        note: Option<String>,
    ) -> Self {
        Self {
            timestamp,
            kind: EventKind::Start {
                task_id: task_id.into(),
                note,
            },
        }
    }

    pub fn stop(
        task_id: impl Into<String>,
        timestamp: DateTime<Utc>,
        note: Option<String>,
    ) -> Self {
        Self {
            timestamp,
            kind: EventKind::Stop {
                task_id: task_id.into(),
                note,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerHeader {
    pub schema_version: u32,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub day_start_offset_hours: i32,
    pub projects: Vec<Project>,
    pub tasks: Vec<Task>,
    pub categories: Vec<Category>,
}

impl LedgerHeader {
    pub fn new() -> Self {
        Self {
            schema_version: 1,
            created_at: Utc::now(),
            day_start_offset_hours: 0,
            projects: Vec::new(),
            tasks: Vec::new(),
            categories: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Ledger {
    pub header: LedgerHeader,
    pub events: Vec<TimeEvent>,
}

impl Ledger {
    pub fn new() -> Self {
        Self {
            header: LedgerHeader::new(),
            events: Vec::new(),
        }
    }

    pub fn day_start_offset(&self) -> Duration {
        Duration::hours(self.header.day_start_offset_hours.into())
    }

    pub fn day_for_timestamp(&self, timestamp: DateTime<Utc>) -> NaiveDate {
        ledger_day_for_timestamp(timestamp, self.day_start_offset())
    }

    pub fn day_bounds_utc(&self, day: NaiveDate) -> (DateTime<Utc>, DateTime<Utc>) {
        ledger_day_bounds_utc(day, self.day_start_offset())
    }

    pub fn project(&self, id: &str) -> Option<&Project> {
        self.header.projects.iter().find(|project| project.id == id)
    }

    pub fn task(&self, id: &str) -> Option<&Task> {
        self.header.tasks.iter().find(|task| task.id == id)
    }

    pub fn category(&self, id: &str) -> Option<&Category> {
        self.header
            .categories
            .iter()
            .find(|category| category.id == id)
    }

    pub fn add_project(&mut self, name: String, color: Option<String>) -> String {
        let id = generate_id();
        self.header.projects.push(Project {
            id: id.clone(),
            name,
            color,
            archived: false,
        });
        id
    }

    pub fn add_category(&mut self, name: String, description: Option<String>) -> String {
        let id = generate_id();
        self.header.categories.push(Category {
            id: id.clone(),
            name,
            description,
            archived: false,
        });
        id
    }

    pub fn add_task(
        &mut self,
        project_id: String,
        category_id: Option<String>,
        description: String,
    ) -> Result<String, String> {
        if self.project(&project_id).is_none() {
            return Err(format!("project not found: {project_id}"));
        }

        if let Some(category_id) = &category_id {
            if self.category(category_id).is_none() {
                return Err(format!("category not found: {category_id}"));
            }
        }

        let id = generate_id();
        self.header.tasks.push(Task {
            id: id.clone(),
            project_id,
            category_id,
            description,
            archived: false,
        });

        Ok(id)
    }

    pub fn start_task(
        &mut self,
        task_id: &str,
        timestamp: DateTime<Utc>,
        note: Option<String>,
    ) -> Result<(), String> {
        let task = self
            .task(task_id)
            .ok_or_else(|| format!("task not found: {task_id}"))?;

        if task.archived {
            return Err(format!("task is archived: {task_id}"));
        }

        let snapshot = self.snapshot(timestamp);
        if snapshot.active_tasks.contains_key(task_id) {
            return Err(format!("task already running: {task_id}"));
        }

        self.events
            .push(TimeEvent::start(task_id.to_string(), timestamp, note));
        Ok(())
    }

    pub fn stop_task(
        &mut self,
        task_id: &str,
        timestamp: DateTime<Utc>,
        note: Option<String>,
    ) -> Result<(), String> {
        if self.task(task_id).is_none() {
            return Err(format!("task not found: {task_id}"));
        }

        let snapshot = self.snapshot(timestamp);
        if !snapshot.active_tasks.contains_key(task_id) {
            return Err(format!("task is not running: {task_id}"));
        }

        self.events
            .push(TimeEvent::stop(task_id.to_string(), timestamp, note));
        Ok(())
    }

    pub fn snapshot(&self, now: DateTime<Utc>) -> LedgerSnapshot {
        let mut events = self.events.clone();
        events.sort_by_key(|event| event.timestamp);

        let mut active_tasks: HashMap<String, ActiveSession> = HashMap::new();
        let mut daily_task_totals: BTreeMap<NaiveDate, HashMap<String, Duration>> = BTreeMap::new();
        let day_start_offset = self.day_start_offset();

        for event in events {
            match event.kind {
                EventKind::Start { task_id, .. } => {
                    active_tasks.insert(
                        task_id,
                        ActiveSession {
                            started_at: event.timestamp,
                        },
                    );
                }
                EventKind::Stop { task_id, .. } => {
                    if let Some(active_session) = active_tasks.remove(&task_id) {
                        accumulate_session(
                            &mut daily_task_totals,
                            &task_id,
                            active_session.started_at,
                            event.timestamp,
                            day_start_offset,
                        );
                    }
                }
            }
        }

        for (task_id, active_session) in &active_tasks {
            accumulate_session(
                &mut daily_task_totals,
                task_id,
                active_session.started_at,
                now,
                day_start_offset,
            );
        }

        LedgerSnapshot {
            active_tasks,
            daily_task_totals,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ActiveSession {
    pub started_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct LedgerSnapshot {
    pub active_tasks: HashMap<String, ActiveSession>,
    pub daily_task_totals: BTreeMap<NaiveDate, HashMap<String, Duration>>,
}

impl LedgerSnapshot {
    pub fn totals_for_day(&self, day: NaiveDate) -> Vec<(String, Duration)> {
        let mut totals = self
            .daily_task_totals
            .get(&day)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .collect::<Vec<_>>();

        totals.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
        totals
    }
}

fn accumulate_session(
    daily_task_totals: &mut BTreeMap<NaiveDate, HashMap<String, Duration>>,
    task_id: &str,
    start: DateTime<Utc>,
    stop: DateTime<Utc>,
    day_start_offset: Duration,
) {
    if stop <= start {
        return;
    }

    let mut day = ledger_day_for_timestamp(start, day_start_offset);
    let last_moment = stop - Duration::seconds(1);
    let last_day = ledger_day_for_timestamp(last_moment, day_start_offset);

    while day <= last_day {
        let (day_start, day_end) = ledger_day_bounds_utc(day, day_start_offset);
        let slice_start = if start > day_start { start } else { day_start };
        let slice_end = if stop < day_end { stop } else { day_end };

        if slice_end > slice_start {
            add_daily_total(
                daily_task_totals,
                day,
                task_id,
                slice_end - slice_start,
            );
        }

        day = day.succ_opt().expect("next day should exist");
    }
}

fn add_daily_total(
    daily_task_totals: &mut BTreeMap<NaiveDate, HashMap<String, Duration>>,
    date: NaiveDate,
    task_id: &str,
    duration: Duration,
) {
    let task_durations = daily_task_totals.entry(date).or_default();
    *task_durations
        .entry(task_id.to_string())
        .or_insert_with(Duration::zero) += duration;
}

fn ledger_day_for_timestamp(timestamp: DateTime<Utc>, day_start_offset: Duration) -> NaiveDate {
    let local_time = timestamp.with_timezone(&Local) - day_start_offset;
    local_time.date_naive()
}

fn ledger_day_bounds_utc(
    day: NaiveDate,
    day_start_offset: Duration,
) -> (DateTime<Utc>, DateTime<Utc>) {
    let start_naive = day
        .and_hms_opt(0, 0, 0)
        .expect("midnight must be valid")
        + day_start_offset;
    let end_naive = start_naive + Duration::days(1);
    let start = local_naive_to_utc_resolved(start_naive);
    let end = local_naive_to_utc_resolved(end_naive);
    (start, end)
}

fn local_naive_to_utc(naive: NaiveDateTime) -> Option<DateTime<Utc>> {
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(local_datetime) => Some(local_datetime.with_timezone(&Utc)),
        LocalResult::Ambiguous(first, second) => Some(first.min(second).with_timezone(&Utc)),
        LocalResult::None => None,
    }
}

fn local_naive_to_utc_resolved(naive: NaiveDateTime) -> DateTime<Utc> {
    if let Some(timestamp) = local_naive_to_utc(naive) {
        return timestamp;
    }

    let mut cursor = naive + Duration::minutes(1);
    for _ in 0..120 {
        if let Some(timestamp) = local_naive_to_utc(cursor) {
            return timestamp;
        }
        cursor += Duration::minutes(1);
    }

    panic!("local day boundary does not exist");
}

pub fn generate_id() -> String {
    thread_rng()
        .sample_iter(&Alphanumeric)
        .take(ID_LEN)
        .map(char::from)
        .collect()
}

pub fn format_duration(duration: Duration) -> String {
    let total_seconds = duration.num_seconds().max(0);
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::{Ledger, format_duration};

    #[test]
    fn computes_parallel_time_independently() {
        let mut ledger = Ledger::new();
        let project = ledger.add_project("Work".to_string(), None);
        let task_a = ledger
            .add_task(project.clone(), None, "Task A".to_string())
            .expect("task should be created");
        let task_b = ledger
            .add_task(project, None, "Task B".to_string())
            .expect("task should be created");

        ledger
            .start_task(
                &task_a,
                Utc.with_ymd_and_hms(2026, 1, 1, 9, 0, 0).unwrap(),
                None,
            )
            .expect("start should work");
        ledger
            .start_task(
                &task_b,
                Utc.with_ymd_and_hms(2026, 1, 1, 9, 30, 0).unwrap(),
                None,
            )
            .expect("start should work");
        ledger
            .stop_task(
                &task_a,
                Utc.with_ymd_and_hms(2026, 1, 1, 10, 0, 0).unwrap(),
                None,
            )
            .expect("stop should work");
        ledger
            .stop_task(
                &task_b,
                Utc.with_ymd_and_hms(2026, 1, 1, 10, 30, 0).unwrap(),
                None,
            )
            .expect("stop should work");

        let now = Utc.with_ymd_and_hms(2026, 1, 1, 10, 30, 0).unwrap();
        let snapshot = ledger.snapshot(now);
        let day = ledger.day_for_timestamp(now);
        let task_totals = snapshot.totals_for_day(day);
        let total_for = |task_id: &str| {
            task_totals
                .iter()
                .find(|(id, _)| id == task_id)
                .map(|(_, duration)| *duration)
                .expect("task total")
        };
        assert_eq!(format_duration(total_for(&task_a)), "01:00:00");
        assert_eq!(format_duration(total_for(&task_b)), "01:00:00");
    }
}
