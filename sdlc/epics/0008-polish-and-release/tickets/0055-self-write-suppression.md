---
status: todo
depends_on: ["0011", "0054"]
---

# 0055: Suppress self-write events

**Epic:** [0008 Polish and release](../description.md) · **Depends on:** 0011, 0054 · **Milestone:** M6

## Goal

Do not notify the browser about changes sdlc just made. Otherwise every save
triggers a reload prompt or a tree refresh storm.

## Build

- Add a `RecentWrites` map to the store or watcher: `path → hash` plus an
  insert timestamp, behind a mutex.
- Record the hash **before** the write completes (the content is known), so the
  watcher event cannot arrive first.
- On a watcher event, read the current hash:
  - equal to the recorded hash → suppress and remove the entry;
  - missing and a delete was recorded → suppress;
  - otherwise publish.
- Entries expire after ~5 seconds and are pruned on insert; bounded size.
- Record creates, modifies, renames (old and new paths) and deletes.
- Proposal accepts record every file they write.

## Already in place

- Watcher, atomic write with known content, read/hash.

## Acceptance criteria

- [ ] Saving from the app emits no `file-changed` event.
- [ ] A create, rename and delete from the app are silent.
- [ ] An external edit still emits an event.
- [ ] An external edit immediately after an app write is not suppressed
      forever.
- [ ] The map does not grow without bound.
- [ ] A proposal accept is silent for all its files.

## Tests

- **Integration:** app write, assert no event within the debounce window.
- **Integration:** external write, assert one event.
- **Integration:** app delete, no event; external delete, event.
- **Unit:** expiry and pruning; same hash recorded twice.
- **Integration:** two rapid app writes then an external write; only the
  external one notifies.

## Gotchas

- Recording after the write is a race; record first.
- If an external tool writes byte-identical content, suppression hides an event
  that would have no visible effect. Acceptable, but state it.
- Rename generates an event for the old path; record both.
- Global suppression means a second tab does not learn about the first tab's
  save. This is a documented single-user limitation in 0058/0061.
- Do not suppress reads or tree refreshes; only writes.
