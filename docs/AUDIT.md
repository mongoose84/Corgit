# Corgit audit — findings

A three-step audit — security, then architecture, then test coverage — run over three
sessions, 11-09-2026 to 15-09-2026, against `branch_search_functionality` @ `ee5b9a4`.
All three steps were surveyed in full; what remains is implementation of carried findings.

`file:line` references below are **as of `ee5b9a4`** and have drifted since — A1 alone moved
a third of `lib.rs` into new modules. Use them to find the code, not to cite it.

This file exists because the findings list previously lived only in a session scratchpad,
and recovering it meant grepping old transcripts. Keep it current in the same change that
closes or opens an item.

## Standard

Every new test is **mutation-verified**: break the thing it guards and confirm that test —
ideally only that test — fails. This caught real gaps twice in T1 alone: a symmetric 1/1
ahead/behind fixture that could not see a sign swap, and a `--track` test that passed
because `branch.autoSetupMerge` sets the upstream anyway. Both were holes in the tests, not
in the mutations.

## Status

| Id | Sev    | Finding                                                        | State        |
|----|--------|----------------------------------------------------------------|--------------|
| S1 | High   | Command injection via `cmd /C` in `open_in_vscode`             | Done         |
| S2 | Medium | A swept repo's `.git/config` is executed code                  | Done         |
| S3 | Medium | `discard` globbed its pathspecs                                | Done         |
| A1 | High   | `lib.rs` was five subsystems in one 2,997-line module          | Done         |
| A3 | Medium | `hash_root` used `DefaultHasher`                               | Done         |
| A4 | Low    | The sweep fsynced on an async worker every tick                | Done         |
| A5 | Low    | Two per-repo maps grew unbounded                               | Done, wiring untested |
| A6 | Medium | `GraphPane.svelte` was A1 on the frontend                      | Done, not runtime-verified; siblings open |
| G1 | Medium | The commit pane could not show a rename                        | Done         |
| T1 | High   | Not one test ever ran git                                      | Done         |
| T2 | Medium | `lib.rs`'s subsystems are untested                             | **Partial** — unblocked |
| T3 | Medium | The store `apply*` methods were testable but untested          | Done         |

Tests at close of session 3: 202 Rust (plus 2 `#[ignore]`d benches), 220 frontend.
Baseline at the start was 159 and 209. After A3's tails (27-10-2026): 209 Rust.

## Open

### T2 — `lib.rs`'s subsystems are untested (partial)

Originally uncovered: `run_bulk` (the `BULK_CONCURRENCY` cap, Stop's "in-flight still
finish", the skipped/failed/succeeded tally); the sweep's merge-not-replace rule (a repo
skipped for a held write lock must not vanish); the generation `Outcome::Restart` path
including straggler abort; `write_and_refresh`'s publish-regardless-of-outcome rule. Each
needed an `AppHandle`, which is exactly why none had tests.

**Done, by naming the decision** — pull it out as a pure function and test that:
- `RootState::merge_sweep_results` — 3 tests. Wholesale replace fails 2 of 3; dropping the
  `errors.remove` fails the third.
- `bulk::tally` — 4 tests. The rule is `succeeded + skipped + failed.len() == total`,
  which the banner turns into a sentence and which is invisible in the loop that produced
  it. The `JoinError`-to-`Skipped` mapping stays in `run_bulk`; only the await can see a
  panic.
- `sweep::decide` — 4 tests. Two of its three branches only run when a root is swapped
  *while a sweep is in flight*, which no test can arrange through `sweep` itself, and the
  dangerous one is silent (one folder's rows painted with another's answers).

**Still open:** `write_and_refresh`'s publish-regardless rule, `run_bulk`'s concurrency cap
and Stop. These are orchestration, not decisions — there is no value to assert on, only
the fact that `emit_repo_status` sits outside the `if`, or that a semaphore is acquired.
The only honest test runs the function with a real handle.

**Where to start.** `tauri::test`'s `MockRuntime` needs the write path generic over
`R: Runtime`. That is **seven signatures and one `Drop` impl**, and it compiled first time.
The first attempt (15-09-2026) was backed out because the test binary would not *load* on
Windows — `STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139)`.

**Loader fixed 27-10-2026.** The note's diagnosis ("a WebView2 linking problem") was
wrong. The binary imports `TaskDialogIndirect` from `comctl32.dll`, which only exists in
Common Controls v6, and only an app manifest selects v6. tauri-build embeds that manifest
through `rustc-link-arg-bins` — binaries only, never test executables. `build.rs` now
turns tauri-build's manifest off and embeds the same file (`windows-app-manifest.xml`,
tauri-build's default verbatim) via `/MANIFEST:EMBED` on `rustc-link-arg`, which reaches
every target. Verified: the probe failed with 0xc0000139 before and passes after; the
app executable still carries the manifest. `mock_runtime::a_mock_app_can_start` stays as
the canary. **Next:** genericise the write path over `R: Runtime` and write the tests.

