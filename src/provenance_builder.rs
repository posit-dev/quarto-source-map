//! Shared run-tiling builder for decoders that walk a decoded string
//! lockstep against the source bytes it came from.
//!
//! `ProvenanceBuilder` is the piece of machinery three decoders share
//! (quarto-yaml's scalar walker, pampa's div-attribute unescaper,
//! comrak's CommonMark text path): feed it a sequence of
//! `verbatim`/`replacement` calls describing how content bytes were
//! produced from source bytes, and `finish()` emits a `SourceInfo` that
//! is contiguous when it safely can be, and a `Concat` otherwise.
//!
//! See `claude-notes/plans/2026-08-20-provenance-1-foundations.md`,
//! § "The shared builder", for the design this module implements.

use std::ops::Range;
use std::sync::Arc;

use crate::source_info::SourceInfo;
use crate::types::FileId;

/// Where a builder's pieces are rooted: directly in a file, or as
/// substrings of a parent `SourceInfo` (which may itself be a `Concat`).
enum Root {
    File(FileId),
    Parent(Arc<SourceInfo>),
}

/// One accumulated piece, before it is turned into a `SourcePiece`.
///
/// `verbatim` is kept only internally — it does not survive into the
/// emitted `SourceInfo` — but it is exactly what `push` needs to decide
/// whether to coalesce, and what `finish` needs to decide whether to
/// collapse. It is the caller's assertion (via which method it called),
/// never re-derived from `src_range`/`content_len`.
struct Piece {
    src_range: Range<usize>,
    content_len: usize,
    verbatim: bool,
}

/// Builds a `SourceInfo` by tiling a decoded string's content against
/// the source bytes it came from.
///
/// Construct with [`ProvenanceBuilder::in_file`] or
/// [`ProvenanceBuilder::in_parent`], describe the decode with
/// [`verbatim`](Self::verbatim) and [`replacement`](Self::replacement)
/// calls in content order, then call [`finish`](Self::finish).
pub struct ProvenanceBuilder {
    root: Root,
    anchor: usize,
    pieces: Vec<Piece>,
}

impl ProvenanceBuilder {
    /// Start a builder whose pieces are `Original` ranges directly in
    /// `file_id`.
    ///
    /// `anchor` is the scalar's span start; `finish()` uses it when the
    /// piece list ends up empty, since an empty piece list has no source
    /// range to infer a position from.
    pub fn in_file(file_id: FileId, anchor: usize) -> Self {
        Self {
            root: Root::File(file_id),
            anchor,
            pieces: Vec::new(),
        }
    }

    /// Start a builder whose pieces are `Substring` ranges over `parent`.
    ///
    /// `parent` may itself be a `Concat` (e.g. q2's cell-options path
    /// hands `quarto_yaml::parse_with_parent` a `SourceInfo::concat(..)`).
    /// The builder never resolves absolute positions out of `parent` —
    /// see [`finish`](Self::finish) — so this is safe regardless of what
    /// shape `parent` is.
    ///
    /// `anchor` is the scalar's span start, in `parent`'s coordinate
    /// space; see [`in_file`](Self::in_file) for why it is needed.
    pub fn in_parent(parent: SourceInfo, anchor: usize) -> Self {
        Self {
            root: Root::Parent(Arc::new(parent)),
            anchor,
            pieces: Vec::new(),
        }
    }

    /// Record `src_range.len()` source bytes decoding to that many
    /// content bytes, unchanged.
    ///
    /// This is a caller assertion: the builder takes byte-identity on
    /// trust and never re-derives it from lengths. Adjacent `verbatim`
    /// calls whose source ranges abut are merged; a `replacement` never
    /// coalesces with anything, however convenient its length.
    pub fn verbatim(&mut self, src_range: Range<usize>) {
        let content_len = src_range.len();
        self.push(src_range, content_len, true);
    }

    /// Record `src_range.len()` source bytes decoding to `out_len`
    /// content bytes, where source and content are not asserted to be
    /// byte-identical.
    ///
    /// `out_len == 0` is a deletion. An empty `src_range` with
    /// `out_len > 0` is synthesis: content with no corresponding source
    /// byte (e.g. a chomped block scalar's trailing newline at EOF).
    pub fn replacement(&mut self, src_range: Range<usize>, out_len: usize) {
        self.push(src_range, out_len, false);
    }

