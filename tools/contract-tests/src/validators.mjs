import fs from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import TOML from '@iarna/toml'
import Ajv2020 from 'ajv/dist/2020.js'

export const EVENTS_MARKER = '\n=== EVENTS ===\n'

const moduleDir = path.dirname(fileURLToPath(import.meta.url))
export const repoRootDir = path.resolve(moduleDir, '../../..')
export const contractsDir = path.join(repoRootDir, 'contracts')
export const fixturesDir = path.join(contractsDir, 'fixtures')
export const schemasDir = path.join(contractsDir, 'schemas')

let validatorPromise

export async function loadContractValidators() {
	if (!validatorPromise) {
		validatorPromise = createContractValidators()
	}

	return validatorPromise
}

export function parseEventsJsonl(raw) {
	return raw
		.split(/\r?\n/u)
		.filter((line) => line.trim().length > 0)
		.map((line) => JSON.parse(line))
}

export function splitLedgerDocument(raw) {
	const markerIndex = raw.indexOf(EVENTS_MARKER)
	if (markerIndex === -1) {
		return {
			headerBlob: raw,
			eventsBlob: '',
		}
	}

	return {
		headerBlob: raw.slice(0, markerIndex),
		eventsBlob: raw.slice(markerIndex + EVENTS_MARKER.length),
	}
}

export function parseLedgerDocument(raw) {
	const { headerBlob, eventsBlob } = splitLedgerDocument(raw)
	return {
		header: TOML.parse(headerBlob),
		events: parseEventsJsonl(eventsBlob),
	}
}

async function createContractValidators() {
	const ajv = new Ajv2020({
		allErrors: true,
		strict: false,
		validateFormats: false,
	})
	const [headerSchema, eventSchema] = await Promise.all([
		readJson(path.join(schemasDir, 'ledger-header.schema.json')),
		readJson(path.join(schemasDir, 'time-event.schema.json')),
	])
	const headerValidator = ajv.compile(headerSchema)
	const eventValidator = ajv.compile(eventSchema)

	return {
		validateHeader(header, label = 'header') {
			assertValid(headerValidator, header, label)
			assertDateTimeString(header.created_at, `${label}.created_at`)
			assertUniqueIds(header.projects, `${label}.projects`)
			assertUniqueIds(header.tasks, `${label}.tasks`)
			assertUniqueIds(header.categories, `${label}.categories`)
			assertTaskReferences(header, label)
			return header
		},
		validateEvent(event, label = 'event') {
			assertValid(eventValidator, event, label)
			assertDateTimeString(event.timestamp, `${label}.timestamp`)
			return event
		},
		validateLedger(ledger, label = 'ledger') {
			this.validateHeader(ledger.header, `${label}.header`)
			ledger.events.forEach((event, index) => {
				this.validateEvent(event, `${label}.events[${index}]`)
			})
			assertEventReferences(ledger, label)
			return ledger
		},
	}
}

function assertValid(validate, value, label) {
	if (validate(value)) {
		return
	}

	const detail = (validate.errors ?? [])
		.map((error) => `${error.instancePath || '/'} ${error.message}`)
		.join('; ')
	throw new Error(`${label} failed schema validation: ${detail}`)
}

function assertDateTimeString(value, label) {
	if (typeof value !== 'string' || Number.isNaN(Date.parse(value))) {
		throw new Error(`${label} must be an RFC 3339 date-time string`)
	}
}

function assertUniqueIds(rows, label) {
	const seen = new Set()
	for (const row of rows) {
		if (seen.has(row.id)) {
			throw new Error(`${label} contains duplicate id ${row.id}`)
		}
		seen.add(row.id)
	}
}

function assertTaskReferences(header, label) {
	const projectIds = new Set(header.projects.map((project) => project.id))
	const categoryIds = new Set(header.categories.map((category) => category.id))

	for (const task of header.tasks) {
		if (!projectIds.has(task.project_id)) {
			throw new Error(`${label}.tasks contains unknown project_id ${task.project_id}`)
		}

		if (task.category_id != null && !categoryIds.has(task.category_id)) {
			throw new Error(`${label}.tasks contains unknown category_id ${task.category_id}`)
		}
	}
}

function assertEventReferences(ledger, label) {
	const taskIds = new Set(ledger.header.tasks.map((task) => task.id))

	for (const event of ledger.events) {
		if (!taskIds.has(event.task_id)) {
			throw new Error(`${label} contains event for unknown task_id ${event.task_id}`)
		}
	}
}

async function readJson(filePath) {
	return JSON.parse(await fs.readFile(filePath, 'utf8'))
}
