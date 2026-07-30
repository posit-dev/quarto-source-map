# quarto-source-map

[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/posit-dev/quarto-source-map)

Unified source-location tracking with byte-range provenance, for parsers and
diagnostics.

`quarto-source-map` records where a piece of text came from and follows it
through transformation chains (extraction, concatenation, normalization), so a
position in derived text can be mapped back to a line/column in the original
source file. It is the source-location substrate underneath Quarto's diagnostics,
but it has no Quarto-specific dependencies and is usable on its own.

## Core types

- [`SourceInfo`] — a location plus its transformation history.
- [`SourceContext`] — registers files and provides the content needed to map
  offsets to `row`/`column`.
- [`MappedLocation`] — the result of mapping an offset back through the chain.

## Example

```rust
use quarto_source_map::*;

// Create a context and register a file.
let mut ctx = SourceContext::new();
let file_id = ctx.add_file("main.qmd".into(), Some("# Hello\nWorld".into()));

// A source location stores only offsets…
let info = SourceInfo::original(file_id, 0, 7);

// …and maps to row/column on demand.
let mapped = info.map_offset(0, &ctx).unwrap();
assert_eq!(mapped.location.row, 0);
assert_eq!(mapped.location.column, 0);
```

## Releasing

Releases are made by merging a version-bump PR: bump `version` in
`Cargo.toml`, open a PR, merge. CI (`.github/workflows/release.yml`) then
tests at the release commit, publishes to crates.io via [Trusted
Publishing](https://crates.io/docs/trusted-publishing), tags `vX.Y.Z`, and
creates a GitHub Release. Pushes that don't change the version never
publish. A failed release run is safe to re-run (already-published steps
skip). To exercise the pipeline without releasing: Actions → Release → Run
workflow with `dry_run`.

## License

MIT © Posit Software, PBC

[`SourceInfo`]: https://docs.rs/quarto-source-map/latest/quarto_source_map/source_info/enum.SourceInfo.html
[`SourceContext`]: https://docs.rs/quarto-source-map/latest/quarto_source_map/context/struct.SourceContext.html
[`MappedLocation`]: https://docs.rs/quarto-source-map/latest/quarto_source_map/mapping/struct.MappedLocation.html
