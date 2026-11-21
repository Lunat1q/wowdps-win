# Test scratch and writer lifetime

Include `scratch.rs` only under `cfg(test)` or a test-support feature. It
uses no Cargo dependency. `Dir` creates a directory exclusively and removes
it on drop. `root()` holds a process root until normal exit; `short_root()`
uses a short Unix parent for socket paths. Check the complete socket suffix,
not just the root length. Child tests must override TMPDIR, TMP, and TEMP.

An exit callback cannot synchronize with filesystem writers. A test that
starts daemon threads calls `process::isolate("exact_test_name")` before any
setup. The parent owns the scratch root and waits for the child test process
to exit, ending all its threads, before deleting it. The child borrows that
root and registers no exit cleanup for it. Its local sandbox destructors use
`remove_when_quiet` to defer deletion to the parent. Explicit input mutations
that are part of the test still run normally. The parent requires an entry acknowledgement, so a stale exact selector cannot
silently run zero tests. It propagates the child's result after cleanup; explicitly requested ignored tests remain
selected in the child.

The parent also kills and waits for its exact child if parent-side unwinding
interrupts collection. This boundary covers in-process threads, not detached
external processes: test scaffolding must own, stop, and wait for those too.
The launcher tests use an exact foreground Child guard and, for the Linux
setsid case, serialized subreaper ownership of descendants.

The failing-writer regression registers an exit handler before scratch
allocation. On failure it releases and joins a pending writer after an
ordinary scratch callback would have removed its root. A parent-owned root
stays present through that last write and is removed only after process exit.

A kill, abort, or hard timeout can bypass guards and exit callbacks. Existing
candidate directories and symlinks are skipped, never swept or adopted.
