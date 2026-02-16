# chronos-timeledger — Initial Product Spec

## Summary
chronos-timeledger is a minimalist, terminal-first time tracking toolkit built with Rust and `ratatui`. It focuses on fast, efficient tracking of time across tasks and projects, with a ledger-based text file storage model chosen for convenient parseability and synchronization across devices, and easy to copy and archive.

## Goals
- Fast, low-friction time tracking in a TUI
- Simple data model based on start/stop events (including pause/resume)
- Clear overviews of time allocation across tasks/projects
- Support for parallel (overlapping) tasks
- Ledger files are self-contained and portable

## Core Concepts
- **Ledger**: A single file containing all tasks, projects, categories, and time events. Each ledger is self-contained and can be copied independently (e.g., one ledger per year).
- **Project**: Top-level container for tasks. Projects have names and optional colors. Tasks inherit project color.
- **Task**: Long, free-form, multiline description. UI typically shows only the first line, with a preview for full text. Tasks have unique IDs for stable references.
- **Category**: Optional organizational grouping (with long, free-form description). A task can have at most one category or be uncategorized. Categories are not locked to a project.
- **Time Event**: Start/stop entries. Pause/resume is represented as stop then start events. Each event references a task ID.
- **Session Note**: Optional per-session description distinct from the task description.

## Data Model (Conceptual)
- **IDs**: Tasks, projects, and categories use unique IDs, allowing renames without altering historical events.
- **Events**: A list of timestamped start/stop events referencing task IDs. Sessions can overlap (parallel tasks), and each task accrues its full time independently.
- **Derived State**: On ledger load, replay events to compute current state (active tasks, totals). Store a snapshot in memory for quick UI access.

## Core Features
- Create, update, activate/deactivate tasks
- Start/stop time tracking for a task
- Pause/resume by recording stop/start events under the hood
- Manual log entry creation (for retroactive tracking)
- View historical log entries
- View summary statistics (by day, project, task, category)
- Quick start from recent tasks
- Parallel task tracking support
- Task reuse and quick restart via recent list or search
- Start/remove a timer for a running task

## Storage and Portability
- Ledger files are self-contained
- Text file storage format chosen for convenient parseability and synchronization across devices
- Easy to archive or copy (e.g., year-based ledgers)
- Import/copy tasks and projects between ledgers (future)
- File format: TOML header for entities + JSONL event log for time events

## UI Overview
- **Running Tasks**: List of currently active tasks (parallel supported)
- **Recent Tasks**: Quick start panel for last-used tasks
- **Task/Project Explorer**: Tree view with preview of full description
- **Day View**: Today’s tasks in editable columns (start, stop, duration, note)
- **Colors**: Terminal colors, configurable per project (tasks inherit)

## Notifications
- Optional alert/bell when a timer’s configured duration ends
- The task keeps running; UI shows a “timer ended” status (e.g., where a countdown/reverse progress bar was)

## Open Questions
- ID format: use a short base62-style ID (e.g., 8 chars)? collisions? generation strategy?
- Event ordering: allow out-of-order entries and sort by timestamp on load?
- Import/export: should ledger merging be first-class or a manual copy command?

## Example Projects
- Work
- Personal Project
- Studies
- Accounting
- Exercise