### A5 — the prune calls are untested

`retain_reads` and `retain_known` are themselves tested, but nothing covers that `sync` /
`sync_watchers` still *call* them — deleting either call leaves every test green. Same
`AppHandle` blocker as T2. Recorded in the test's own comment. A3's `cache::prune` call
from `remember_root` is in the same position.

### A6 — `RepoList.svelte` and `CommitPane.svelte`

At 611 and 860 lines, both are on the trajectory GraphPane was on. Untouched. Also: A6's
`<BranchFind>` extraction is the one closed item **not verified at runtime** — there are
no component tests and the app was not driven. It rests on `svelte-check` (0 errors / 0
warnings over 395 files), the test suite, `vite build`, and inspection.

## Step 1 — Security

### S1. High — command injection via `cmd /C` in `open_in_vscode` (done)

`src-tauri/src/lib.rs:717-733`. Rust quotes a Windows arg only if it contains a space or
tab; cmd.exe re-parses `& | ^ > <`, all legal in Windows filenames. A repo directory or
committed file named `build&calc.exe` (no space, so unquoted) made "Open in VS Code" run
`calc.exe`. CVE-2024-24576 class — the Rust 1.77.2 fix covers `.bat`/`.cmd` *programs*,
not arguments passed to cmd.exe.

**Fixed:** `program_on_path` + `path_extensions` in `lib.rs` resolve `code.cmd` via a PATH ×
PATHEXT walk and spawn it directly; the `cmd /C` hop is gone. 3 tests. Verified
empirically — the old form ran the injected `ver`, the new form passes the argument
through intact.

### S2. Medium — a swept repo's `.git/config` is executed code (done)

`status.rs:128`, `graph.rs:51`, `discovery.rs:27`. Discovery and the sweep run git in
every child of the root with no user click. `core.fsmonitor` (fires on `status`) and
`[diff "x"].textconv` (fires on diff) name programs git runs. `diff.rs:102` already
blocked `diff.external`; the same reasoning had not reached the sweep.

**Fixed:** `git::READ_CONFIG` prefixes `-c core.fsmonitor=false` — read path only; the write
path keeps the user's full git environment per §3. `--no-textconv` added to `diff.rs`'s
common args. Both verified against real git: the fsmonitor hook ran *twice* per
`git status` on the sweep's own command; textconv is shell-executed *and* its stdout was
parsed as file content. Perf measured per CLAUDE.md's rule: 3,000-file repo with the
builtin daemon, 319 ms on vs 320 ms off (best of 8) — no difference, as spawn-bound
predicts.

### S3. Medium — `discard` globbed its pathspecs; `delete` didn't (done)

`commit.rs:34-35` vs `:70`. `literal_pathspec` was applied to `git clean` but not
`git restore --worktree`. `--` ends option parsing only; pathspec globbing still applies,
so `report[1].txt` is a character class. Discard destroys uncommitted work with no undo.

**Fixed:** `literal_pathspec` applied inside `run_pathspec`, covering stage, unstage and
discard. Reproduced against real git first — discarding `report[1].txt` also reverted
`report1.txt`. Mutation-verified.

### Deferred (low / info — recorded, not carried)

- **S4** credential URLs in the on-disk log (`git.rs` non-zero-exit warn, `lib.rs:2611`).
- **S5** `Path::join` with an absolute input escapes the repo root (`lib.rs:729`, `:799`,
  `diff.rs:145`).
- **S6** branch names reach `switch` / `branch` / `merge` without `--`.

## Step 2 — Architecture

### A1. High — `lib.rs` is five subsystems in one 2,997-line module (done)

Sibling modules were 91–500 lines and single-purpose (`inflight.rs` even argues why it is
*not* part of `writequeue.rs`). `lib.rs` held commands, bulk runs, the status sweep,
focus/tickers and the fetch sweep. `AppState` had 14 fields with no scoping, and 28
hand-rolled `root.lock()` sites; the take/clone/drop-before-await invariant was observed by
convention, not by a boundary.

**Fixed:** cut `bulk.rs` (433), `sweep.rs` (562), `fetchsweep.rs` (261); `lib.rs` 2,997 →
2,038. `publish_fetch_state` moved bulk → fetchsweep so `FetchSweepEvent` has one builder.
Tickers and focus stayed in `lib.rs` as wiring, since they start both sweeps.

### A3. Medium — `hash_root` uses `DefaultHasher` (done)

