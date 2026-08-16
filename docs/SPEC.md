# Corgit — v1 Spec

A fast, mouse-first dashboard over many local git repositories. Built because VS Code is
slow when you keep 77 repos and want to know, at a glance, which ones need attention.

**Not** a general git client. It does four verbs — fetch, pull, commit, push — plus staging
and branch switching, over a lot of repos at once.

---

## 1. Product

### Problem

77 local repos, mostly microservices. Roughly 5 are active at any time, and a single fix
often touches several of them. Existing tools are either single-repo-focused or slow to
start.

### Shape

A dashboard first, a git client second. The left pane — "which of my repos need me?" — is
the product. The graph is a viewer.

### Performance budget (this is the reason the project exists)

| Metric | Target |
| --- | --- |
| Cold start → repo list painted | < 500 ms |
| RAM, all 77 repos loaded | < 150 MB |
| Status sweep, 77 repos | < 300 ms wall clock |
| Background CPU, window unfocused | ~0 |

First paint renders from cache. It never waits on git.

---

## 2. Scope

### In (v1)

- Repo discovery by scanning configured root folders
- Per-repo status: branch, dirty indicator, ahead/behind
- Stage / unstage files (file-level)
- Commit (staged files only)
- Push, including "Publish branch" for a branch with no upstream
- Pull (merge only) and fetch
- Merge (via pull); conflict *detection* only
- Branch switching (local and remote-tracking)
- Commit graph for the selected repo, with commit details on click
- Pinning repos

### Out (v1)

Hunk staging · rebase · conflict resolution · stash · amend · force-push · commit signing ·
tags · cherry-pick · revert · reset · submodules · LFS-specific UI · multi-repo bulk actions

### v2 candidates

1. **Commit one message across N selected repos** — the highest-value feature, deliberately
   deferred. See §9: selection is modelled as a set from day one so this stays cheap.
2. Bulk push / bulk pull across the pinned set
3. Linux support (§10)
4. Stash, if branch-switch friction proves annoying in practice

### Non-goals, permanently

Replacing VS Code for conflict resolution or diff editing. When Corgit hits something it
doesn't handle, the escape hatch is a single **Open in VS Code** button.

---

## 3. Stack

| Layer | Choice | Why |
| --- | --- | --- |
| Shell | **Tauri 2** | ~10 MB binary, fast cold start, Rust backend |
| Backend | **Rust** | Process fan-out, FS watching (`notify`), concurrency primitives |
| Frontend | **Svelte 5** (runes) | Minimal runtime, fine-grained reactivity for 77 live rows |
| Git access | **Shell out to system `git`** | Inherits credential helpers, hooks, LFS, aliases, config |

### Why not libgit2 / gitoxide

Credential helpers (Git Credential Manager on Windows), hooks, LFS, sparse-checkout and
per-user config all come free when shelling out. Reimplementing GCM integration is a
project in itself. Optimize *spawn count*, not parse speed. Revisit only if profiling
proves spawning is the bottleneck.

### Git binary

Resolve `git` from PATH at startup. If absent, show a blocking first-run screen with a
download link rather than failing per-operation. Record the version — `core.fsmonitor`
support (§6) needs ≥ 2.37.

---

## 4. Layout

```
┌──────────────────────────────────────────────────────────────────────┐
│ File   View   Repository   Help                                      │
├─────────────────┬──────────────────┬─────────────────────────────────┤
│ REPO LIST       │ COMMIT / DETAILS │ GRAPH                           │
│ ~25%            │ ~20%             │ rest                            │
│                 │                  │                                 │
│ [Pinned]        │ ┌──────────────┐ │  ● Uncommitted Changes          │
│  ● api-gateway  │ │ message…     │ │  │                              │
│  ● auth-svc     │ └──────────────┘ │  ●─┐ feat: add retry            │
│  ● billing      │ [Commit] [Push]  │  │ │                            │
│                 │                  │  ● │ fix: null check            │
│ [🔍 filter…]    │ Staged (2)       │  │ ●  chore: bump deps          │
│                 │  M src/main.rs   │  ●─┘                            │
│ [All]           │  A src/lib.rs    │  ●   initial commit             │
│  ● admin-ui     │                  │                                 │
│  ● audit-log    │ Changes (14)     │                                 │
│  ● …            │  M README.md     │                                 │
└─────────────────┴──────────────────┴─────────────────────────────────┘
```

