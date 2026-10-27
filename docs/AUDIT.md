# Corgit audit — findings

A three-step audit — security, then architecture, then test coverage — run over three
sessions, 11-09-2026 to 15-09-2026, against `branch_search_functionality` @ `ee5b9a4`.
All three steps were surveyed in full, and every carried finding is closed (27-10-2026).

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
| S4 | Low    | Credential URLs could reach the on-disk log                    | Done         |
| S5 | Low    | `Path::join` let a frontend path escape the repo root          | Done         |
| S6 | Low    | Ref names and hashes could be parsed as git options            | Done         |
| A1 | High   | `lib.rs` was five subsystems in one 2,997-line module          | Done         |
| A3 | Medium | `hash_root` used `DefaultHasher`                               | Done         |
| A4 | Low    | The sweep fsynced on an async worker every tick                | Done         |
| A5 | Low    | Two per-repo maps grew unbounded                               | Done         |
| A6 | Medium | `GraphPane.svelte` was A1 on the frontend                      | Done         |
| G1 | Medium | The commit pane could not show a rename                        | Done         |
| T1 | High   | Not one test ever ran git                                      | Done         |
| T2 | Medium | `lib.rs`'s subsystems were untested                            | Done         |
| T3 | Medium | The store `apply*` methods were testable but untested          | Done         |

Tests at close of session 3: 202 Rust (plus 2 `#[ignore]`d benches), 220 frontend.
Baseline at the start was 159 and 209. After A3's tails (27-10-2026): 209 Rust; after the
mock-runtime harness (same day): 215; after `run_bulk` (same day): 218; after S4–S6
(same day): 234.

## Open

Nothing carried. What was deliberately left is under each step's *Not carried*, and one
cosmetic note under *Turned up along the way*.

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

S4–S6 were first recorded as low/info and deferred; all three were closed on 27-10-2026.
One threat model covers them, and it is what makes them low rather than nothing: the
frontend is the only thing between a typed or clicked string and argv or the filesystem,
so each is reachable only by something that can make the webview send what the UI never
would. They are defence in depth around that boundary — and S6 turned out to be the
deepest of them.

### S4. Low — credential URLs could reach the on-disk log (done)

`git.rs`'s two `log::warn!`s write argv and, on a non-zero exit, the whole stderr. Corgit's
own argv never carries a URL (fetch, pull and push name the remote), and git strips the
userinfo from URLs it prints itself — checked against 2.53. But the same stderr carries
whatever hooks, credential helpers and LFS print, which git does not rewrite, and the log
is a file that Help ▸ Open Log Folder invites the user to attach to a bug report.

**Fixed:** both lines are built by `killed_line` / `non_zero_line` and passed through
`redact_credentials`, which strips the whole userinfo of every `scheme://userinfo@host` —
username included, since a GitHub-style token is routinely the username. The log only:
what the user is shown keeps git's stderr whole (§13, CLAUDE.md). 5 tests; 4 mutations
(either line unredacted, redacting only `user:pass@`, redacting only the first URL), all
caught.

### S5. Low — `Path::join` let a frontend path escape the repo root (done)

The untracked-file diff, Open in VS Code and Reveal in File Explorer each joined a
frontend-supplied path onto the repo root. `join` with an absolute argument *replaces* the
base, and on Windows so do a root-relative path (keeps only the drive) and a
drive-relative one (`C:x`); `..` needs no special form. The diff is the one that matters —
it reads the file and returns its contents, so it was a read of any file on the machine.

**Fixed:** `repopath::join` admits only `Normal` and `.` components, refusing the rest
rather than cleaning them up — git never emits any of them, so such a path did not come
from a file row. Lexical, deliberately: an untracked symlink inside the repo is a real row
git reports, and what it may show is a separate question. 5 tests; 4 mutations (the diff
bypassing it, and admitting `..`, a root, or a prefix), all caught. The two `lib.rs` call
sites are not mutation-tested — under mutation they would launch VS Code or Explorer from
the test run — and rest on the helper's tests plus inspection.

### S6. Low — ref names and hashes could be parsed as git options (done)

Recorded as "branch names reach `switch` / `branch` / `merge` without `--`". Both halves
were reproduced against git 2.53 before fixing, and the second was not in the original
note at all:
- `switch --guess --detach` — a branch name of `--detach` — **detached HEAD**.
- `diff-tree` and `show` given `--output=<path>` in the hash slot **wrote that file**.
  Both run on the read path, from `graph::details` and the commit-file diff, so a hash
  the frontend sent could write anywhere — undoing what S2 established for that path.

