# Chronos Ledger Format v1

## Scope

The current Chronos datastore is a single self-contained `.ledger` file.
The Rust TUI reads and writes this format today. The future mobile client
should target the same bytes on disk.

JSON Schema cannot describe a file that mixes TOML and JSONL in one stream,
so the machine-readable contract is split into:

- `contracts/schemas/ledger-header.schema.json` for the parsed TOML header
- `contracts/schemas/time-event.schema.json` for each JSONL event line

## File Layout

Each ledger file has two sections:

1. A TOML header containing metadata, projects, tasks, and categories.
2. A JSONL event log, separated from the header by a literal marker line:

```text
=== EVENTS ===
```

Canonical example:

```text
schema_version = 1
created_at = "2026-03-08T09:00:00Z"
day_start_offset_hours = 4
categories = []

[[projects]]
id = "Ab12Cd34"
name = "Client Work"
color = "light_cyan"
archived = false

[[tasks]]
id = "Tsk12345"
project_id = "Ab12Cd34"
description = "Ship the shared ledger contract"
archived = false

=== EVENTS ===
{"timestamp":"2026-03-08T09:15:00Z","type":"start","task_id":"Tsk12345","note":"Schema pass"}
{"timestamp":"2026-03-08T10:00:00Z","type":"stop","task_id":"Tsk12345","note":null}
```

## Header Contract

The TOML header maps to the `LedgerHeader` Rust struct.

Required top-level fields:

- `schema_version`: integer, currently `1`
- `created_at`: RFC 3339 timestamp
- `projects`: array
- `tasks`: array
- `categories`: array

Optional top-level fields:

- `day_start_offset_hours`: integer, defaults to `0` when absent

Entity notes:

- Project IDs, task IDs, and category IDs are currently generated as 8-character base62-ish strings.
- In TOML, optional entity fields are omitted when they are unset. In the JSON schema representation of the parsed header, those same fields appear as `null`.
- The contract schema is intentionally stricter than the parser on extra keys; the current Rust loader ignores unknown fields, but new writers should not emit them.

## Event Contract

Each non-empty line after the marker is a standalone JSON object.

Required fields per line:

- `timestamp`: RFC 3339 timestamp
- `type`: `"start"` or `"stop"`
- `task_id`: task ID string
- `note`: string or `null`

The ledger version lives in the header, not on each event line. All events in
the file inherit `schema_version` from the header section.

## Parser Behavior In Current Rust Code

The storage implementation currently behaves like this:

- Missing file: returns an empty in-memory ledger.
- Empty file: returns an empty in-memory ledger.
- Missing `=== EVENTS ===` marker: parses the whole file as header and treats the event log as empty.
- Blank lines in the event section are ignored.
- Events are replayed in timestamp order when computing snapshots.

These behaviors are compatibility constraints for the mobile client unless the
Rust storage layer is changed in a coordinated format migration.
