// ---- THE COMPARE-AND-SET, one locked section (CONC1, 2026-09-14) ----
// Re-read the claims and the ledger, re-run the allocator's decision,
// and re-check that it still holds before anything is written.
// return to the records first, worktree second. It used to be the other way
// round, so a crash between the two left a worktree nothing pointed at.
export const LOCK = 1;
