//! `SourceInfo::map_offset` must not allocate proportionally to the file.
//!
//! Regression test for quarto-dev/q2's bd-jn7r22g8: through 0.1.3,
//! `map_offset` cloned the entire in-memory file content on every call so it
//! could pass a `&str` to `FileInformation::offset_to_location`. Callers that
//! map one offset per AST node (pampa's list loose/tight detection maps two
//! per list item) then paid O(nodes × file size) — 26 % of a 1.1 MB, 15k-item
//! document's render time.
//!
//! This lives in its own integration-test binary because it installs a
//! counting `#[global_allocator]`, which is per-binary.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use quarto_source_map::{SourceContext, SourceInfo};

struct CountingAllocator;

/// Total bytes requested from the allocator since process start.
static ALLOCATED: AtomicUsize = AtomicUsize::new(0);

// SAFETY: delegates every call to `System` unchanged; only adds a counter.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATED.fetch_add(layout.size(), Ordering::Relaxed);
        // SAFETY: same contract as the caller's.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from `System.alloc` with this `layout`.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

/// Build a ~1 MB, many-line in-memory file.
fn one_megabyte_file() -> String {
    let line = "- item with some words, `code`, and *emphasis* on it\n";
    line.repeat((1 << 20) / line.len() + 1)
}

#[test]
fn map_offset_on_in_memory_file_allocates_a_bounded_amount() {
    let content = one_megabyte_file();
    let total = content.len();
    let mut ctx = SourceContext::new();
    let file_id = ctx.add_file("big.qmd".to_string(), Some(content));
    let info = SourceInfo::original(file_id, 0, total);

    const CALLS: usize = 1000;
    let before = ALLOCATED.load(Ordering::Relaxed);
    for i in 0..CALLS {
        // Spread the offsets over the whole file so every call lands on a
        // different line; the result is checked so the loop can't be
        // optimized away.
        let offset = (i * 7919) % total;
        let mapped = info.map_offset(offset, &ctx).expect("offset is in bounds");
        assert_eq!(mapped.location.offset, offset);
    }
    let allocated = ALLOCATED.load(Ordering::Relaxed) - before;

    // The mapping itself allocates nothing; leave generous headroom for the
    // test harness. Cloning the file once per call would be ~1 GB here.
    const BUDGET: usize = 64 * 1024;
    assert!(
        allocated < BUDGET,
        "{CALLS} map_offset calls on a {total}-byte in-memory file allocated \
         {allocated} bytes (budget {BUDGET}); map_offset must borrow the \
         stored content, not clone it"
    );
}