    fn push(&mut self, src_range: Range<usize>, content_len: usize, verbatim: bool) {
        if verbatim
            && let Some(last) = self.pieces.last_mut()
            && last.verbatim
            && last.src_range.end == src_range.start
        {
            last.src_range.end = src_range.end;
            last.content_len += content_len;
            return;
        }
        self.pieces.push(Piece {
            src_range,
            content_len,
            verbatim,
        });
    }

    /// Emit the tiled `SourceInfo`: `Original`/`Substring` if the piece
    /// list collapsed to exactly one verbatim piece (or is empty), a
    /// `Concat` otherwise.
    ///
    /// This never calls `resolve_byte_range` — on the parent or on
    /// anything derived from it — because `in_parent`'s parent may be a
    /// `Concat`, for which that accessor returns `None`. Positions are
    /// always built from the piece's own `src_range`, never resolved.
    pub fn finish(self) -> SourceInfo {
        #[cfg(debug_assertions)]
        {
            for pair in self.pieces.windows(2) {
                debug_assert_eq!(
                    pair[0].src_range.end, pair[1].src_range.start,
                    "ProvenanceBuilder::finish: pieces do not tile their source \
                     contiguously (gap or overlap between adjacent pieces)"
                );
            }
        }

        if self.pieces.is_empty() {
            return self.leaf(self.anchor, self.anchor);
        }

        // Collapse iff there is exactly one piece and it is verbatim.
        // Nothing weaker is sound: a 2->1 replacement collapsing would
        // violate the length invariant, and a 1->1 fold (equal lengths,
        // different bytes) collapsing would license Verbatim-copying the
        // fold's source bytes for its (different) content bytes.
        if self.pieces.len() == 1 && self.pieces[0].verbatim {
            let p = &self.pieces[0];
            return self.leaf(p.src_range.start, p.src_range.end);
        }

        let concat_pieces: Vec<(SourceInfo, usize)> = self
            .pieces
            .iter()
            .map(|p| (self.leaf(p.src_range.start, p.src_range.end), p.content_len))
            .collect();
        SourceInfo::concat(concat_pieces)
    }