`cache.rs:47-53`. The std docs say the algorithm "is not specified, and so it and its
hashes should not be relied upon over releases" (it has changed before: SipHash-2-4 →
1-3). It keyed two files with opposite durability needs: `cache/<hash>.json` (loss costs
one sweep) and `roots/<hash>.json` (pins and last-selected — "truth, not a cache", §9.5
rule 5). A toolchain bump would silently re-key the second and wipe every user's pins.

**Fixed:** FNV-1a in `cache.rs`, pinned by a known-answer test so CI holds the value rather
than the compiler. Plus the half the original note missed: swapping the algorithm
*re-keys every existing file*, inflicting the exact pin loss A3 warns about, once, on
upgrade. So `legacy_hash_root` stays, and `roots::load` adopts a pre-FNV file on the miss
path (copy → atomic write → best-effort remove; a current file always wins over a stale
legacy one). The cache is deliberately *not* migrated — a re-key there costs one cold
sweep, which `CACHE_VERSION`'s own comment already argues is acceptable. 6 tests; 3
mutations all caught.

**Tails, closed 27-10-2026** (§9.5 rule 6). The two halves got opposite answers, for the
same reason the files are separate:
- `cache/` is pruned. `cache::prune` runs from `remember_root` — the one moment a root can
  fall off the 10-entry recent list — and deletes every `*.json` not keyed by a recent
  root. Legacy-keyed cache files go too, even for a recent root: the cache was never
  migrated, so nothing reads them. Temps are left to `prune_stale_temps`.
- `roots/` is **not** pruned — a product call: a folder off the recent list may still be
  reopened through *Open Folder…*, its pins have no other copy, and the files are a few
  hundred bytes. Instead each file records the `root` it was hashed from, stamped by
  `save` from its argument rather than trusted from the caller. No version bump, so
  existing files load unchanged and gain the field on their next save.
7 tests; all 7 mutations caught, each by the test written for it. One mutation
(dropping `#[serde(default)]`) passed on the first sweep — correctly, since serde reads
a missing `Option` as `None` anyway; the real hazard there is a `ROOTS_VERSION` bump, and
the pre-field test catches that. The call from `remember_root` is untested — the same
`AppHandle` blocker as T2.

### A4. Low — the sweep fsyncs on an async worker every tick (done)

`persist_cache` → `cache::save` → `atomicfile::write` (create, write, `sync_all`, rename)
ran synchronously inside the async sweep task. Every other instance of this pattern was
documented; this one, running every 60 s, was not.

**Fixed:** `persist_cache` is `async` and saves inside `spawn_blocking`. Awaited rather
than fired and forgotten — overlapping saves for one root were always normal (§9.5 rule
1), but awaiting keeps each caller's writes in the order it issued them. Cascaded
`record_fetch_attempt` and `publish_state` into `async`; both were already called from
async contexts. No test — it is about which thread blocks, not what the code decides.

### A5. Low — two per-repo maps grow unbounded (done; wiring under Open)