**Fixed:** `git::END_OF_OPTIONS` (`--end-of-options`) before every positional ref or hash
the frontend supplies: `switch`, `branch` create/delete, `merge`, `diff-tree`, `show`.
Not `--`, which in `diff-tree`/`show`/`checkout` also starts pathspecs and so cannot sit
before a revision that is followed by a path. Option *values* (`-c <name>`) need nothing —
git takes them literally and rejects `--orphan` as a branch name on its own. This raises
the effective git floor from 2.23 (`switch`/`restore`) to 2.24; SPEC §3 now says so.

6 real-git tests through the public functions, each using a name git would *accept* as an
option, so the call does something different without the marker. 11 mutations; 9 caught.
The two survivors are `switch_remote_tracking`'s, and survive correctly: an option-shaped
`remote_ref` makes an option-shaped `-c` value, which git rejects first, so neither marker
is reachable. They stay for uniformity, with a comment saying exactly that. One test was
wrong on its first draft: it claimed `branch -d --list` lists and exits 0. It errors, as
every option there does, so delete's test now pins the *message* — unguarded, git says
"branch name required" about a command Corgit never meant to run.

The mutation harness had a hole of its own, found on the S4 sweep: it reported a mutation
that failed to *compile* as SURVIVED whenever the error had no `error[E…]` code. It now
requires a `test result` line, and the only two earlier SURVIVED verdicts were re-run to
confirm they compiled.

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
the pre-field test catches that. The call from `remember_root` is covered by
`mockapp.rs` (see T2).

### A4. Low — the sweep fsyncs on an async worker every tick (done)

`persist_cache` → `cache::save` → `atomicfile::write` (create, write, `sync_all`, rename)
ran synchronously inside the async sweep task. Every other instance of this pattern was
documented; this one, running every 60 s, was not.

**Fixed:** `persist_cache` is `async` and saves inside `spawn_blocking`. Awaited rather
than fired and forgotten — overlapping saves for one root were always normal (§9.5 rule
1), but awaiting keeps each caller's writes in the order it issued them. Cascaded
`record_fetch_attempt` and `publish_state` into `async`; both were already called from
async contexts. No test — it is about which thread blocks, not what the code decides.

### A5. Low — two per-repo maps grow unbounded (done)

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

### A6. Medium — `GraphPane.svelte` is A1 on the frontend (done)

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

**Siblings, 27-10-2026.** Same rule — ownership, not line count — and it gave one cut each:
- `CommitPane.svelte` 860 → 316, `<ChangesList>` 591. The compose box and the file lists
  met in exactly one place, `busy`, which crosses as a bindable the way BranchFind's
  `open` does. ChangesList owns the selection, ↑/↓ and its held-key coalesce, the menu,
  the ignore entries and the discard/delete dialog. The pane keeps the loading and error
  states, since they replace the lists rather than living in them. One behaviour change,
  deliberate: the menu and dialog now unmount with the list, so a repo whose files stop
  being readable no longer leaves a confirmation up over rows that are gone. One scoped-CSS
  trap: `.link` had been silently inheriting `cursor: pointer` and its disabled colour
  from the pane's base `button` rule, and now says both itself.
- `RepoList.svelte` 611 → 426, `<RootStrip>` 209. The strip is the one part of the pane
  scoped to the folder — it reads the bulk run and the behind count, never the filter,
  pins or rows. The bands stayed: they are the list's own chrome.