All three panes are **resizable by dragging**, with widths persisted to settings. The
percentages above are defaults, not constraints. Minimum widths prevent collapse to
unusable.

### 4.1 Menu bar

A **native Windows menu** via Tauri's `tauri::menu` API — not an HTML reimplementation.
VS Code hand-rolls its menu because it needs identical behaviour on three platforms and
command-palette integration; you need neither. Native gets OS-consistent behaviour,
keyboard accessibility and accelerators for free, and costs no CSS.

| Menu | Items |
| --- | --- |
| **File** | Open Folder… `Ctrl+O` · Open Recent ▸ · New Window `Ctrl+Shift+N` · Close Window `Ctrl+W` · Exit |
| **View** | Toggle Repo List · Toggle Commit Pane · Reset Pane Sizes · Reload |
| **Repository** | Fetch · Pull · Push — acting on the selected repo, mirroring the buttons for discoverability. Disabled when no repo is selected. |
| **Help** | About · Check for Updates · Open Log Folder |

Menu events arrive on the Rust side and are dispatched to the window that raised them.

---

## 5. Panes

### 5.1 Repo list (left)

Two sections, each alphabetical:

- **Pinned** — the hot set. This is also the FS-watch budget (§6). Every row carries a pin
  toggle in a reserved leading gutter, hover-revealed on unpinned rows and always drawn on
  pinned ones — the set only earns its keep if putting a repo in it costs one click, and a
  right-click-only affordance is not discoverable. Clicking the pin must not also select
  the repo. The *Pinned* header carries a hover-revealed **Unpin all**, hidden while the
  filter box is non-empty so it can never unpin repos the user cannot currently see.
  Right-click → Pin/Unpin stays as a second route.
- **All** — everything else.

A **filter box** sits between them. Typing filters both sections by substring on **repo name
only** — not branch, not path. This is the primary navigation tool for 77 repos; it is not
optional.

**Row contents:** repo name · current branch · dirty dot · ahead/behind badge.

The **dirty dot** is a single dot — one state, no distinction between staged and unstaged.
The row answers "does this need me?", nothing finer. Detail lives in the middle pane.

**Row-level Pull.** A repo that is behind shows a pull affordance on hover — the dashboard's
whole thesis is acting without navigating, and forcing a select-then-cross-the-window trip
for a one-click operation undercuts it. Constraints that follow:

- Shown **only on rows that are behind**, hover-revealed. Not on every row — 77 rows with
  permanent buttons is noise, and mis-clicking Pull on the wrong repo is a real cost.
- It is a **write on a possibly-cold repo**, so it goes through that repo's write queue
  (§7) like any other write.
- Failures have nowhere to render, because the repo may not be selected. Therefore the
  row must be able to carry an **error badge** (click → detail popover with raw stderr and
  *Open in VS Code*), and the merge-conflict state (§13) must be renderable **on a row**,
  not only in the middle pane.

No row-level fetch — fetch is automatic (§6). No row-level commit or push in v1; those need
a message or a diff review, which means the middle pane.

Rows are compact (single line). The list **never reorders itself** — no dirty-float, no MRU
shuffle. Position stability matters more than sorting cleverness for a mouse-first UI.

**Interactions:**
- Click row → select repo (drives middle pane and graph)
- Click branch name → dropdown of local + remote branches → switch (§8.3)
- Right-click → Pin/Unpin, Fetch, Open in VS Code, Open in Terminal, Copy path

**Staleness:** cold repos are refreshed by the sweep, so a badge can lag reality by up to
one sweep interval. Accepted.

### 5.2 Middle pane — two modes

The pane is **modal**, driven by graph selection.

**Mode A — Working tree** (default; active when the graph's *Uncommitted Changes* node is
selected):

```
[ commit message textarea ]
[ Commit ]  [ Push / Publish branch ]   [ Fetch ] [ Pull ]

Staged Changes (2)          [− unstage all]
  M  src/main.rs
Changes (14)                [+ stage all]
  M  README.md
  ?  notes.txt
```

- **Commit commits staged files only.** Disabled when nothing is staged or the message is
  empty.
- File rows: status letter, path (ellipsized head-first so the filename stays visible),
  hover reveals `+`/`−` stage/unstage buttons. Click a file → diff opens in VS Code (v1 has
  no internal diff viewer).
- **File list is capped at 100 entries per section.** The header must then read
  `Changes (100 of 3,412)`. "Stage all" still stages everything and its tooltip says so
  explicitly — the user must never commit files the UI silently hid.

