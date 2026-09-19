# Box the `Generated` payload so `SourceInfo` shrinks from 136 to 32 bytes

Origin: Quarto 2 (q2) strand `bd-1c085k3a`, plan
`~/rooms/room-5/q2/claude-notes/plans/2026-09-19-source-info-generated-box.md`
(discovered from `bd-w0x91nmh`, PR q2#698; measurements in q2's
`claude-notes/research/2026-09-19-topdown-traverse-alloc-perf.md`).
Braid strand: `qsm-g8ht9dod` (skein created 2026-09-19; prefix `qsm`).
Previous cross-repo change of the same kind: 0.1.4, "Borrow file content in
`SourceInfo::map_offset`" (PR #5, q2 strand `bd-jn7r22g8`).

## Overview

`SourceInfo` is 136 bytes. Three of its four variants are 24 bytes of payload;
`Generated { by: By, from: SmallVec<[Anchor; 2]> }` is 56 + 80. Every q2 AST
node carries at least one `SourceInfo` (attribute-bearing nodes carry several),
so the whole tree pays for a payload only synthesized nodes use. q2 measured
`memmove` at 27.5 % of a `q2 render` profile of the Connect docs, and its
microbenchmark shows walk cost scaling roughly linearly with element size.

Goal: move the `Generated` payload behind a `Box` so `SourceInfo` (and
`Option<SourceInfo>`) is 32 bytes, with **no change to the serialized JSON
shape**. q2's expected wins (arithmetic, no change to `quarto-pandoc-types`):
`Inline` 776 → ~360, `Block` 1552 → ~830.

This plan covers the crate side only. The q2 migration (114 `Generated {`
sites in 19 non-test files) and the follow-on releases of the two other
dependent crates are tracked in the q2 plan; the release-order constraints are
repeated below because they decide our version number.

## Assessment (verified 2026-09-19 against `main`, 0.1.4)

Probed with a throwaway test (arm64, debug; sizes are layout, not
optimization-dependent):

| type                       | bytes |
| -------------------------- | ----: |
| `SourceInfo`               |   136 |
| `Option<SourceInfo>`       |   136 |
| `By`                       |    56 |
| `Anchor`                   |    32 |
| `AnchorRole`               |    24 |
| `SmallVec<[Anchor; 2]>`    |    80 |

These match the q2 plan's numbers. After boxing, `Generated(Box<Generated>)`
is 8 bytes of payload, so the enum floor is `Original` (24) + tag = 32, and
`Option<SourceInfo>` fits in the tag niche (also 32).

Current JSON wire shape (serde externally-tagged enum, `from` skipped when
empty, `data` skipped when `Null`):

```json
{"Generated":{"by":{"kind":"sectionize"}}}
{"Generated":{"by":{"kind":"test-scaffold"}}}
{"Generated":{"by":{"kind":"shortcode","data":{"name":"meta"}},"from":[{"role":"Invocation","source_info":{"Original":{"file_id":0,"start_offset":3,"end_offset":17}}},{"role":{"Other":"ext/x/role"},"source_info":{"Original":{"file_id":1,"start_offset":0,"end_offset":2}}}]}}
```

A newtype variant `Generated(Box<Generated>)` wrapping a struct with the same
two fields serializes to exactly these bytes: serde encodes a newtype variant
as `{"Generated": <inner>}`, `Box<T>` as `T`, and the struct as `{"by", "from"}`.
The `skip_serializing_if` / `default` attributes move from the variant field to
the struct field. These three literals become the pinned wire-shape test.

Crate-internal sites that spell out the variant (the compiler will list them;
this is for sizing the work):

- `src/source_info.rs`: definition; `generated()`, `for_test()`,
  `invocation_anchor()`, `value_source_anchor()`, `anchors_with_role()`,
  `append_anchor()`; the `Generated { .. }` arms in the offset / length /
  `preimage_in` / `root_file_id` / walk methods (~12 arms); ~8 test matches.
