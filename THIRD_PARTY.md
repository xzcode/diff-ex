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
