---
status: done
depends_on: ["0009"]
---

# 0010: Read a document with content hash

**Epic:** [0002 Document store](../description.md) · **Depends on:** 0009 · **Milestone:** M1

## Goal

Return a document's exact bytes as UTF-8 text plus a stable SHA-256 hash the
client sends back on save.

## Build

- `src/store/mod.rs`: `DocStore::read(&self, path: &DocPath) -> Result<Document, StoreError>`
  with `Document { path, content: String, hash: String }`.
- Resolve with `Existing`; read bytes; reject invalid UTF-8 with a clear
  `InvalidUtf8` error; hash the bytes with `sha2`, hex-encoded lowercase.
- Add `sha2` and `tokio` dependencies.
- Map `NotFound` from the OS and from resolution.

## Already in place

- `DocPath` and safe resolution (0008, 0009).

## Acceptance criteria

- [x] Content is returned byte-for-byte as UTF-8: no line-ending normalization,
  no BOM stripping, no trimming.
- [x] The hash is deterministic and identical for identical bytes.
- [x] Missing files return `NotFound`.
- [x] Invalid UTF-8 returns a distinct error, not a panic or lossy string.
- [x] Hashing does not run on the async worker for large files
  (`spawn_blocking` above a small threshold).

## Tests

- **Unit:** known SHA-256 of `"# PRD\n"` and of an empty string.
- **Unit:** CRLF and trailing whitespace survive a read.
- **Integration:** read a file with a BOM and assert the bytes match.
- **Unit:** missing file and invalid UTF-8 cases.
- **Unit:** two reads return the same hash.

## Gotchas

- Hash the raw bytes, not the `String`, or a later normalization silently
  changes conflict detection.
- Do not use the path string as part of the hash; only content matters.
- Keep the hash format in one helper so 0011 and the proposal service compare
  like with like.
- Avoid loading a multi-megabyte file on every tree refresh; 0013 reads only
  metadata and the title prefix.