- `src/mapping.rs:80`: one `Generated { .. }` arm.
- `src/provenance_builder.rs`: none.
- `tests/alloc_budget.rs`: none.

Public API surface that changes: the variant shape only. `By`, `Anchor`,
`AnchorRole`, and every accessor keep their signatures. Note the existing
constructor is `SourceInfo::generated(by: By)` (one argument, empty anchors);
the q2 plan sketches `generated(by, from)`, which would be a second breaking
change on top of the variant. See Design.

Downstream consumers on crates.io (from the q2 plan's registry grep,
2026-09-19): `quarto-yaml` 0.1.3 and `quarto-error-reporting` 0.2.2 depend on
this crate; neither names `Generated`, `By`, `Anchor` or `SmallVec`, so both
compile unchanged against 0.2.0 once their requirement is bumped.

## Design

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SourceInfo {
    Original { .. },            // unchanged
    Substring { .. },           // unchanged
    Concat { .. },              // unchanged
    /// Node produced by a pipeline transform. Boxed so the common
    /// variants don't pay for this one's payload; see
    /// claude-notes/plans/2026-09-19-source-info-generated-box.md.
    Generated(Box<Generated>),
}

/// Payload of [`SourceInfo::Generated`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Generated {
    pub by: By,
    #[serde(default, skip_serializing_if = "SmallVec::is_empty")]
    pub from: SmallVec<[Anchor; 2]>,
}

