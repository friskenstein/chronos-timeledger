import assert from 'node:assert/strict'
import fs from 'node:fs/promises'
import path from 'node:path'

import {
	fixturesDir,
	loadContractValidators,
	parseEventsJsonl,
	parseLedgerDocument,
} from './validators.mjs'

const validators = await loadContractValidators()

await validateValidFixtures()
await validateInvalidFixtures()

console.log('shared datastore contract fixtures validated')

async function validateValidFixtures() {
	const header = JSON.parse(
		await fs.readFile(path.join(fixturesDir, 'valid', 'example-header.json'), 'utf8')
	)
	const events = parseEventsJsonl(
		await fs.readFile(path.join(fixturesDir, 'valid', 'example-events.jsonl'), 'utf8')
	)
	const validLedger = parseLedgerDocument(
		await fs.readFile(path.join(fixturesDir, 'valid', 'example.ledger'), 'utf8')
	)
	const roundtripLedger = parseLedgerDocument(
		await fs.readFile(path.join(fixturesDir, 'roundtrip', 'example.ledger'), 'utf8')
	)

	validators.validateHeader(header, 'valid/example-header.json')
	events.forEach((event, index) => {
		validators.validateEvent(event, `valid/example-events.jsonl:${index + 1}`)
	})
	validators.validateLedger(validLedger, 'valid/example.ledger')
	validators.validateLedger(roundtripLedger, 'roundtrip/example.ledger')
}

async function validateInvalidFixtures() {
	const invalidHeader = JSON.parse(
		await fs.readFile(
			path.join(fixturesDir, 'invalid', 'header-missing-schema-version.json'),
			'utf8'
		)
	)
	const invalidEvent = JSON.parse(
		await fs.readFile(path.join(fixturesDir, 'invalid', 'event-missing-type.json'), 'utf8')
	)

	assert.throws(() => {
		validators.validateHeader(invalidHeader, 'invalid/header-missing-schema-version.json')
	})
	assert.throws(() => {
		validators.validateEvent(invalidEvent, 'invalid/event-missing-type.json')
	})
}
