# Contract Tests

Validation scripts for the shared Chronos datastore contract live here.

Current responsibilities:

- validate shared header and event fixtures against the JSON schemas
- validate full `.ledger` fixtures by parsing TOML + JSONL sections
- assert that intentionally invalid fixtures are rejected

Run:

```bash
pnpm --dir tools/contract-tests run validate
```