impl SourceInfo {
    pub fn generated(by: By) -> Self;                       // unchanged signature
    pub fn generated_with(by: By, from: impl Into<SmallVec<[Anchor; 2]>>) -> Self; // new
    pub fn as_generated(&self) -> Option<&Generated>;       // new
    pub fn as_generated_mut(&mut self) -> Option<&mut Generated>; // new
}
```

Decisions:

1. **Keep `generated(by)` as-is.** It is already public and used in q2 and
   in this crate; changing its arity would make every caller break twice for
   no layout benefit. Add `generated_with(by, from)` for the "anchors at
   construction time" case the current doc comment tells people to build by
   hand (`SourceInfo::Generated { by, from }`), which stops compiling.
2. **Struct is named `Generated`** and re-exported from `lib.rs` alongside
   `Anchor`, `AnchorRole`, `By`. Same identifier as the variant, in a
   different namespace; matches the q2 plan so its site-migration text stays
   valid. (`GeneratedInfo` is the alternative if the shadowing reads badly.)
3. **Keep `SmallVec` inside the box.** Inline capacity no longer affects
   `SourceInfo`'s size, and keeping the type avoids touching q2's 22
   `smallvec!` sites. Switching to `Vec<Anchor>` is a separate optional
   cleanup.
4. **`By` untouched.** Shrinking `By::data` is pointless once boxed.
5. **Wire shape unchanged, and pinned by test** (the three literals above),
   written and passing *before* the type changes.
6. **Size pinned by test**: `size_of::<SourceInfo>() == 32` and
   `size_of::<Option<SourceInfo>>() == 32`, written first so it fails on
   0.1.4. Lives in `src/source_info.rs`'s test module (no allocator hook
   needed, so not in `tests/alloc_budget.rs`).
7. **Version 0.2.0.** The variant's public shape changes; that is a breaking
   change under semver regardless of whether any published consumer names it.
   Consequence for q2: cargo resolves two copies of this crate if
   `quarto-yaml` / `quarto-error-reporting` still ask for `0.1.x` while q2
   asks for `0.2`, making `quarto_yaml`'s `SourceInfo` a different type from
   q2's. Release order is therefore: this crate 0.2.0 →
   `quarto-error-reporting` 0.2.3 (dep bump) → `quarto-yaml` 0.1.4 (dep bump)
   → q2 bumps all three together and confirms `cargo tree -d` shows one copy.
   Shipping as 0.1.5 to skip the two follow-on releases is possible but
   misstates semver; only with explicit sign-off.
8. **No CHANGELOG file exists in this crate** (releases are version-bump PRs
   per README). Record the change in the release PR description and in this
   plan.

Doc comments to update: the `Generated` variant docs, `generated()`'s "build
the variant directly" advice, and the module-level provenance notes that
write `Generated { by: <kind>, .. }` (`deprecated` note on the old default
constructor, `By` docs, `preimage_in` docs). Prose can keep the
`Generated { by, from }` shorthand where it describes semantics rather than
syntax.

## Checklist

### Phase 0 — tests first (must fail / pass on today's code as noted)

- [x] Size test in `src/source_info.rs`: `SourceInfo` and
      `Option<SourceInfo>` both 32 bytes. Fails on 0.1.4 (136).
- [x] Wire-shape test: serialize `generated(sectionize)`, `for_test()`, and a
      `Generated` with `Invocation` + `Other` anchors and `data`; compare to
      the three literal strings in Assessment. Round-trip each back through
      `from_str` and compare with `PartialEq`. Passes on 0.1.4.
- [x] `cargo test --locked` and doctests green as a baseline.

### Phase 1 — crate change

- [x] Add `pub struct Generated { by, from }` with the serde attributes on
      `from`; switch the variant to `Generated(Box<Generated>)`; re-export
      `Generated` from `lib.rs`.
- [x] Add `generated_with`, `as_generated`, `as_generated_mut`; keep
      `generated(by)` and `for_test()` signatures unchanged.
- [x] Migrate every `Generated { .. }` arm in `src/source_info.rs` and
      `src/mapping.rs`; migrate the in-file tests.
- [x] Update the doc comments listed under Design.
- [x] Size test passes; wire-shape test still passes unchanged; full
      `cargo test --locked`, `cargo clippy --all-targets`, `cargo fmt`
      (128 unit + 1 integration + 4 doctests, 2026-09-19). Also added
      `test_generated_with_and_as_generated` for the new API.
- [ ] Bump `Cargo.toml` to 0.2.0; open a PR on a branch (Carlos reviews,
      merges; CI publishes on merge). PR description carries the changelog.

### Phase 2 — verify against q2 before publishing (tracked in q2's plan)

- [ ] q2 builds this branch via an uncommitted `[patch.crates-io]` path
      override at `external-sources/quarto-source-map`; migrate its sites;
      `cargo nextest run --workspace` and `cargo xtask verify`; **no JSON
      `.snap` file changes** (that is the wire-compatibility check on the
      pampa side).
- [ ] q2 records `hyperfine` before/after on the release-perf build in its
      plan. Copy the headline numbers into this file once measured.
- [ ] Any API gap q2's migration surfaces (e.g. a missing accessor) comes
      back into Phase 1 before the version-bump PR merges.

### Phase 3 — release and cutover

- [ ] Merge the 0.2.0 PR; confirm the release workflow published and tagged
      `v0.2.0`.
- [ ] `quarto-error-reporting`: bump dep to `0.2`, release 0.2.3.
- [ ] `quarto-yaml`: bump dep to `0.2`, release 0.1.4.
- [ ] q2: bump all three, `cargo tree -d` shows a single
      `quarto-source-map`, migration PR merged.

## Decisions (2026-09-19, Carlos; the q2 agent reviewed and agreed)

1. **Version 0.2.0.** Semver-honest; the two follow-on releases in
   `quarto-error-reporting` and `quarto-yaml` are accepted.
2. **Constructor API.** Keep `generated(by)` unchanged; add
   `generated_with(by, from)`, `as_generated`, `as_generated_mut`. The q2
   plan's two-argument `generated(by, from)` sketch is superseded.
3. **Struct name `Generated`.**
4. **Braid.** Skein initialized in this repo (`.braid.toml` gitignored,
   skill stub at `.claude/skills/braid/`); this work is `qsm-g8ht9dod`.