**Mode B — Commit details** (active when a commit is selected in the graph):

```
← Back to changes
a3f9c21  feat: add retry logic
Jeppe Kronborg · 2026-08-12 14:03
[ full commit message ]

Files (7)
  M  src/main.rs
  A  src/retry.rs
```

Read-only. Clicking a file opens that commit's diff in VS Code.

### 5.3 Graph (right)

Selected repo only — one repo at a time, so graph cost never multiplies by 77.

- **Synthetic "Uncommitted Changes" node** pinned at the top when the working tree is dirty.
  Clicking it returns the middle pane to Mode A. This is what ties the two panes into one
  coherent surface.
- Rows: graph lanes · hash (short) · message · author · date · ref badges (branches, tags,
  `origin/*`).
- **The HEAD commit's row is marked.** Its dot is drawn larger with a halo, and the row
  carries a low-alpha tint of its own lane colour. Both are keyed off `branch.oid` from
  §8.2 rather than off the current branch's ref badge, so a detached HEAD — the state where
  "which commit am I on" is hardest to answer — still marks the right row. The tint is not
  the accent (§11 rule 3) and is declared so that hover and selection both override it:
  being on HEAD is a standing fact about the row, not a transient state.
- **Date format is `dd-MM-yyyy HH:mm:ss`**, always absolute, never relative, rendered in
  **local time** from `%ct` (a Unix timestamp). Use a fixed format string — never a
  locale-dependent formatter, or the column shifts with machine settings. The column is
  fixed-width and right-aligned; at 19 characters it costs ~140 px, so it is laid out
  before the message column gets its remaining space.
- Loads **300 commits at a time** with a "Load more" row at the bottom.
- Click a commit → middle pane Mode B.
- Right-click a commit → Copy hash, Copy message, Open in VS Code. (Thin by design — the
  graph is a viewer in v1.)

**Rendering: SVG lanes + virtualized DOM rows.** Not canvas. A few hundred SVG paths for
the lanes, HTML rows for text — this gives text selection, hover and context menus for
free, and avoids hand-rolled hit-testing. Canvas only pays off past ~10k simultaneous rows,
which we never render.

**Lane layout** is implemented in-house. Do not parse `git log --graph` ASCII output.

---

## 6. Data & refresh model

### Hot vs cold

**Hot** = pinned repos ∪ currently selected repo.
**Cold** = everything else.

Hot repos get FS watchers for instant feedback. Cold repos rely on the sweep. Because the
status sweep is cheap enough to cover all 77, hot/cold is a **latency and UI** distinction,
not a correctness one.

### Watchers (hot only)

Watch `.git/HEAD`, `.git/refs/**`, `.git/index` — small, bounded, and catches commits,
branch switches and staging done in a terminal. **Never watch the working tree**; watching
77 trees means watching 77 `node_modules` and blowing past inotify limits. Working-tree
changes are caught by the sweep and by window focus.

Debounce watcher events ~200 ms — git writes several files per operation.

### The two sweeps

These are different mechanisms and must not be conflated. Fetch does **not** detect dirty
repos.

| | Status sweep | Fetch sweep |
| --- | --- | --- |
| Network | No | Yes |
| Cost | ~5–20 ms/repo | ~0.5–2 s/repo |
| Detects | dirty, staged, conflicted, branch, **ahead** | **behind** |
| Interval | 60 s | 5–10 min, jittered |
| Concurrency | 8 | 4 |
| Runs when unfocused | No | No |

**Re-entrancy guard:** a sweep never starts while one is in flight — the tick is skipped,
not queued. At 77 repos the status sweep should finish in ~150 ms and never collide, so
this is cheap insurance rather than an expected path. Individual repos are also skipped
while their write lock is held (§7).

**Focus gating:** sweeps run only for the root of the **focused window**. Unfocused windows
and their roots go idle; on focus, that window's root gets an immediate status sweep. With
no tray and close-means-quit (§9.1), the app has no background lifetime at all — background
CPU is zero, not merely low.

Fetch additionally: skips repos with no remote, skips repos fetched within the last
interval, and records `last_fetch_at` per repo.

### Making `git status` fast on Windows

`git status` cost is dominated by untracked-file scanning. Git ≥ 2.37 ships a built-in
FSMonitor daemon that makes this near-instant:

