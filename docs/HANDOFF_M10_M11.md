# M10/M11 implementation handoff

Base: 1f3def8617941a1a1e104d526f5f564da2b470a9 (M8/M9). Work branch work/m10-m11 and CI branch codex/m10-m11.

## Implementation
- Read-only SQLite, MySQL/MariaDB, PostgreSQL and MongoDB adapters; catalog listing, selected relation schema and record comparison. Native drivers, quoted identifiers, typed keys, composite key validation, no guessing on duplicate/null keys, ignored columns, missing/changed rows. SQLite opens read-only; SQL sessions use read-only transactions; Mongo uses only read operations. Network TLS is verified by default and may be explicitly disabled after warning. Passwords and passphrases are not persisted by the UI.
- Database UI with reusable connection descriptors, relation selection, schema/data modes, detailed comparison results, search/filter, paged rendering, JSON/HTML reports. No database DDL/DML or generated migration statements.
- Shared Rust batch engine and diff-ex-cli. Task manifest v1, local text/structured/binary/folder/archive/document/Git comparisons, per-task errors, complete flag, JSON/HTML, no-overwrite atomic report output, exit codes 0/1/2. GUI batch runner supports local comparison kinds except Git; tasks and options persist, results are transient unless exported.
- 13 workspaces, 5 themes and 12 locales, 473 application-owned keys.

## Verification
The first compiled batch passed 48 Rust tests, CLI execution and report/no-overwrite tests, and MySQL 8.4 real database fixture. MariaDB returned NULL for GENERATION_EXPRESSION, fixed by COALESCE. Frontend type check and 47 tests passed separately with updated translations. Subsequent CI is authoritative for other live databases and browser/packaging checks. Fixtures use disposable loopback containers and generated readonly accounts, never production credentials.

## Remaining scope
Comparison is per selected relation, not whole database synchronization. Row maps are in memory; no external sort or disk spill. PostgreSQL server text representation is type-tagged, not byte-level storage equivalence. Mongo top-level keys are matched by name and BSON nested value order/types retained. SQL index/constraint metadata is partial (e.g. functional index expressions, triggers, complete DDL, extensions); exact migration is not claimed. Use consistent snapshots/pause concurrent writes; two independent connections are not a distributed atomic snapshot. CLI does not yet accept DB/SFTP credentials or scheduling. Desktop installation/runtime remains separate from compilation. Reports can contain sensitive local data.
