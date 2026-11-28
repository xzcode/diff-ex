# M6/M7 implementation handoff (2026-11-28)

Base: c27c8fbbb008f05d9fc1a13281af7e9eb2132586. M4/M5 passed its full CI before being merged into master.

## M6
- Exact line tokens preserve CR/LF/CRLF and final newline during planning. Common-base changes are clustered by overlap; adjacent nonempty edits are independent, unequal concurrent inserts and overlapping changes are conflicts.
- Editable base/left/right input, explicit generation, one conflict at a time, choose base/left/right or manual content, reset conflict resolution, editable result once all conflicts are resolved.
- Source edits invalidate the prepared result and clear decisions; previous output is retained until regeneration. Manual result edits and regeneration require confirmation before replacing the result. Saved workspace includes inputs, decisions, result and output metadata.
- Native export uses existing atomic save and fingerprint check, plus rejects source path aliases and hard links through same-file. Browser export is UTF-8. Output EOL and encoding are user-selectable.
- Text only. Sources are editable drafts, not written from this view. Large merges still compute in frontend memory; textarea edits can normalize line endings, and selected output EOL governs final file serialization. Three-way folder merge remains future work.

## M7
- Native ZIP/JAR/APK/WAR, 7Z, TAR, TAR.GZ/TGZ and GZIP reader, read-only streaming BLAKE3 of uncompressed file bytes, inferred parent directories, relative path matching, exact size and hash.
- Plain gzip stream compares under virtual name payload. Links/special items are not followed and marked as errors; unsafe and duplicate normalized paths and parent conflicts are rejected; ZIP central-directory duplicates are detected before indexing. Encrypted archives and unsupported compression fail rather than falsely report equality.
- Compare across formats, cancel at entry/read boundaries, filter by status or path, paginated listing, hashes/details. No extraction, no command execution, no rewrite. Password entry, archive member open/preview and archive write-back are not yet implemented. Archive decompression depends on codec memory requirements, result index is in memory.
- Browser preview explicitly disables native archive operations.

## Verification
Seoul isolated temp workspace: 39 Rust tests passed including real ZIP/TAR, gzip and 7z, source-alias protection, and the previous M1–M5 tests. The dependency lock and formatted source are committed. GitHub Actions is the authoritative source for full frontend, browser and installer status. UI catalogs contain 382 keys in twelve languages. English README remains default.
