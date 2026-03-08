import TOML from '@iarna/toml'

export const EVENTS_MARKER = '\n=== EVENTS ===\n'

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

export function parseEventsJsonl(raw) {
	return raw
		.split(/\r?\n/u)
		.filter((line) => line.trim().length > 0)
		.map((line) => JSON.parse(line))
}

export function parseLedger(raw) {
	const { headerBlob, eventsBlob } = splitLedgerDocument(raw)
	return {
		header: TOML.parse(headerBlob),
		events: parseEventsJsonl(eventsBlob),
	}
}

export function serializeLedger({ header, events }) {
	const headerBlob = TOML.stringify(header).trimEnd()
	const eventsBlob = events.map((event) => JSON.stringify(event)).join('\n')

	if (eventsBlob.length === 0) {
		return `${headerBlob}${EVENTS_MARKER}`
	}

	return `${headerBlob}${EVENTS_MARKER}${eventsBlob}\n`
}