`inflight.rs` removes at zero and explains why. Its siblings didn't: `WriteQueues.locks`
inserted for every id asked about (including the sweep's per-tick `try_read`) and never
removed; `RepoWatchers.reads` correctly survived `clear()` (alt-tab must not reset the
throttle) but also survived a root swap.

**Fixed:** `WriteQueues::retain_known` + `RepoWatchers::retain_reads`, called from
`sync_watchers`. 5 tests, 4 of 5 mutations caught. The write-queue prune needed a
condition the original note missed: dropping an entry someone still holds would let the
next `get` mint a *second* `RwLock` for the same repo, and two locks are no lock — §7 rule
1 would be silently gone. So it keeps any entry whose `Arc::strong_count > 1` (every
outstanding guard is an `Owned*` one holding its own clone, waiters included), under the
same mutex as `get`. There is a test for exactly that.

### A6. Medium — `GraphPane.svelte` is A1 on the frontend (done; siblings under Open)

1,388 lines, six independent concerns. The branch-search feature was the tell: its pure
logic went to `branchSearch.ts` with tests, but ~160 lines of UI state went into GraphPane
because there was no component to hold them.

**Fixed:** `<BranchFind>` extracted; GraphPane 1,388 → 972, BranchFind 512. Split by
ownership, not line count. BranchFind owns query, cursor, outOfReach, jumping, the results
derivation, key handling, markup and styles; it reads `graph` directly so `loadedTips` can
skip building a Set over 3,000 rows for a closed box. The pane keeps `findOpen` (its
header button paints from it), `revealRef` (pages and selection are the graph's) and
`scrollToRow` (the pane's scroll box). `reveal` and `centre` are two props rather than one
because the close has to happen *between* them — closing returns 186 px to the scroll
container, and centring against the shorter viewport is wrong every time a jump had to
read pages.

### Not carried

- **A2** — multi-window readiness is overclaimed in CLAUDE.md: the state and event layers
  are single-window by construction; only the concurrency layer is ready. A doc-accuracy
  issue; deliberately not carried.

## Step 3 — Test coverage

Baseline: 368 tests green in ~2 s (159 Rust + 2 `#[ignore]`d benches; 209 vitest). Pure
modules close to fully covered. Uncovered: `lib.rs` (10 tests over 2,997 lines, all pure
helpers), `menu.rs` (0), all 10 `.svelte.ts` stores, all 24 components.

### T1. High — not one test ever runs git (done)

Every parser was tested against hand-written fixtures, which encode the same belief as the
parser: they catch an edit, never a misreading of git.

**Fixed:** `TempRepo` plus 16 real-git tests over two sessions, all mutation-verified.
- `status::parse` — every record type at once (four deliberately different counts), the
  rename's second NUL field (original named `1-old.txt` so an unconsumed field would be
  tallied), `+N -M` from a genuinely diverged upstream — ahead 2 / behind 1, *lopsided on
  purpose*, because 1/1 let the sign-swap mutation through — the `(initial)` and
  `(detached)` literals, and `u` records from a real merge conflict.
- `graph::parse_raw_and_numstat` — the positional zip on a real merge, with the two
  incoming files given different line counts so a swapped pair shows in the numbers, not
  only the paths. Dropping `-m` is caught. This turned up G1.
- `branch::switch_remote_tracking` — the `"already exists"` match driven through a real
  collision, plus the create path. `--track` is *not* what sets the upstream
  (`branch.autoSetupMerge` does it anyway); only `--no-track` changes the outcome — the
  same trap `create_args`' comment already documents.
- `commit::DISCARD_FLAGS` — see S3.

`TempRepo` gained `try_git`, `git_stdout`, `remote_url`,
`allow_pushes_to_checked_out_branch`, and a process id in its temp directory name — the
counter restarts per process, so consecutive runs reused names and raced `Drop`'s
best-effort `remove_dir_all`. One unexplained single failure was seen right after the
mutation sweep and did not reproduce in 8 full runs; name reuse is the most plausible
cause and is now closed off, but it was never positively identified.

### T2. Medium — `lib.rs`'s subsystems are untested (partial — see Open)

### T3. Medium — the store `apply*` methods are testable today (done)

`applyRoot` / `applySweep` / `applyFetchSweep` / `applyRepoStatus`: synchronous, take a
plain object, touch only `$state`. `vite.config.ts` excluded stores as "whose whole job is
talking to Tauri" — true of `open()` / `write()` / `fetchAll()`, not of these.

**Fixed:** `src/lib/repos.svelte.test.ts`, 11 tests, all 7 mutations caught. `RepoStore` is
exported (with a comment that the singleton is still the only instance the app may build)
and the four `apply*` methods are public, documented as the seam worth testing. Covered:
`applyRoot`'s `isNewRoot` gate in both directions, the selection/file-pane drop on a root
change, the stale-root guard on all three event appliers, `applyRepoStatus`'s
statuses-XOR-errors in both directions, and the carried file list being dropped when the
selection moved while the event was in flight.

The original note's "enabler" was wrong: it claimed `select()`'s unguarded
`invoke('set_selected_repo')` blocked the test. Outside Tauri `invoke` returns a rejected
promise, and `select()` already swallowed it. The real wart was in `loadFiles`, where the
rejection set `filesError` to "ReferenceError: window is not defined" — the store
inventing git data it cannot have, which `tauri.ts`'s own doc comment forbids. Both are
guarded now, for consistency rather than because a test needed it.

### Not carried

- **T4** — 10 argv assertions are change detectors, not behavioural coverage. Legitimate as
  locks, but for some paths the only coverage there is; pair each with a real-git test.
- **T5** — `menu.rs` has 0 tests; neither CI job measures coverage.

## Turned up along the way

### G1. Medium — the commit pane cannot show a rename (done)

Found while closing T1. `parse_raw_and_numstat` had `R`/`C` arms in both walks, and
neither could run: `diff_tree_args` passed no `-M`, and `diff-tree` is plumbing, so it
ignores `diff.renames` (checked against git 2.53). The details pane (§8.5) showed every
rename as an add plus a delete, and `CommitFileEntry.status`'s documented `R`/`C` letters
never reached the frontend.

**Fixed:** `-M` added to `diff_tree_args`, pinned in the flags test. The rename test now
exercises both walks' `R`/`C` arms with the rename placed *between* two other files — the
position is the test: a rename sorted last has nothing after it to be shifted, which is
exactly how the first version passed while asserting nothing. Dropping `-M` fails it.
