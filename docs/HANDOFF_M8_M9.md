# M8/M9 implementation handoff

Base f23a64dee71b557fcd5db325d5a29b69890530ee (M6/M7). The work/m8-m9 branch introduces native read-only Git, SFTP and document comparison.

## Implemented
- Git: local refs discovery, pinned tree ID for comparisons, different repos/commits/tags/branches/WORKTREE, content-hash snapshot index, selected regular-file detached text pair with revalidation. No CLI, checkout, fetch, hooks, custom diff drivers or writes.
- SFTP: saved reusable profiles, TCP/SSH handshake, strict independently verified SHA256 server pin before credentials, password/private-key/agent auth, local/remote file or directory indexing, cancellation at read boundaries, metadata validation, content hashes and revalidated detached text reads. No remote commands or writes. Connection secrets are component memory only; saved descriptors exclude them.
- Documents: DOCX/DOCM text and structural package/property comparison, PDF pages and content stream signatures, optional antiword for legacy DOC extraction and pdftoppm/Poppler for real PDF page pixel comparison. No macros, external relationships or document scripts executed by built-in extractors. DOCX aligns paragraphs by text; PDF compares page positions. Structural match is not visual equality; extraction failures are unknown.
- 11 workspace types, five themes, 12 locales and 433 application-owned strings. Bilingual README remains English default.

## Verification and limits
- Real Git repositories and synthetic DOCX/PDF fixtures in Rust tests; optional real SFTP test uses a dedicated loopback sshd fixture, separate generated keys, no user credentials.
- Browser regression verifies configuration UI and nonpersistent secret, native-only modules do not simulate native results.
- User needs to install optional local antiword and Poppler tools; Word rendering, OCR, encrypted docs, Git index-only comparison, rename detection, remote writes, jump hosts are not implemented. Large document and selected text reads require available RAM. Native codec / SSH platform acceptance is separate from browser preview.
- Check actual CI job outcomes; only report successful native integration / build / browser / installer verification when confirmed.
