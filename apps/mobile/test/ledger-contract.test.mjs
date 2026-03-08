import assert from 'node:assert/strict'
import fs from 'node:fs/promises'
import path from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

import {
	EVENTS_MARKER,
	parseEventsJsonl,
	parseLedger,
	serializeLedger,
} from '../src/ledgerContract.mjs'
import {
	fixturesDir,
	loadContractValidators,
} from '../../../tools/contract-tests/src/validators.mjs'

const testDir = path.dirname(fileURLToPath(import.meta.url))
const mobileDir = path.resolve(testDir, '..')

test('mobile parser validates shared valid fixtures', async () => {
	const validators = await loadContractValidators()
	const header = JSON.parse(
		await fs.readFile(path.join(fixturesDir, 'valid', 'example-header.json'), 'utf8')
	)
	const events = parseEventsJsonl(
		await fs.readFile(path.join(fixturesDir, 'valid', 'example-events.jsonl'), 'utf8')
	)

	validators.validateHeader(header, 'valid/example-header.json')
	events.forEach((event, index) => {
		validators.validateEvent(event, `valid/example-events.jsonl:${index + 1}`)
	})
})

test('mobile ledger roundtrip preserves the shared contract', async () => {
	const validators = await loadContractValidators()
	const fixturePath = path.join(fixturesDir, 'roundtrip', 'example.ledger')
	const raw = await fs.readFile(fixturePath, 'utf8')
	const parsed = parseLedger(raw)
	const serialized = serializeLedger(parsed)
	const reparsed = parseLedger(serialized)

	validators.validateLedger(parsed, 'roundtrip/example.ledger')
	validators.validateLedger(reparsed, 'mobile-reserialized-example.ledger')

	assert.ok(serialized.includes(EVENTS_MARKER))
	assert.deepEqual(reparsed, parsed)
	assert.equal(serializeLedger(reparsed), serialized)
})

test('mobile package can resolve the contract parser entrypoint', async () => {
	const entrypoint = path.join(mobileDir, 'src', 'ledgerContract.mjs')
	const stat = await fs.stat(entrypoint)
	assert.ok(stat.isFile())
})
