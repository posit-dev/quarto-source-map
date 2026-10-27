# `SourceInfo::resolved_runs()`: canonical source runs for any shape

**Strand:** qsm-m9xmq12h · **Master plan:**
quarto-error-reporting `claude-notes/plans/2026-10-27-coalesce-concat-runs.md`
· **GitHub:** [quarto-error-reporting#10](https://github.com/posit-dev/quarto-error-reporting/issues/10)

## Overview

`resolve_byte_range` refuses `Concat` and `preimage_in` refuses a
`Substring` over a `Concat`, both for good reasons (their callers need one
contiguous range or byte-identity). quarto-error-reporting's coalescer
keys diagnostics on `resolve_byte_range`, so any diagnostic located in a
folded multi-line YAML scalar (a `Concat` since quarto-yaml 0.5.0) can
never be grouped across pages. Grouping only needs "which source bytes
does this location point at", for any shape.

This adds that accessor here, where `SourcePiece`'s content-vs-source
length semantics live, instead of re-deriving them in the consumer.

## Design

- `pub struct SourceRun { file_id, start, end }` (24 bytes, `Copy`,
  `Hash`).
- `SourceInfo::resolved_runs(&self) -> Option<Vec<SourceRun>>`: ordered
  runs in content order, adjacent same-file runs merged. `Concat`
  expands per piece; a `Substring` over anything is sliced through the
  pieces it covers; offsets inside a piece compose affinely, clamped to
  the piece's source length, and a piece covered to its content end
  reaches its source end (the `map_offset` exclusive-end rule). Deletion
  pieces (zero content) count only strictly inside a range. `Generated`
  follows the `Invocation` anchor like `resolve_byte_range`. Empty
  ranges give one zero-length run where `map_offset` points.
- Offset claim only, never byte identity (same caveat as
  `preimage_in`'s hull); the doc comment says so.
- Additive: 0.3.0 → 0.3.1.

## Checklist

- [x] Strand qsm-m9xmq12h, branch `resolved-runs`.
- [x] `SourceRun` + `resolved_runs` + private `collect_runs` in
      `src/source_info.rs`; `SourceRun` re-exported from `lib.rs`.
- [x] 15 unit tests (`resolved_runs_*`): Original, Substring chain,
      gapped / tiled / one-piece Concat, replacement pieces (whole and
      partial), cross-file, empty ranges, out-of-range, deletion pieces,
      Generated with/without anchor, `ProvenanceBuilder::in_parent` over
      a Concat parent.
- [x] Doc cross-references from `resolve_byte_range` and `preimage_in`.
- [x] `cargo fmt --check`, `cargo clippy --all-targets --locked -D
      warnings`, `cargo test --locked` green.
- [ ] Version bump to 0.3.1 (separate commit at the tip of the PR).
- [ ] PR opened; cross-linked to the quarto-error-reporting PR.
- [ ] Merged and published (user go-ahead required).
