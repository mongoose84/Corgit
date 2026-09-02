# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Corgit — a Tauri 2 desktop app (Svelte 5 + Rust) that is a dashboard over *many* local git
repositories, not a general git client. Four verbs: fetch, pull, commit, push, plus staging
and branch switching. Windows-only for v1.

## Commands

```sh
npm run tauri:dev        # the real app (needs Rust + MSVC build tools)
npm run dev              # frontend only, http://localhost:1420 — no Rust, no git
```

Outside Tauri the frontend falls back to `localStorage` for settings and shows the welcome
screen instead of a repo list, which makes `npm run dev` useful for layout work only.

```sh
npm run check            # svelte-check + TypeScript
npm test                 # vitest, all frontend tests
npm run build            # check, then production bundle
cargo test               # from src-tauri/
cargo clippy --all-targets -- -D warnings    # from src-tauri/
```

Single test:

```sh
npx vitest run src/lib/gitErrors.test.ts     # one file
npx vitest run -t 'rejected push'            # by name, any file
cargo test publish_pushes_head               # substring match on the test name
```

The performance benchmarks are checked-in `#[ignore]`d tests, and must run `--release` —
debug numbers are meaningless. They need a real folder of repos:

```powershell
cd src-tauri
$env:CORGIT_BENCH_ROOT = 'C:\dev\code'
cargo test --release --lib -- --ignored --nocapture bench_status_sweep
```

`bash scripts/make-demo-root.sh` builds a throwaway root (ahead, behind, dirty, a feature
branch, one repo with no upstream) to develop and screenshot against.

## SPEC.md is normative

`docs/SPEC.md` is the design document, and the code **cites it by section number** —
`(§7)`, `(§8.7)`, `(§5.1)` appear throughout both the Rust and the Svelte. These are not
decorative. When changing behaviour that a `§` reference describes, either keep the code
matching the spec or update the spec in the same change; a stale citation is worse than
none. Read the cited section before altering the code around it.

## Architecture

**Rust owns application state; the frontend is a view.** `repos.svelte.ts`,
`graph.svelte.ts` and `settings.svelte.ts` are mirrors fed by Tauri events, not sources of
truth. This is what keeps multi-window safe later (§9.2): the per-repo write queues and the
global git semaphore are process-local, so Corgit is one process with many windows.

**Every mutating command has one shape.** `write_and_refresh` in `lib.rs` resolves the
repo, holds its write-queue lock for the operation, then re-reads and publishes that repo's
status *regardless of whether the operation succeeded* — a failed stage can still have
changed the index. New write commands go through it; they do not hand-roll the sequence.

**Concurrency (§7), in order of how easy it is to break:**
1. One write queue per repo, keyed by canonicalised path. Every mutation, including
   `git fetch` — it writes to `.git`, so it is not a read.
2. Reads run concurrently with each other but block on that repo's write lock. Never parse
   a repo mid-mutation.
3. A global semaphore of 8 in-flight git processes, static in `git.rs` rather than in app
   state, because the cap must hold across every window.
4. A bulk run (Fetch All, Pull All Behind) takes at most 4 of those 8, so the sweep and
   whatever the user clicks next are never stuck behind a run they cannot see the end of.
   Headroom, not a priority queue — the fetch sweep already picked 4 for the same reason.
5. Never touch `index.lock`. Surface the error — the user is probably in a terminal.

**Two git binaries, deliberately (§3).** Git for Windows ships a launcher at
`<install>\cmd\git.exe` that only execs the real binary; that hop costs ~75 ms, more than
`git status` spends working. Read-only commands go straight to the real binary via
`git::read`. Anything that fetches, pulls, pushes or commits must keep the `git` on PATH
(`git::write`, `git::write_network`) — inheriting credential helpers, hooks and LFS is the
whole reason Corgit shells out instead of linking libgit2. Putting a network operation on
the read path breaks authentication in a way that is not obvious locally.

**The status cache is a cache, never truth.** `cache.rs` exists for cache-first paint;
anything read from it can be stale by up to one sweep interval, and that is accepted (§5.1).
Code that *decides* something from cached status must be safe when the answer is wrong —
see `remote::publish`, which pushes `HEAD` rather than a cached branch name for exactly
this reason.

**Watchers, not sweeps (§6).** Every repo gets one recursive FS watcher covering its
working tree *and* `.git` — on Windows a subtree watch is one handle whatever its depth
(measured: 69 repos = 17 ms, 73 handles, 4.4 MB), so §6's original "never watch the working
tree" applies to inotify and not here. The status sweep is now a reconciliation pass: most
ticks cover only repos no watcher would take, and a full pass runs every fifth tick.

This is the one place where spending is worth it, because **the sweep is spawn-bound, not
git-bound**: a bare `git version` costs 85 ms on the bench machine against 2–10 ms of
actual work per repo, so §1's 300 ms budget is unreachable for as long as a tick spawns one
process per repo. Tuning the status flags, the concurrency cap, or `core.fsmonitor` all aim
at the 5–15 % that isn't spawn — measure before reaching for any of them.

Hot vs cold survives only as layout: pinning decides where a repo sits in the list, not how
fast it updates.

**The graph lays itself out.** `graphLayout.ts` assigns lanes in-house; never shell out to
`git log --graph`. Ref badges come from `for-each-ref`, not `%d` (§8.4).

## Conventions

**Comments explain why, not what.** This codebase is unusually heavily commented, and the
comments carry the reasoning — trade-offs considered, alternatives rejected, the measurement
behind a constant. Match that density and that voice. A comment restating the code is
noise here; a comment recording why the obvious approach was not taken is the point. The
"Deliberately absent" block at the end of `.github/workflows/ci.yml` is the house style.

**Shared predicates over repeated inline conditions.** `isDirty` and `needsPublish` live in
`repos.svelte.ts` and are imported by every consumer, so the row, the button and the menu
cannot disagree about the same state. Prefer adding to that set over re-deriving.

**Do not run `cargo fmt`.** The Rust is hand-formatted — comment blocks and line breaks
placed to be read — and rustfmt disagrees with it in ~79 places. CI deliberately does not
gate on it, and the reasoning is recorded in `ci.yml`. Reformatting the tree is a decision
to be made on its own merits, not a side effect of another change.

**Dates are fixed `dd-MM-yyyy HH:mm:ss`** via `dateFormat.ts`, never a locale format.

**Colours live in `src/app.css`** as tokens (§11). No literal colour values in components.
The accent is reserved for selection and primary buttons; status colours must stay distinct
from it or the repo list stops being scannable.

**Errors keep their raw stderr.** Backend git failures return the whole trimmed stderr, and
`gitErrors.ts` picks a plain-language headline out of it for display, with the raw text
still available in a details view (§13). Do not truncate stderr at the boundary.

## CI

Two required checks on `main`: **Frontend** (Linux — `npm ci`, check, test, `vite build`)
and **Backend** (Windows — clippy with `-D warnings`, `cargo test`). Windows because that
is what v1 ships and what the code is written against: `#[cfg(windows)]` process flags, the
Git-for-Windows launcher hop, `wt.exe` discovery.

The backend job is dominated by compiling ~506 crates: ~3 min cold, ~25 s warm. The
`Swatinem/rust-cache` step is the only thing that matters for its runtime, and it can only
restore from a cache saved on the default branch. `main` must have run the workflow at
least once or every PR builds cold.