```
git config core.fsmonitor true
git config core.untrackedCache true
```

Corgit should **offer** this per repo (a one-click banner), never apply it silently — it
modifies the user's config.

If the status sweep still measures slow, add a Defender exclusion for the repo roots and
`git.exe` before touching the architecture.

---

## 7. Concurrency rules

These guarantees are **process-local**, which is exactly why Corgit is one process with
many windows (§9.2). Queues and semaphores are keyed by canonicalised repo path and shared
across every window.

1. **One write queue per repo.** Every mutating operation — fetch, pull, commit, push,
   stage, unstage, checkout, merge --abort — goes through it, one at a time. `git fetch`
   writes to `.git`; it belongs here, not on the read path.
2. **Reads run concurrently with each other**, but block while that repo's write lock is
   held. Never parse a repo mid-mutation.
3. **Global semaphore of 8 in-flight git processes.** Without it the sweep spawns 77
   `git.exe` at once and Defender melts the machine.
4. Never touch `index.lock` directly. If git reports a lock error, surface it — the user is
   probably in a terminal in the same repo.

---

## 8. Git command reference

Every command runs with `--no-optional-locks` where it is read-only, and with a working
directory set to the repo root.

### 8.1 Discovery

**Depth 1 — direct children of the root only.** Read the root's entries; any directory
containing `.git` (file *or* directory) is a repo. No recursion, no walk, no skip-list, no
`node_modules` traversal. Discovery over 77 repos is one directory read plus 77 `exists()`
calls — sub-millisecond.

Repos nested deeper are out of scope by design: open that folder as its own root instead
(§9.1).

### 8.2 Status — one command gives everything

```
git --no-optional-locks status --porcelain=v2 --branch -z
```

Yields `# branch.head`, `# branch.upstream`, `# branch.ab +N -M`, plus `1`/`2` (changed /
renamed), `u` (unmerged → conflict state), `?` (untracked) records. Parse NUL-delimited.
This single call populates the repo row *and* the middle pane.

### 8.3 Branches

```
git for-each-ref --format='%(refname:short)%1f%(upstream:short)%1f%(objectname)' refs/heads refs/remotes
git switch <branch>                              # local
git switch -c <branch> --track origin/<branch>   # remote-tracking
git branch <new> <start-point>                   # create, stay put
git switch -c <new> <start-point>                # create and check out
```

The switcher lists **local branches, plus remote branches with no local counterpart**,
under `Local` / `Remote` headings. Listing every `origin/*` would show most branches twice,
and checking out `origin/foo` when local `foo` exists is never what you meant.

Repos with many remote branches still need a **type-to-filter box inside the dropdown** —
required, not polish. Stale remote branches are handled by `--prune` on fetch (§8.7).

(This is the *switcher* only. The graph shows every ref, local and remote — see §8.4's
`--all`.)

On checkout failure with a dirty tree: show git's actual stderr plus **Open in VS Code**.
**Never offer force-checkout** — it silently discards work.