**Runtime-verified, 27-10-2026** — closing the gap BranchFind was left with. The dev app
was driven over CDP (`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=…`,
then `Runtime.evaluate` dispatching real DOM events) against `make-demo-root.sh`'s root:
- BranchFind: header button and Ctrl+F open it focused; hits filter and count ("2 of
  4"); Enter jumps, closes, and leaves the target selected inside the viewport; Escape
  closes; reopening keeps the query selected; a repo switch closes it and clears the
  query.
- ChangesList: click selects and opens the diff; ↓ crosses from Staged into Changes
  moving focus and diff; ctrl-click extends; the mixed-selection menu reads *Stage 2
  files / Discard 1 (skipping untracked) / Delete 1 / Ignore 1 (skipping tracked)*;
  Discard's dialog lists the one tracked row and Escape closes it; row `+`, *unstage
  all* and *stage all* land; `busy` crosses both ways (a row's stage disables the
  textarea, the header's Fetch disables the rows).
- RootStrip: *Pull all* walked `Pulling… 0 of 1 | Stop` → `1 of 1` → `Nothing to pull`,
  with the bar drawn and the button disabled after.

Opening the demo root also exercised A3's prune live — two legacy-keyed cache files went.
The app data was backed up first and restored afterwards.

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

### T2. Medium — `lib.rs`'s subsystems are untested (done)

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

**The rest is orchestration, not decisions** — there is no value to assert on, only the
fact that `emit_repo_status` sits outside the `if`, or that a semaphore is acquired. The
only honest test runs the function with a real handle, via `tauri::test`'s `MockRuntime`.
The first attempt (15-09-2026) made the write path generic and compiled first time, but
was backed out because the test binary would not *load* on Windows —
`STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139)`.

**Loader fixed 27-10-2026.** The note's diagnosis ("a WebView2 linking problem") was
wrong. The binary imports `TaskDialogIndirect` from `comctl32.dll`, which only exists in
Common Controls v6, and only an app manifest selects v6. tauri-build embeds that manifest
through `rustc-link-arg-bins` — binaries only, never test executables. `build.rs` now
turns tauri-build's manifest off and embeds the same file (`windows-app-manifest.xml`,
tauri-build's default verbatim) via `/MANIFEST:EMBED` on `rustc-link-arg`, which reaches
every target. Verified: the probe failed with 0xc0000139 before and passes after; the
app executable still carries the manifest. `mockapp::a_mock_app_can_start` stays as the
canary.

**Mock-runtime harness, 27-10-2026.** `src/mockapp.rs`: a `mock_app` with the real
`AppState` managed on it. `AppState::new` was pulled out of `setup` so the tests build
the state the app builds, not a copy that can drift. Generic over `R: Runtime`:
`write_and_refresh`, `repo_path`, `WriteMarker`, `persist_cache`, `sync_watchers`,
`remember_root`, `sweep::emit_repo_status`, `RepoWatchers::sync`, `watch::start` — no
call site changed, since `AppHandle` defaults to the real runtime. 5 tests, 7 mutations,
all caught, each by the test aimed at it:
- `write_and_refresh` publishes after a failed write — the op stages a file *then* fails,
  and both `RootState` and the `status:repo` event must show it staged. Catches publishing
  only on success, and publishing before the op.
- A failed write still sends `write:end` — the row would otherwise spin forever (§13).
- `sync_watchers` prunes the write queues and throttle entries of a dropped repo, on a
  root swap and on a close — closes A5's wiring hole.
- `remember_root` prunes the cache of a root pushed off the recent list — closes A3's.
One test was wrong on its first draft (it wrote a cache file before that root was on the
recent list, so an earlier prune rightly took it); `open_root`'s order is now the test's.

**`run_bulk`'s cap and Stop, 27-10-2026 — T2 closed.** Not reachable with real git: local
fetches finish too fast for four to ever overlap, so a cap test would pass with the cap
deleted. `run_bulk` keeps its signature and its `BulkOp` (whose comment argues for an enum
over a closure, and still holds for every command); the loop moved one level down into
`run_each`, which takes the git call as a closure and is private to `bulk.rs`. The tests
pass it a *gate*: an op that counts itself in, records the peak, and waits until the test
opens it. Made generic along the way: `stop_bulk`, the bulk progress and fetch-failure
helpers, `fetchsweep::{record_fetch_attempt, publish_state}`, and
`sweep::{trigger_sweep, start_sweep, sweep, finish_stragglers}`. 3 tests, 4 mutations, all
caught:
- At most `BULK_CONCURRENCY` in flight with ten queued, and the peak is *exactly* the cap —
  catches a raised cap and an accidentally serialised run alike.
- Stop lets the running four finish and skips the queued two (skipped, not failed).
- A Stop pressed before a run does not carry into it — the flag clears at run *start*.

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

### Bulk strip reads "0 of 0" for one round trip (not carried)

Seen while verifying A6. `runBulk` sets `{ total: 0 }` before invoking, on purpose — the
click must acknowledge itself before the backend's first progress event (§13) — so the
strip says *Pulling… 0 of 0* until that event lands. Cosmetic and one IPC hop long; a
label that drops the count while `total === 0` would fix it.

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
