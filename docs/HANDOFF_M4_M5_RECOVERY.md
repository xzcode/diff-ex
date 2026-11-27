# M4/M5 implementation and recovery

Base: a2765eba83d7bfe4c544900a810dbcc95ffb9837. The earlier Tokyo working tree /app/agent/diff-ex-m4-m5 was unreachable, so the source was reconstructed in a separate GitHub branch without changing or deleting the original directory. Calamine 0.38.0 was locked and Rust formatted and tested in an isolated Seoul temporary work directory.

## Implemented

- Native asset metadata descriptors, raw chunk IPC, streaming positional byte comparison and cancellation.
- Read-only HEX/ASCII page viewer, exact byte totals and first/last difference navigation.
- Real raster decoding and RGBA comparison with dimension accounting, tolerance, side-by-side, split, weighted overlay and heatmap.
- Strict CSV/TSV parsing, selectable encoding and delimiter, native XLS/XLSX/XLSM/XLSB/ODS sheet values, absolute worksheet coordinates, header and primary-key matching, paging and detail.
- Saved file references, persistent settings, five themes and twelve application locales (354 keys).
- Synthetic actual XLSX fixture with two worksheets; Rust tests exercise native parsing, metadata validation and chunk boundaries. Vitest covers parsing, matching, raster and workspace logic. The Chromium integration workflow exercises local browser file picks and compare interactions.

## Limitations and follow-up

- Binary uses same-offset alignment and is read-only; no byte edit or insertion alignment yet.
- Raster images load and decode in memory, compare one frame, and depend on the platform raster decoder; animated/vector content is not compared. Metadata/size guards are not cryptographic content fingerprints.
- Tables are read-only, compare cell values (including cached formula results), not formulas/formatting; no workbook write-back. CSV/workbook sheet data is held in memory. Pages bound UI rendering, not supported file size.
- Native sheet handling is implemented via Calamine; synthetic XLSX fixture has been tested. Other workbook format-specific and installed desktop runtime checks should be added.
- Browser file handles are not restored automatically. Application code does not copy asset contents into persisted sessions.

Use the actual GitHub Actions results for current platform build and Chromium verification status. Do not treat the Tokyo source tree as merged or verified.