**Creating a branch** (Git Graph's gesture): right-click a ref badge in the graph → *Create
branch from `<ref>`…* → a small modal takes the name plus a **Check out after creating**
checkbox, which picks between the two commands above. The start point is always the badge
that was right-clicked, never HEAD. The name is checked against `check-ref-format`'s rules
and against the local branches already in the graph as it is typed, so the obvious mistakes
never reach git; everything else surfaces as git's own stderr, like any other write. A new
branch never gets an upstream — that is `switch -c --track`'s job, and a different intent.

### 8.4 Graph

```
git log --all --date-order -z -n 300 --skip=<offset> \
  --format=%H%x1f%P%x1f%ct%x1f%an%x1f%s
```

Records NUL-separated (`-z`), fields separated by `%x1f`. Parents in `%P` drive lane layout.
Ref badges come from `for-each-ref` (§8.3), not `%d`.

`--all` includes remote-tracking refs, so `origin/*` branches appear in the graph. Intended.
If lane count becomes unreadable on a repo with hundreds of remote branches, add a branch
filter dropdown above the graph (Git Graph's approach) — v2, not v1.

Run `git commit-graph write --reachable` on first load of a large repo, and consider setting
`fetch.writeCommitGraph=true`, to keep traversal fast.

### 8.5 Commit details

```
git diff-tree --no-commit-id --name-status -r -z <hash>
git show -s --format=%H%x1f%an%x1f%ae%x1f%ct%x1f%B <hash>
```

### 8.6 Staging and commit

```
git add -- <paths>
git restore --staged -- <paths>
git commit -F -            # message via stdin, avoids arg-escaping pain
```

### 8.7 Remote operations

```
git fetch --prune --no-tags --quiet
git pull --no-rebase              # explicit: user config may set pull.rebase=true
git push
git push -u origin <branch>       # "Publish branch" — no upstream configured
```

**Background fetch must never block on a prompt:**

```
git -c credential.interactive=never fetch --prune --no-tags --quiet
env: GIT_TERMINAL_PROMPT=0
     GIT_ASKPASS=echo
     SSH_ASKPASS=echo
     SSH_ASKPASS_REQUIRE=never
```

On auth failure, mark the repo "auth needed" in the UI and stop retrying it in the
background. A **manual** fetch/push triggered by the user *is* allowed to prompt
interactively — they're sitting right there.

---

## 9. Roots, windows & persistence

### 9.1 Roots

A window opens **one root folder** and shows every repo discovered beneath it — the same
mental model as opening a folder in VS Code. The root is remembered and reopened on next
launch, along with window size, position and last selected repo.

- **First run, or saved root missing** (renamed folder, disconnected drive): show a welcome
  screen with *Open Folder…* and the recent list. Never an empty repo list, never a crash.
- *File → Open Folder…* **replaces** the root in the current window (VS Code's default).
  Use *New Window* to keep both.
- *File → New Window* opens a second window on another root.
- A **recent roots** list backs *File → Open Recent*.

### 9.2 One process, many windows

**Multiple windows, single process.** Tauri supports several `WebviewWindow`s over one Rust
backend; use `tauri-plugin-single-instance` so a second launch routes into the running
process and spawns a window there, rather than starting a second process.

This is not a preference — a second *process* silently breaks §7, because those guarantees
are process-local:

| | Single process, N windows | N processes |
| --- | --- | --- |
| Global git semaphore | One, honoured (8 total) | One each → 8×N spawns |
| Per-repo write queue | Shared, correct | Independent → two `git fetch` on one repo, `index.lock` contention |
| Cache file | One writer | Concurrent writers, corruption |

**Overlapping roots are therefore safe.** Open `C:\dev` in one window and
`C:\dev\microservices` in another and the same repo appears in both; because write queues
are keyed by **canonicalised repo path** and shared across windows, operations still
serialise correctly, and a status change refreshes the repo in every window showing it.

### 9.3 Ownership

**Rust owns all state.** The 77 repos' status lives in a Rust structure behind a lock; the
frontend is a pure view fed by Tauri events. No mirrored state in JS — otherwise
reconciliation logic gets written twice and the cache becomes a third source of truth.

### 9.4 Selection is a set

`selected: HashSet<RepoId>` **per window**, with a v1 invariant of `len() <= 1`. Costs
nothing now and keeps the v2 multi-repo commit from requiring a rewrite.

### 9.5 Files

| File | Location (Tauri) | Contents |
| --- | --- | --- |
| `settings.json` | `app_config_dir` | Global: pane widths, scan depth, sweep intervals, recent roots |
| `roots/<hash>.json` | `app_config_dir` | Per root: pins, last selected repo, window size/position |
| `cache/<hash>.json` | `app_cache_dir` | Per root: branch, dirty, ahead/behind, `last_fetch_at` |

`<hash>` is a short hash of the canonicalised root path. **Cache and pins must be per-root**
— one shared `cache.json` would have two windows overwriting each other's state.

Pins are per-root because a pin identifies a repo, and repos live under a root. Pane widths
are global because they're a display preference.

Rules:

1. **Atomic writes** — temp file + rename. A crash mid-write must not corrupt either file.
2. **Versioned schema** (`"version": 1`) with a migration path.
3. `cache.json` is a **cache, never truth**. On any parse failure: delete, rebuild silently,
   never show the user an error.
4. **Debounced writes** — every 30 s and on quit, not on every status change.
5. Deleting the cache must never lose pins. Hence two files.

---

## 10. Platforms

**v1 targets Windows.** Linux is a v2 deliverable, not a v1 constraint.

The code stays **portable by construction** — no Win32 APIs, no hardcoded path separators,
no shelling to `cmd`, all paths via `PathBuf`. Tauri gives cross-platform packaging for
free when the time comes.

The deferred cost is real and is why it waits: Linux renders in WebKitGTK rather than
Chromium, so large-DOM performance and CSS behaviour differ from the Windows build and need
a real Linux box to verify. Packaging (AppImage/deb) is additional work.

Practical implication for v2: keep graph row counts capped aggressively (§5.3) — WebKitGTK
is slower than Chromium at large DOM.

---

## 11. Theming

**Dark only in v1.** No theme toggle, no `prefers-color-scheme` handling. This is a personal
tool used mostly at night, and a second theme doubles every visual decision for zero v1
value.

One constraint so that light mode later isn't a rewrite: **all colour goes through CSS
custom properties** on `:root` (`--bg-app`, `--bg-surface`, `--bg-raised`, `--border`,
`--text-primary`, `--text-muted`, `--accent`, `--status-dirty`, `--status-error`,
`--lane-1..8`). **No hex literals in component styles.** Adding a light theme then means
shipping a second token block, not touching 40 components.

Three rules for the dark palette:

1. **Not pure black.** Use a lifted surface ramp (roughly `#141517` app → `#1a1b1e` surface
   → `#222428` raised). Pure black against light text causes halation and makes elevation
   impossible to express. VS Code Dark+ and GitKraken both do this.
2. **Elevation via surface lightness, not shadows.** Shadows read poorly on dark; a panel
   one step lighter than its background reads correctly.
3. **One accent colour**, used for selection and primary buttons only. Status colours
   (dirty, ahead/behind, error) are semantic and must never reuse the accent, or the repo
   list stops being scannable — which is the entire point of the left pane. Settled on an
   indigo/blurple (`--accent: #5865f2`, hover `#7c87f5`, muted/selection fill `#262b5e`) after
   live-comparing it against violet, electric cyan and fuchsia in the running app — bold
   enough to read as a deliberate choice next to VS Code Dark+ blue, but far enough from the
   status hues (amber/green/orange/red) to stay unambiguous at a glance.

Set the Tauri window background colour to `--bg-app` so the window doesn't flash white
before the webview paints.

Graph lane colours are a fixed palette of ~8 hues cycled by lane index, chosen to stay
distinguishable from each other *and* from the accent and status colours on the dark
surface.

---

## 12. Distribution & updates

Two **separate** signing systems, easy to confuse:

| | Purpose | Key |
| --- | --- | --- |
| **Authenticode** | Windows trusts the installer; suppresses SmartScreen | Code-signing cert |
| **Tauri updater** | App verifies an update is genuinely yours | minisign keypair via `tauri signer generate` |

The updater keypair is free and mandatory if auto-update is on — public key goes in
`tauri.conf.json`, private key stays in CI secrets. The Authenticode cert is the one that
costs money.

### Auto-update

Tauri's updater plugin against a manifest hosted on **GitHub Releases** (free, and the
standard path). Check on launch, download in the background, apply on next start. Roughly a
few hours of work.

### Code signing — price this before committing

- OV code-signing certificates now require the private key on FIPS-140-2 hardware (USB
  token or cloud HSM); you can no longer just download a `.pfx`. Budget ~€200–400/year plus
  token logistics.
- **Azure Trusted Signing** (~$10/month, integrates with `signtool`) is the cheapest and
  least painful route. Verify individual-developer eligibility and identity-verification
  requirements before planning around it — the rules differ for individuals and orgs.
- **A new OV certificate has no SmartScreen reputation**, so early builds may still warn
  until enough downloads accumulate. Only EV certificates get instant reputation.

Honest recommendation: if Corgit stays private, **ship auto-update in v1 and defer
Authenticode**. Clicking through SmartScreen once, on your own machine, costs nothing.
Signing becomes worth it the moment you hand a build to a colleague — wire the CI signing
step in then, not now.

---

## 13. Error handling

The rule: **never strand the user in a state Corgit can't get them out of.** Every failure
path ends in either a recovery action or *Open in VS Code*.

### Merge conflict

`pull` is `fetch` + `merge`, and merge is precisely what produces conflicts. Detect via
`u` records in `status --porcelain=v2`, or the presence of `.git/MERGE_HEAD`.

On detection:
- Banner in the middle pane *and* a conflict marker on the repo row — row-level Pull
  (§5.1) means a conflict can be created in a repo that isn't selected, so the row must be
  able to show it
- Exactly two buttons: **Abort merge** (`git merge --abort`) and **Open in VS Code**
- **Block commit and push** for that repo until resolved or aborted

### Other failures

Translate the common cases into plain language, with raw stderr always available in a
collapsible "Details":

| Case | Message | Action |
| --- | --- | --- |
| Push rejected (non-fast-forward) | "Remote has commits you don't have" | Pull |
| Pull with dirty tree | "Commit or discard your changes first" | Open in VS Code |
| Checkout blocked by local changes | git's own stderr | Open in VS Code |
| Auth failure (background) | Repo badged "auth needed" | Manual fetch (may prompt) |
| No upstream | Button becomes "Publish branch" | — |
| `index.lock` present | "Another git process is running" | Retry |

---

## 14. Naming

Product name: **Corgit**. Crate, package and identifier: `corgit` (lowercase), bundle
identifier `dev.kronborg.corgit`.

Corgi + git. Corgis are a cattle-herding breed, which is what this app does — a herd of
repositories, kept in line, with the five that need attention nipped to the front. The
earlier name (`twogit`, from Twoday + git) described where it was written rather than what
it does, and the pun didn't carry.

The dog is the brand, in the TunnelBear sense: the mascot is expected to appear often and
without embarrassment. The constraint that makes that survivable in a three-pane dense
layout is **§14.1**.

### 14.1 Where the mascot may appear

The rule: **dead space and dead time, never over live data.** Density is the product's
promise, so the dog never costs a row of repositories, files or commits.

The full brief for whoever draws him — character, palette, the set of poses needed, and
what has already been ruled out — is **[mascot.md](mascot.md)**.

Permitted:

| | |
| --- | --- |
| App identity | icon, taskbar, installer, tray, favicon, About dialog |
| Empty states | welcome screen, no repo selected, no commits yet, no filter matches |
| Transitions | reading history, fetch/pull in flight, first scan of a large root |
| Failures | `GitErrorNotice` — the dog softens git's worst moments (§13) |
| Resting | every repo clean and in sync: the payoff state, dog lies down |

Not permitted: inside repository rows, file rows, graph rows, or the commit info panel.
One exception is allowed by the icon rule — a mascot glyph may serve as a *button* icon
where the verb fits (Fetch), because that reads as chrome, not decoration.

---

## 15. Open questions

Resolved: Svelte 5 · filter matches repo name only · 60 s sweep with re-entrancy guard ·
graph shows all refs, switcher dedupes local/remote · dark only · auto-update yes, signing
deferred (§12) · single dirty dot · row-level Pull · close quits · one root per window with
recent list and multi-window (§9) · absolute `dd-MM-yyyy HH:mm:ss` timestamps · native menu
bar (§4.1) · window restores size, position and last selected repo.

· scan depth 1 (§8.1) · *Open Folder…* replaces the current window's root.

Still open, none blocking:

- [ ] Is there a cap on windows, or on total repos across all open roots?
- [ ] **Should the status sweep emit incrementally rather than in one batch?**
      Step 2 emits a single event when all repos are done, so every row stays
      branch-less until the whole sweep lands — 20 s on a cold start on the
      first machine measured. Cache-first paint (step 3) should hide this,
      since rows arrive filled in from disk and the sweep only corrects them.
      Decide *after* the cache is in and only if it still reads badly: doing
      both means 77 IPC round trips per sweep to solve a problem the cache may
      already have solved. The change is local to `collect()`.

---

## 16. Build order

Each step ends somewhere useful:

1. **Skeleton** — Tauri app, three resizable panes, persisted widths
2. **Discovery + status** — welcome screen, *Open Folder…*, scan the root,
   `status --porcelain=v2`, populate the repo list.
   *Already useful on its own.*
3. **Cache + sweep** — per-root cache, instant first paint, focus-gated 60 s sweep
4. **Middle pane Mode A** — staging, commit
5. **Remotes** — push, publish branch, pull, background fetch sweep, ahead/behind
6. **Graph** — log parsing, lane layout, virtualized rows, Uncommitted Changes node
7. **Middle pane Mode B** — commit details
8. **Branch switching + conflict detection**
9. **Polish** — pins, filter, watchers on hot repos, context menus, error translation,
   native menu bar, multi-window + recent roots
10. **Ship** — GitHub Releases, Tauri updater keypair, auto-update wired end to end
    (Authenticode deferred per §12)

Measure against the §1 budget at step 3 and again at step 6. If cold start exceeds 500 ms,
stop and fix it — that number is the reason this project exists.