    /// Build a leaf `SourceInfo` (`Original` or `Substring`) over
    /// `[start, end)` in the root's coordinate space.
    fn leaf(&self, start: usize, end: usize) -> SourceInfo {
        match &self.root {
            Root::File(file_id) => SourceInfo::Original {
                file_id: *file_id,
                start_offset: start,
                end_offset: end,
            },
            Root::Parent(parent) => SourceInfo::Substring {
                parent: Arc::clone(parent),
                start_offset: start,
                end_offset: end,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::FileId;

    // -------------------------------------------------------------------
    // Frozen test: all-verbatim -> a contiguous SourceInfo, not a
    // 1-piece Concat.
    // -------------------------------------------------------------------
    #[test]
    fn test_all_verbatim_collapses_to_contiguous() {
        let mut b = ProvenanceBuilder::in_file(FileId(0), 100);
        b.verbatim(0..3);
        b.verbatim(3..7);
        b.verbatim(7..10);
        let si = b.finish();
        match si {
            SourceInfo::Original {
                file_id,
                start_offset,
                end_offset,
            } => {
                assert_eq!(file_id, FileId(0));
                assert_eq!(start_offset, 0);
                assert_eq!(end_offset, 10);
            }
            other => panic!("expected a contiguous Original, got {other:?}"),
        }
    }

    // -------------------------------------------------------------------
    // Frozen test: the fold shape (verbatim / 1->1 replacement /
    // verbatim) stays a 3-piece Concat and must not collapse.
    // -------------------------------------------------------------------
    #[test]
    fn test_fold_shape_stays_uncollapsed_concat() {
        // root plain, col-0 continuation: "aaa\nbbb" -> "aaa bbb"
        // verbatim 0..3, replacement 3..4 (1 source byte -> 1 content
        // byte, but NOT byte-identical: \n -> ' '), verbatim 4..7.
        let mut b = ProvenanceBuilder::in_file(FileId(0), 0);
        b.verbatim(0..3);
        b.replacement(3..4, 1);
        b.verbatim(4..7);
        let si = b.finish();
        match si {
            SourceInfo::Concat { pieces } => {
                assert_eq!(pieces.len(), 3, "fold shape must not collapse");
                assert_eq!(si_length(&pieces[0].source_info), 3);
                assert_eq!(pieces[0].length, 3);
                assert_eq!(si_length(&pieces[1].source_info), 1);
                assert_eq!(pieces[1].length, 1);
                assert_eq!(si_length(&pieces[2].source_info), 3);
                assert_eq!(pieces[2].length, 3);
            }
            other => panic!("expected a 3-piece Concat, got {other:?}"),
        }
    }

    // -------------------------------------------------------------------
    // Frozen test: zero pieces -> a zero-length SourceInfo at the
    // anchor.
    // -------------------------------------------------------------------
    #[test]
    fn test_zero_pieces_yields_zero_length_at_anchor() {
        let b = ProvenanceBuilder::in_file(FileId(3), 42);
        let si = b.finish();
        match si {
            SourceInfo::Original {
                file_id,
                start_offset,
                end_offset,
            } => {
                assert_eq!(file_id, FileId(3));
                assert_eq!(start_offset, 42);
                assert_eq!(end_offset, 42);
            }
            other => panic!("expected a zero-length Original at the anchor, got {other:?}"),
        }
    }

    // -------------------------------------------------------------------
    // Frozen test: an out_len == 0 piece (a deletion) is STORED, and
    // the source tiling stays gap-free.
    // -------------------------------------------------------------------
    #[test]
    fn test_deletion_piece_is_stored_and_tiling_is_gap_free() {
        // verbatim 4..7, deleted 7..11, verbatim 11..14 (the
        // escaped-break shape from the fixtures note).
        let mut b = ProvenanceBuilder::in_file(FileId(0), 0);
        b.verbatim(4..7);
        b.replacement(7..11, 0);
        b.verbatim(11..14);
        let si = b.finish();
        match si {
            SourceInfo::Concat { pieces } => {
                assert_eq!(pieces.len(), 3, "the deletion piece must not be dropped");
                assert_eq!(si_length(&pieces[1].source_info), 4);
                assert_eq!(pieces[1].length, 0, "deletion produces zero content bytes");
                // Tiling: pieces' source ranges must abut with no gap.
                assert_eq!(source_range(&pieces[0].source_info), 4..7);
                assert_eq!(source_range(&pieces[1].source_info), 7..11);
                assert_eq!(source_range(&pieces[2].source_info), 11..14);
            }
            other => panic!("expected a 3-piece Concat, got {other:?}"),
        }
    }

    // -------------------------------------------------------------------
    // Frozen test: in_parent over a real Concat parent yields
    // parent-relative pieces (never resolves absolute positions).
    // -------------------------------------------------------------------
    #[test]
    fn test_in_parent_over_concat_parent_yields_parent_relative_pieces() {
        let parent = SourceInfo::concat(vec![
            (SourceInfo::original(FileId(1), 0, 5), 5),
            (SourceInfo::original(FileId(1), 10, 15), 5),
        ]);
        assert!(
            parent.resolve_byte_range().is_none(),
            "sanity: Concat has no resolvable range"
        );

        // Case A: a single verbatim piece collapses to a Substring
        // directly over the Concat parent — no resolve_byte_range call
        // is needed or made.
        let mut single = ProvenanceBuilder::in_parent(parent.clone(), 0);
        single.verbatim(2..6);
        match single.finish() {
            SourceInfo::Substring {
                parent: p,
                start_offset,
                end_offset,
            } => {
                assert!(matches!(*p, SourceInfo::Concat { .. }));
                assert_eq!(start_offset, 2);
                assert_eq!(end_offset, 6);
            }
            other => panic!("expected a Substring over the Concat parent, got {other:?}"),
        }

        // Case B: verbatim + replacement (does not collapse) yields a
        // Concat whose pieces are each Substrings over the same parent.
        let mut multi = ProvenanceBuilder::in_parent(parent.clone(), 0);
        multi.verbatim(0..4);
        multi.replacement(4..6, 1);
        match multi.finish() {
            SourceInfo::Concat { pieces } => {
                assert_eq!(pieces.len(), 2);
                for piece in &pieces {
                    match &piece.source_info {
                        SourceInfo::Substring { parent: p, .. } => {
                            assert!(matches!(**p, SourceInfo::Concat { .. }));
                        }
                        other => panic!("expected a Substring piece, got {other:?}"),
                    }
                }
                match &pieces[0].source_info {
                    SourceInfo::Substring {
                        start_offset,
                        end_offset,
                        ..
                    } => {
                        assert_eq!(*start_offset, 0);
                        assert_eq!(*end_offset, 4);
                    }
                    _ => unreachable!(),
                }
                match &pieces[1].source_info {
                    SourceInfo::Substring {
                        start_offset,
                        end_offset,
                        ..
                    } => {
                        assert_eq!(*start_offset, 4);
                        assert_eq!(*end_offset, 6);
                    }
                    _ => unreachable!(),
                }
            }
            other => panic!("expected a 2-piece Concat, got {other:?}"),
        }
    }

    // -------------------------------------------------------------------
    // Additional cases named alongside the frozen tests: one
    // replacement, synthesis, adjacent replacements, a replacement at
    // offset 0, and a replacement at the end.
    // -------------------------------------------------------------------

    #[test]
    fn test_single_replacement_does_not_collapse() {
        // k: '''' -> value "'" : a single 2->1 replacement. Must stay a
        // 1-piece Concat, not collapse to Original{4,6} (which would
        // violate the length invariant: length() 2 != decoded.len() 1).
        let mut b = ProvenanceBuilder::in_file(FileId(0), 4);
        b.replacement(4..6, 1);
        let si = b.finish();
        match si {
            SourceInfo::Concat { pieces } => {
                assert_eq!(pieces.len(), 1);
                assert_eq!(pieces[0].length, 1);
                assert_eq!(si_length(&pieces[0].source_info), 2);
            }
            other => panic!("expected a 1-piece Concat, got {other:?}"),
        }
    }

    #[test]
    fn test_synthesis_empty_src_range_with_positive_out_len() {
        // block | no final newline: verbatim 7..10, then a synthesized
        // trailing newline with no source byte at all: replacement(10..10, 1).
        let mut b = ProvenanceBuilder::in_file(FileId(0), 7);
        b.verbatim(7..10);
        b.replacement(10..10, 1);
        let si = b.finish();
        match si {
            SourceInfo::Concat { pieces } => {
                assert_eq!(pieces.len(), 2);
                assert_eq!(pieces[1].length, 1);
                assert_eq!(source_range(&pieces[1].source_info), 10..10);
            }
            other => panic!("expected a 2-piece Concat, got {other:?}"),
        }
    }

    #[test]
    fn test_adjacent_replacements_do_not_coalesce() {
        let mut b = ProvenanceBuilder::in_file(FileId(0), 0);
        b.replacement(0..2, 1);
        b.replacement(2..4, 1);
        let si = b.finish();
        match si {
            SourceInfo::Concat { pieces } => {
                assert_eq!(
                    pieces.len(),
                    2,
                    "replacements never coalesce, even when adjacent"
                );
                assert_eq!(source_range(&pieces[0].source_info), 0..2);
                assert_eq!(source_range(&pieces[1].source_info), 2..4);
            }
            other => panic!("expected a 2-piece Concat, got {other:?}"),
        }
    }

    #[test]
    fn test_replacement_at_offset_zero() {
        let mut b = ProvenanceBuilder::in_file(FileId(0), 0);
        b.replacement(0..2, 1);
        b.verbatim(2..5);
        let si = b.finish();
        match si {
            SourceInfo::Concat { pieces } => {
                assert_eq!(pieces.len(), 2);
                assert_eq!(source_range(&pieces[0].source_info), 0..2);
            }
            other => panic!("expected a 2-piece Concat, got {other:?}"),
        }
    }

    #[test]
    fn test_replacement_at_the_end() {
        let mut b = ProvenanceBuilder::in_file(FileId(0), 0);
        b.verbatim(0..3);
        b.replacement(3..5, 1);
        let si = b.finish();
        match si {
            SourceInfo::Concat { pieces } => {
                assert_eq!(pieces.len(), 2);
                assert_eq!(source_range(&pieces[1].source_info), 3..5);
                assert_eq!(pieces[1].length, 1);
            }
            other => panic!("expected a 2-piece Concat, got {other:?}"),
        }
    }

    // -------------------------------------------------------------------
    // Test helpers
    // -------------------------------------------------------------------

    fn si_length(si: &SourceInfo) -> usize {
        si.length()
    }

    fn source_range(si: &SourceInfo) -> Range<usize> {
        match si {
            SourceInfo::Original {
                start_offset,
                end_offset,
                ..
            } => *start_offset..*end_offset,
            SourceInfo::Substring {
                start_offset,
                end_offset,
                ..
            } => *start_offset..*end_offset,
            other => panic!("source_range: unexpected variant {other:?}"),
        }
    }
}
