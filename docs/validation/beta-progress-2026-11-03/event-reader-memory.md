# Streaming event history — 2026-11-05

Public runtime `a6a38c4d15f74be05bcd6da111ab010deb0498fc` and private pin
`3679931a99efb69c1bfdeeda777a306337ae951e` remove another reproduced memory
amplifier. This follows the [output capture repair](rust-output-memory.md).
Neither change establishes the allocation source of the live Linux loop's
[#1681 OOMs](https://github.com/Kh1ng/Parasaur/issues/1681).

## Behavior

Every full event read previously retained the complete journal text while also
decoding every event into a vector. The controller repeats full reads during
ordinary passes. The shared reader now decodes records from a buffered file,
removing that additional whole-file allocation. Full-history consumers still
receive every event: old orphan claims, gates and review evidence are not dropped.

`gah events --limit N` previously loaded all history before filtering and sorting.
It now keeps only the newest N matching records while scanning. Timestamp order,
later append order for ties, since/profile filtering and final append-order
emission are unchanged. Every line is validated, including discarded records
and a zero-limit query; corruption remains an error. Missing files remain empty.

The limited query retains N records plus the current record and selection keys.
This is not a universal byte limit: a single large event can still allocate a
large line and payload. Full controller history remains proportional to the
journal size, and all reads still scan the journal. No event is deleted, compacted
or rewritten. A durable controller projection would need separate evidence that
all recovery and gate semantics survive compaction.

## Verification

On Windows/Ubuntu-24.04 WSL, `event-reader-verification.log` under
`/home/khing/.cache/parasaur-instance-dispatch.9e0730tx` records:

- `cargo fmt -- --check` and all-target/all-feature Clippy with warnings denied.
- 12 event cases, including a new comparison against full-history selection
  with delayed timestamps, ties, other profiles, blanks/CRLF and corrupt tails.
- 197 controller cases, including recovery and gate behavior.
- Two real-CLI event query cases.

The tracked memory regression is runnable without providers or services:

```sh
python3 scripts/check-event-reader-memory.py /path/to/gah
```

It generates a 128 MiB journal in a temporary directory, queries its newest four
events, and limits only the fixture child to 192 MiB address space. The new CLI
passes and returns the correct four records. The retained pre-change CLI fails
with allocation failure under the same limit. Core dumps are disabled for that
child. This is a Linux/WSL check, not a Windows-native memory-limit test or a
production service limit.

## Real CLI measurements

Probe records: `/home/khing/.cache/parasaur-event-memory.xhl5dn_5/measurements.json`.
Both debug binaries read the same generated journal with `--limit 4`. Each record
has a 64 KiB diagnostic payload. GNU `time` measures peak RSS for the CLI child;
the generator runs separately. Output identities are checked, not just exit codes.

| Journal bytes | Records | Previous peak RSS (KiB) | Streaming peak RSS (KiB) |
| ---: | ---: | ---: | ---: |
| 33,619,858 | 512 | 92,160 | 29,500 |
| 134,480,810 | 2,048 | 290,304 | 29,760 |

Previous CLI SHA-256: `fe07bdd295997b906aef70cb3a823ea081ea42dd70ec21d71819c60485e54e31`.
Streaming CLI SHA-256: `20368c67549a1a58a8834c45d3ed97a12671ede8c9e8e486adc0be4aff1a53d3`.
These are short-lived event-query measurements, not a daemon soak or heap profile.

The matched private assembly is retained at
`/home/khing/.cache/parasaur-enterprise-current.wiu0wvho/source`. It passed
1,093 server cases (37 explicit skips), 15 mock cases, typechecks, production
builds and private input/artifact/transformed transport guards. Private runtime,
web and SQL source are unchanged; only the core pin advances. Prior dedicated
isolation and Auth/browser runs remain evidence for their recorded binaries and
revisions; they were not repeated for this event-reader change.

No installed application, live worker, service limit or hosted deployment was
changed. Live RSS/heap attribution and a multi-hour idle/dispatch soak remain
required; the Mac investigation remains deferred.
