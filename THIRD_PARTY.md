# Third-party components

Diff EX's original application code is Apache-2.0. Dependencies retain their own copyright and license terms. The lockfiles identify exact versions; each package's distributed license is authoritative.

| Component | Purpose | Upstream |
| --- | --- | --- |
| Tauri / Tauri dialog plugin | Native desktop shell and dialogs | https://github.com/tauri-apps/tauri |
| Vue | UI runtime | https://github.com/vuejs/core |
| Monaco Editor | Editable diff and code editor | https://github.com/microsoft/monaco-editor |
| Lucide | UI icons | https://github.com/lucide-icons/lucide |
| similar | Native text comparison | https://github.com/mitsuhiko/similar |
| serde / serde_json | Data contracts and exact JSON parsing | https://github.com/serde-rs/serde |
| serde_yaml_ng / yaml-rust2 | YAML semantic parsing and scalar precision checks | https://crates.io/crates/serde_yaml_ng / https://crates.io/crates/yaml-rust2 |
| roxmltree | XML parsing without external entity loading | https://github.com/RazrFalcon/roxmltree |
| encoding_rs | Character encoding | https://github.com/hsivonen/encoding_rs |
| BLAKE3 | Content fingerprints | https://github.com/BLAKE3-team/BLAKE3 |
| ignore | Recursive traversal and ignore rules | https://github.com/BurntSushi/ripgrep |
| tempfile / uuid / num-bigint | Atomic temporary files, identifiers, exact decimal exponents | https://crates.io/crates/tempfile / https://crates.io/crates/uuid / https://crates.io/crates/num-bigint |
| diff / jsonc-parser | Explicitly limited browser preview | https://github.com/kpdecker/jsdiff / https://github.com/microsoft/node-jsonc-parser |

Vite, TypeScript, vue-tsc, Vitest and Playwright are development/build/verification dependencies. This component list is informational; it does not replace the license texts shipped by packages or a complete distribution audit.

## Spreadsheet reader

- Calamine 0.38.0 (`calamine`, MIT): native read-only Excel and OpenDocument workbook parsing. Dependency versions and transitive packages are recorded in Cargo.lock. Spreadsheet macros are not executed.

## Archive comparison

- zip 9.8.x, tar 0.4.x, flate2 1.x and sevenz-rust2 0.27.x: read archive streams; the exact resolved dependencies are recorded in Cargo.lock. Sevenz compression is enabled only for tests; production does not extract or rewrite archive members. See each crate's license metadata.
- same-file 1.x: cross-platform file identity checks for three-way result save targets.

## Git, SSH and document comparison

- git2 0.21 (MIT/Apache-2.0), vendored libgit2: read local Git objects and working trees.
- ssh2 0.9 (MIT/Apache-2.0), libssh2 and vendored OpenSSL: read-only SFTP with pinned server host keys.
- lopdf 0.47 (MIT): PDF extraction. Optional system antiword and pdftoppm/Poppler executables are not redistributed with the app. Users install those separately. See Cargo.lock for exact dependency versions.

## Database drivers

- rusqlite 0.43 (MIT), bundled SQLite; mysql 28 (MIT/Apache-2.0); postgres 0.19 and postgres-native-tls (MIT/Apache-2.0); mongodb 3.10 (Apache-2.0), BSON 2.x; native-tls and Tokio runtime. Exact transitive versions are in Cargo.lock.
