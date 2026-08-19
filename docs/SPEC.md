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
- Per-repo status: branch, changed-files count, ahead/behind
- Stage / unstage files (file-level)
- Commit (staged files only)
- Push, including "Publish branch" for a branch with no upstream
- Pull (merge only) and fetch
- Merge (via pull, or a branch picked from the graph's context menu — §8.3); conflict
  *detection* only
- Branch switching (local and remote-tracking)
- Commit graph for the selected repo, with commit details on click
- Read-only side-by-side diff for one file at a time, working tree or commit (§5.4)
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
5. **Multiple windows, one per root** (§9.2) — v1 is one window on one root. The single
   *process* half of that design ships in v1 and is what keeps §7 honest; the second
   window is the deferred part, and it is not a small one: every root-scoped field in
   `AppState` becomes per-window, and every command has to be told which window is asking.

### Non-goals, permanently

Replacing VS Code for conflict resolution or diff editing. When Corgit hits something it
doesn't handle, the escape hatch is a single **Open in VS Code** button.

*Reading* a diff is in (§5.4) and does not weaken this: the viewer is read-only, has no
hunk staging, no editing and no conflict resolution, and every case it cannot render —
binary, oversized, conflicted — ends at that same button rather than at a half-measure.

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
│ 🐕  File   View   Repository   Help                        ─   □   ✕ │
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
unusable. The same is true of the boundary inside the diff view (§5.4), and *View ▸ Reset
Pane Sizes* returns every draggable boundary in the window — not only the pane ones — to
its default.

A fourth column — the **commit info panel** (§5.2) — opens to the right of the graph when
a row's right-click menu asks for it. It is fixed-width and deliberately outside all of the
above: no divider, no stored fraction, nothing to reset. The graph's track absorbs it, and
opening it must never resize the two panes to its left.

### 4.1 Title bar and menu

**One row**, in the client area: app mark, the four menus, then minimize / maximize /
close. The window is `decorations: false`.

| Menu | Items |
| --- | --- |
| **File** | Open Folder… `Ctrl+O` · Open Recent ▸ · Close Window `Ctrl+W` · Exit |
| **View** | Toggle Repo List · Toggle Commit Pane · Reset Pane Sizes · Reload |
| **Repository** | Fetch · Pull · Push — acting on the selected repo, mirroring the buttons for discoverability. Disabled when no repo is selected. Push reads *Publish Branch* on a branch with no upstream, matching §8.7's button. |
| **Help** | About · Check for Updates · Open Log Folder |

#### This reverses an earlier decision, and the earlier reasoning was not wrong

This section used to specify a **native Windows menu** via `tauri::menu`, explicitly *not*
an HTML reimplementation, on the grounds that VS Code hand-rolls its menu only because it
needs three platforms and a command palette, while native gives OS-consistent behaviour,
keyboard accessibility and accelerators for free and costs no CSS. All of that was true and
all of it still is.

What it did not account for is that on Windows a native menu is **its own row, below the
caption** — the OS stacks them and there is no arrangement in which they share one. Two
OS-drawn rows above a window whose entire purpose is fitting as many repository rows on
screen as possible cost ~30px permanently. Merging them means drawing the caption
ourselves, and a custom caption cannot host a native menu. So the menu became HTML as a
*consequence* of the title bar, not because the original argument stopped holding.

What it cost, all of it real:

- **Accelerators** are re-registered by hand (`TitleBar.svelte`). The key and the label
  shown beside the item are now two declarations that can drift; `menuModel.test.ts` pins
  them together.
- **Alt mnemonics** (Alt+F for File) are **not** reimplemented. They want underlined
  letters, an Alt-held state, and a collision rule against shortcuts. Recorded as absent
  rather than half-built.
- **Snap Layouts** — Windows 11's flyout when hovering Maximize — is gone, and this is
  measurable rather than assumed: the maximize button reports `HTCLIENT` to `WM_NCHITTEST`
  where a native frame would report `HTMAXBUTTON`. `Win`+arrow snapping is OS-level and
  unaffected. Restoring the flyout needs hand-rolled `WM_NCHITTEST` or a plugin; judged not
  worth a new dependency or a `#[cfg(windows)]` block for v1.
- **Screen-reader semantics** are hand-built out of `menubar`/`menuitem`/`menuitemcheckbox`
  roles instead of being inherent.

What it did **not** cost, because these were checked rather than assumed:

- **Resize edges** all still grab — the window keeps `WS_THICKFRAME`, and all eight zones
  report correctly.
- **Rounded corners** survive.
- **The maximized overhang is not a bug.** The maximized window rect is the work area
  inflated by 9px on every side, which looks exactly like the well-known custom-titlebar
  defect where the close button lands off-screen. It is not: tao handles `WM_NCCALCSIZE`,
  so the *client* rect is already the work area at screen 0,0 and the overhang is invisible.
  Padding it away costs a visible 9px band on all four sides and buys nothing. Measure the
  client rect, not the window rect, before believing otherwise.

#### Where each item's behaviour lives

Unchanged by the move, and deliberately so. Items that only touch process lifecycle or a
boolean Rust owns (§9.3) — Close Window, Exit, the View checkboxes, About, Open Log Folder
— are invoked as `menu_command` and handled in `menu.rs`. Everything else is a frontend
store method called directly. Those used to make a round trip out to Rust and back as a
`menu:action` event purely because the menu lived on the Rust side; with the menu in the
webview, that event has no cargo and is gone.

Two things Rust used to *push* are now *derived* instead: Repository's enabled state comes
from `repos.selectedId` rather than `set_repo_selected`, and Open Recent renders from
`settings.data.recentRoots` rather than being repopulated on every change. The one thing
still pushed is `pane:visibility`, because Rust owns those booleans and outlives the
webview — the frontend asks for them once on load, since unlike a native menu an HTML one
is rebuilt by every reload.

---

## 5. Panes

### 5.1 Repo list (left)

Two sections, each alphabetical:

- **Pinned** — the hot set (§6). This used to be the FS-watch budget as well, and is not any
  more: every repo is watched on Windows, so pinning buys position in the list, not
  freshness. Keep the one-click affordance anyway — it earned it as navigation for 77 rows,
  which is what it now is. Every row carries a pin
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

**Row contents:** repo name · current branch · changed-files badge · ahead/behind badge.

The **changed-files badge** is a filled dot grown enough to hold a number — the count of
files with uncommitted changes, and nothing finer. No distinction between staged and
unstaged; that detail lives in the middle pane. The row answers "does this need me?", and
the number answers "how much?", which is the difference between a typo fix and an
afternoon's work and is worth the two extra glyphs.

The count is **distinct paths**, not the sum of the per-side totals: git reports one record
per path with a state on each side, so a file that is staged and then edited again appears
in both. Summing was correct while the row drew a dot — anything non-zero meant the same
thing — and became a small lie the moment the row printed a number, so `RepoStatus` carries
its own `changed_files` counted off the records themselves (§8.2).

It stays a **fill**, not coloured text like ahead/behind: those are read after you have
decided a row is interesting; this one is what makes you decide, and a solid shape is what
survives a scan down 77 rows. The fill is **neutral** (`--count-bg`), not a status colour:
the badge's presence is the state, its number is a quantity, and a hue on it says the same
thing twice in a palette where every hue already means something else (§11).

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

### 5.2 Middle pane, and the commit info panel

The middle pane is the **working tree**, always. Commit details are a **fourth column**
that opens beside the graph — see the end of this section for why that replaced the
original modal design.

**Working tree** (the middle pane's only mode):

```
[ commit message textarea ]
[ Commit ]  [ Push / Publish branch ]   [ Fetch ] [ Pull ]

Staged Changes (2)          [− unstage all]
  M  src/main.rs
Changes (14)                [+ stage all]
  M  README.md                        [↺] [+]
  M  lib.rs      src                  [↺] [+]   ← selected ┐ ctrl-clicked,
  D  old.txt     docs/archive         [↺] [+]   ← selected ┘ then right-clicked
  ?  notes.txt   docs                      [+]
                             ┌──────────────────────────────┐
                             │ Stage 2 files                │
                             │ Discard changes to 2 files…  │
                             └──────────────────────────────┘
```

- **Commit commits staged files only.** Disabled when nothing is staged or the message is
  empty.
- File rows: status letter, **filename**, then the directory trailing behind it in muted
  smaller type; hover reveals `+`/`−` stage/unstage buttons. The filename leads because it
  is what the row is for and what the eye scans a list of changes by — a left-aligned full
  path buries it behind however deep the file happens to sit. When the row runs out of
  room the directory is trimmed first, and both halves are trimmed at the tail: what a
  path can afford to lose is its deepest folder, not the segment naming the project. The
  full path stays on the row's tooltip.
- **Never a folder row.** Status is read with `-uall` (§8.2) so a wholly-untracked
  directory is listed as its files rather than collapsed to `? dir/`. Collapsed, staging
  the row replaced it with N file rows — the thing you looked at was not the thing you
  acted on — and the repo row's untracked count meant folders while every count beside it
  meant files.
- **Click a file → its diff opens in the right pane** (§5.4). The section the row is in
  decides which two sides get compared, because that is the only thing that knows: a
  partly-staged file appears in both lists at once with a different diff on each.
- **Ctrl-click and shift-click build a selection; right-click acts on it.** Staging six of
  fourteen files is the pane's most ordinary job, and one `+` per file is six round trips
  through the write queue for what is one act. A modified click therefore selects instead
  of opening a diff — a diff per ctrl-click would spawn `git diff` for every row picked on
  the way to staging them, and leave the right pane on whichever was last. Shift extends
  from the last row clicked deliberately, so the same anchor holds while the range is
  dragged up and down. Four rules, none of them negotiable:
  - **A selection belongs to one section.** *Staged* and *Changes* have opposite verbs, so
    a set spanning both leaves the menu with nothing honest to offer. Ctrl-clicking across
    the divide starts a new selection rather than growing the old one.
  - **Right-clicking a row outside the selection replaces it**, the way every file list
    does. The menu must never act on rows the user cannot see are picked.
  - **The row's own `+`/`−`/`↺` still act on that row alone**, selected or not. A button
    attached to one row that quietly acted on five would be the tick column back in a
    worse form.
  - **The selection is transient.** It survives no repo switch, and staging clears it by
    construction: the rows leave the section and the selection empties with them, rather
    than lingering over whatever slid up into their place. Nothing about it is persisted
    (§9.5) — it is not state the backend owns.
- **The menu**, per section: *Stage N files* / *Unstage N files*; *Discard changes to N
  files…* in *Changes* only, dropping untracked rows and saying so when it does (git
  rejects a pathspec list wholesale, so one `?` row would take the whole discard down with
  it); *Reveal in File Explorer* on a single row only, because `explorer /select,` takes
  one path and N files would mean N windows rather than one window with them all picked
  out. A file that no longer exists — a `D` row — reveals the nearest folder that does,
  never a silent jump to Documents.
- **The selected rows and the open diff are marked differently**: selection fills the row,
  the diff on screen gets an accent bar down its left edge. With six rows filled, a second
  fill that only differed in shade would lose the one row the right pane is actually
  showing.
- **File list is capped at 100 entries per section.** The header must then read
  `Changes (100 of 3,412)`. "Stage all" still stages everything and its tooltip says so
  explicitly — the user must never commit files the UI silently hid.
- **Discard** (`↺`, hover-revealed beside `+`) throws away a file's **unstaged** changes —
  `git restore --worktree` (§8.6), so a partly-staged file keeps its staged half. The `↺`
  is one row; the context menu above discards a whole selection through the same dialog.
  **Rows still carry no tick column and there is no selection bar**: a checkbox per row
  made *Changes* read as a form to be filled in rather than a list of what changed, which
  is the one thing this pane has to be scannable as, and it charged every user for a batch
  most of them were not making. A ctrl-click costs the list nothing when nobody uses it.
  Three limits, all deliberate:
  - **Only in *Changes*.** A staged row keeps `−` alone. Discard there could only mean
    "throw away the staged work too", which is not what a button sitting beside `−` reads
    as; unstaging first moves the row here, where discard means one plain thing.
  - **Never on an untracked (`?`) row** — no `↺`. Git has nothing to restore an untracked
    file from, so discarding one could only be `git clean` deleting it. **Corgit does not
    delete files.**
  - **Always confirmed**, by a modal listing every path and saying what goes and what
    stays. §8.3 refuses force-checkout because it "silently discards work"; this is the
    same act done loudly, and it is the only thing in the app that destroys work git cannot
    give back. `git revert` and `git reset` stay out of v1 (§2) — this is neither.

**Commit info panel** — a fourth column to the right of the graph, opened from a row's
**right-click ▸ Info**:

```
COMMIT                                        ✕
a3f9c21  feat: add retry logic
Local   ▸ main
Remote  ▸ origin/main
Jeppe Kronborg · 12-08-2026 14:03:11
[ full commit message ]

Changed in this commit                         7
  M  src/main.rs            +5  −0
  A  src/retry.rs          +12  −0
```

Read-only. Clicking a file opens that commit's diff against its parent in the right pane
(§5.4) — the same viewer the working-tree rows use.

**Why a column and not a mode.** The original design made this Mode B of the middle pane,
with a *← Back to changes* affordance. That was wrong in a way only visible once both
existed: reading a commit and staging work are not alternatives, and putting them in one
slot meant every glance at history cost the staging state its place on screen. A fixed
320 px column costs the graph width it can spare and costs the middle pane nothing.

It is **not resizable** and has no stored fraction — the graph's `1fr` track absorbs it
(§4). The only constraint it adds is that the graph must still clear its minimum width
while the panel is open.

**Opening it is a deliberate act, and selecting a row is not one.** *Info* on a row's
context menu is the only way in. Selection used to be: clicking any commit opened the
column, which meant reading the graph — the ordinary thing to do in that pane — cost a
320 px reflow every click, and shelled out to `git show` for a commit the user was only
scrolling past. Right-click ▸ Info separates "I am looking at the graph" from "tell me
about this one".

Every row's menu carries *Info*, including rows with no ref badges on them, which before
this had no menu at all. It is the first entry, above the branch entries (§8.3) that only
appear on rows carrying a badge.

**Once open, it follows the selection** and shows whichever commit is picked, so browsing
with the column up works the way any detail view does — and the highlighted row and the
column can never disagree about which commit is on screen. **Nothing is fetched while it is
shut**: a click on a row with the column closed paints the row and does nothing else.

**Closing it**, three ways, none of which disturb the selection:

- the panel's own ✕;
- **Esc**;
- the *Uncommitted Changes* node, or a click on empty graph background past the last row.
  Both mean "back to the working tree", which leaves no commit for the column to be about.

**Asking for the same commit twice is inert**, not merely idempotent — see §8.5 on why
re-fetching an immutable commit is a visible regression rather than a no-op. The state that
makes this work is the hash the panel is *showing or fetching*, which is not the same as
the hash it has already loaded.

### 5.3 Graph (right)

The right pane is **two views behind a tab strip** in its header: the graph below, and the
open file's diff (§5.4). Selecting *Graph* does not close the diff — the tab stays, and
the graph keeps its scroll position and loaded pages, so glancing between the two is free.

Selected repo only — one repo at a time, so graph cost never multiplies by 77.

- **Synthetic "Uncommitted Changes" node** pinned at the top when the working tree is dirty.
  Clicking it selects the working tree, which also closes the info panel (§5.2). This is
  what ties the panes into one coherent surface.
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
- Click a commit → it is selected, and nothing else happens. **The info panel does not
  open on selection** (§5.2); *Info* on the row's context menu opens it.
- Right-click a commit → **Info** first, then the branch entries for any ref badges the row
  carries (§8.3). Copy hash, Copy message and Open in VS Code belong here too. (Thin by
  design — the graph is a viewer in v1.)

**Rendering: SVG lanes + virtualized DOM rows.** Not canvas. A few hundred SVG paths for
the lanes, HTML rows for text — this gives text selection, hover and context menus for
free, and avoids hand-rolled hit-testing. Canvas only pays off past ~10k simultaneous rows,
which we never render.

**Lane layout** is implemented in-house. Do not parse `git log --graph` ASCII output.

### 5.4 Diff view (right pane, second view)

One file at a time, read-only, opened by clicking a file row in either §5.2 mode.

```
┌──────────────────────────────────────────────────────┐
│ [ GRAPH ] [ main.rs × ]                              │
├──────────────────────────────────────────────────────┤
│ src/main.rs                                   +2 −1  │
│ INDEX                    │ WORKING TREE              │
│  11   let a = 1;         │  11   let a = 1;          │
│  12   old();             │  12   new();              │
│                          │  13   extra();            │
│           ⋯ 34 unchanged lines ⋯                     │
│  47   let b = 2;         │  48   let b = 2;          │
└──────────────────────────────────────────────────────┘
```

- **Side by side, never unified.** Old on the left, new on the right, each column captioned
  with what it actually is — `HEAD`/`Index` for a staged row, `Index`/`Working tree` for an
  unstaged one, `<hash>~`/`<hash>` for a commit.
- **Both columns are sized against the pane, never against the content**, and the boundary
  between them is draggable, with the fraction persisted beside the pane widths (§9.5).
  Sizing them to the longest line instead is the obvious implementation and is wrong: on
  any file with real code in it the old column alone exceeds the pane, so the new column
  starts off the right edge and the two cannot be read together — which is the only reason
  to put them side by side.
- **Alignment is positional, not computed.** Git already decided which lines changed; a run
  of removals is zipped against the run of additions that follows it and the shorter side
  gets an empty filler cell. There is no second diff algorithm in the frontend, and there
  must not be one — it would disagree with the patch it was handed.
- **Lines never wrap.** Uniform row height is what makes the rows virtualizable, and a
  wrapped row silently breaks the two columns' alignment. Each column clips its own text
  instead, and **one shared horizontal offset moves both** — comparing two lines means
  seeing the same columns of characters on each side, which independent offsets break.
  That offset gets its own scrollbar strip along the bottom, whose travel is the longest
  line in the *whole* file less the narrower of the two columns: with rows virtualized,
  `max-content` would be measured from whichever rows happen to be mounted and the scroll
  extent would shift under the user as they scrolled vertically.
- **Unchanged regions between hunks are one row** saying how many lines were skipped. Only
  the three lines of context git sends are ever shown; there is no "expand" affordance.
- **Line-level tinting only.** No word-level intra-line highlighting and no syntax
  highlighting — the latter needs a real dependency, and the frontend has exactly one.
- **An *Open in VS Code* button sits in the file header, in every state** — the one action
  a read-only view can offer, and it must not move around between states or it becomes
  something to look for. It opens the repo folder *and* the file: a lone file gives a
  VS Code window with no source control and no search around it, which is most of what
  made offering VS Code worth doing. For a working-tree source it also jumps to the first
  changed line; for a commit it does not, because that side is an older revision and its
  line numbers would land the reader wherever those lines sit today.
- **Four cases have nothing to render and rely on that button:** a binary file, a diff past
  the backend's line cap (§8.8, truncated rather than silently short), a conflicted file
  (§13 — resolution is VS Code's job), and an untracked file large enough to be a build
  artefact. An untracked file below that limit renders as an all-additions diff, since that
  is what it is.
- Switching repos closes the diff — the path it names may not exist in the new one. Esc
  closes it. A write to the repo (stage, unstage, commit) re-reads an open working-tree
  diff; a commit's diff is immutable and is never re-read.

---

## 6. Data & refresh model

### Hot vs cold — a layout distinction, not a refresh one

**Hot** = pinned repos ∪ currently selected repo. **Cold** = everything else.

This once decided who got FS watchers. It no longer does: on Windows every repo is watched
(below), so hot/cold survives only as a **layout and ordering** idea — the *Pinned* section
of §5.1. Pinning is where a repo appears, not how fast it updates.

That reverses an earlier decision, and the earlier reasoning was not wrong — it was
measured on the wrong platform. See below.

### Watchers

**Where the platform gives subtree watches, watch the whole working tree, one watch per
repo.** On Windows that is `ReadDirectoryChangesW`, which watches a subtree through a
single handle *regardless of its depth* — so 77 repos cost 77 handles, not 77 × every
directory beneath them. Measured over the 69-repo bench root: **17.3 ms to establish all of
them, +73 handles, +4.4 MB** — nothing against §1's 150 MB.

**Where it does not, watch `.git/HEAD`, `.git/refs/**`, `.git/index` only**, and leave
working-tree changes to the sweep. This is the original v1 rule and it stays correct on
inotify platforms, where a recursive watch costs one descriptor per directory and 77 trees
means 77 `node_modules` and a blown `max_user_watches`. Linux is a v2 deliverable (§10);
this is one of the places the two platforms genuinely differ rather than one being a
portable subset of the other.

**Why this is worth the asymmetry.** The status sweep costs one `git status` process per
repo, and process creation — not git — is 85–95 % of that. Measured on the bench machine: a
bare `git version`, which opens no repository at all, costs 85.7 ms wall (19.5 ms user,
29.7 ms kernel, 36.5 ms waiting), while git's own work on a typical repo is 2–10 ms. 69
repos therefore cost 1.2 s at best and ≈6 s often, whatever the concurrency — raising the
in-flight cap from 8 to 16 buys 7 %, and to 32 buys nothing. **§1's 300 ms budget is
unreachable for as long as a tick spawns one process per repo.** Watching the trees is not a
latency optimisation layered on the sweep; it is what removes the periodic 69-process work
altogether. Steady state is ~0 spawns per tick.

Debounce `.git` events ~200 ms — git writes several files per operation, and these are the
ones the user is waiting on after a terminal command. Debounce working-tree events longer,
~1 s: nobody is watching those land.

**Build churn, not handle count, is the cost to manage.** A `cargo build` or a webpack run
fires thousands of events for paths git will never report. Two defences, both of which may
only ever make a refresh *late*, never wrong:

1. **Coalesce per repo** — at most one status read per repo per ~2 s, however many events
   arrive. A minute-long build costs that repo ≤30 reads, not thousands.
2. **Skip the usual build outputs before debouncing** — `node_modules`, `target`, `dist`,
   `bin`, `obj`, `.next`, `.venv`. This is a guess and it is allowed to be wrong: a repo
   that really does track `dist/` gets its refresh from the reconciliation sweep instead.
   Correctness never rests on the list being right.

**A repo whose watcher cannot be established must not be treated as though it were** — a
network share, a permissions failure, a `.git` file rather than a directory. Those repos
stay on the sweep interval. Mixed mode is the normal case, not an error path.

**An overflow is a lost history, not a lost repo.** When the platform reports that it
dropped events, treat that repo's status as unknown and read it outright.

### The two sweeps

These are different mechanisms and must not be conflated. Fetch does **not** detect dirty
repos.

| | Status sweep | Fetch sweep |
| --- | --- | --- |
| Network | No | Yes |
| Cost | ~60–340 ms/repo, almost all of it process creation | ~0.5–2 s/repo |
| Detects | dirty, staged, conflicted, branch, **ahead** | **behind** |
| Interval | 60 s unwatched-only, full pass every 5th tick | 5–10 min, jittered |
| Concurrency | 8 | 4 |
| Runs when unfocused | No | No |

**The status sweep is a reconciliation pass, not the refresh mechanism.** The watchers keep
the rows current; the sweep exists to repair what they can miss — an unwatchable repo, a
dropped-event overflow, anything that happened while the window was blurred. It is still
what runs on focus gain.

It keeps a 60 s tick, but most ticks cover **only the repos with no working watcher**,
which is normally none of them and therefore costs nothing at all — no processes, no event,
no cache write. Every fifth tick is a full pass over every repo, which is the one that
still costs 69 processes and is why it happens every five minutes rather than every
minute. An unwatchable repo is not made to wait for that: it is on the 60 s cadence it
always had.

**Re-entrancy guard:** a sweep never starts while one is in flight — the tick is skipped,
not queued. At 77 repos the status sweep should finish in ~150 ms and never collide, so
this is cheap insurance rather than an expected path. Individual repos are also skipped
while their write lock is held (§7).

**Focus gating:** sweeps run only while the window is **focused**. On blur the tickers are
aborted outright rather than left running and skipping their own ticks; on focus they
restart and the root gets an immediate status sweep. With no tray and close-means-quit
(§9.1), the app has no background lifetime at all — background CPU is zero, not merely low.

**The watchers are dropped on blur too, for that same promise.** Left running they would
wake the app on every background build — *low* CPU rather than *no* CPU, a weaker guarantee
than the one above, and the one above is worth keeping. Re-establishing all of them costs
17 ms, and the focus-gain sweep already covers whatever changed while they were gone.

Fetch additionally: skips repos with no remote, skips repos fetched within the last
interval, and records `last_fetch_at` per repo.

### Making `git status` fast on Windows

This section used to open by asserting that `git status` cost is dominated by untracked-file
scanning. **On the bench machine it is not, and the measurement is worth keeping because of
where it redirects the effort.** `-uno` saves 82 ms on a 32k-file repo and ≈4 ms on every
other one; `-uall` and `--no-renames` change nothing at all. Untracked scanning is a
rounding error next to the 60–85 ms it costs merely to *start a process* here.

So the FSMonitor remedy below is aimed at the 5–15 % of a status read that is actually git:

```
git config core.fsmonitor true
git config core.untrackedCache true
```

Corgit should **offer** this per repo (a one-click banner), never apply it silently — it
modifies the user's config. It is worth having for the genuinely large repos, where git's
own work does clear the spawn floor, and worth nothing for the rest.

A Defender or EDR exclusion for the repo roots and `git.exe` remains the largest single
lever on spawn cost, and on a managed machine it is the one you cannot pull yourself. The
bench machine runs Microsoft Defender for Endpoint, where a trivial `cmd.exe /c ver` costs
the same ~80 ms — so this is machine-wide rather than anything about git.

**Neither of those changes the architecture, which is the point of the watchers above:** the
sweep cannot meet §1's budget by getting better at spawning, only by not spawning.

---

## 7. Concurrency rules

These guarantees are **process-local**, which is exactly why Corgit enforces a single
process (§9.2). Queues and semaphores are keyed by canonicalised repo path and shared
across everything in that process — which, once multi-window lands, means across every
window too.

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
git --no-optional-locks status --porcelain=v2 --branch -uall -z
```

Yields `# branch.head`, `# branch.upstream`, `# branch.ab +N -M`, plus `1`/`2` (changed /
renamed), `u` (unmerged → conflict state), `?` (untracked) records. Parse NUL-delimited.
This single call populates the repo row *and* the middle pane.

`-uall` is for §5.2's "never a folder row", not for speed, and it costs nothing: the
measurement below found `-uall` no different from the default, because untracked scanning
is a rounding error next to what it costs to start the process.

### 8.3 Branches

```
git for-each-ref --format='%(refname:short)%1f%(upstream:short)%1f%(objectname)' refs/heads refs/remotes
git switch <branch>                              # local
git switch -c <branch> --track origin/<branch>   # remote-tracking
git branch <new> <start-point>                   # create, stay put
git switch -c <new> <start-point>                # create and check out
git merge --no-edit <source>                     # into the checked-out branch
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

**Merging a branch** (same menu): right-click a ref badge in the graph → *Merge `<ref>` into
`<current>`*. One click, no modal — the label names both ends, which is the whole decision.
Only the source is chosen; the destination is always the checked-out branch, and the command
names no destination at all, so it is HEAD as git sees it rather than anything Corgit
cached (§5.1). Remote-tracking badges are offered too: merging `origin/main` into the branch
you are on is the same gesture, and it is the case Pull does not cover, since Pull only ever
merges *your* upstream. The badge for the current branch itself offers nothing — merging a
branch into itself is git's own no-op. On a detached HEAD the entry is absent entirely.

`--no-edit` for the same reason Pull passes `--no-rebase`: user config (`merge.edit`,
`GIT_MERGE_AUTOEDIT`) can otherwise summon an editor, and an editor spawned by a process
with no console is a hang with nothing on screen to explain it. Never `--no-ff`: a
fast-forward where one is possible is what was asked for, not a merge commit recording that
Corgit was involved.

A conflict is a normal outcome, not a special path — the merge fails like any other write,
the status refresh that follows every write raises §13's conflict banner, and *Abort merge*
is already the way out. The one thing merging needs that no other write does is to read
**stdout as well as stderr** for its error text: a conflicting `git merge` exits non-zero
with stderr empty and puts `CONFLICT (content): …` on stdout, so a stderr-only message
would be blank in exactly the case that most needs a sentence.

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
git diff-tree --no-commit-id -m --first-parent --root --raw --numstat -r -z <hash>
git show -s --format=%H%x1f%an%x1f%ae%x1f%ct%x1f%B <hash>
```

`--raw --numstat` together, rather than `--name-status`: git treats that one as exclusive
with `--numstat`, and the panel wants both halves — the status letter from the raw block,
the per-file +/− from the numstat block. They describe the same files in the same order,
so they are zipped **by position**, which sidesteps matching rename pairs across the two
blocks entirely.

**`-m --first-parent` and `--root` are load-bearing.** Bare `diff-tree` prints nothing at
all for a merge (it will not pick a parent on its own) and nothing for the root commit (it
has none). The graph is a plain `git log --all` with no `--no-merges` (§8.4), so both are
selectable rows — and without these flags every merge in a PR-merging repo opened a panel
reading "no files changed" over history that had been read correctly. `--first-parent`
because `-m` alone emits one diff per parent, repeating every path on an octopus merge.

`--cc` is the other way to make a merge non-empty and is **rejected**: on a
trivially-resolved merge it emits the numstat block with no raw block at all, and the
positional zip above then produces an empty list — the same bug wearing a different flag.
`-m --first-parent` also answers the question the panel is asking ("what did this merge
bring in") rather than "how was it resolved", which is why the panel labels a merge's file
list as compared with the first parent (§5.2).

**A commit is immutable, so its details are fetched once.** Nothing re-reads them — not the
`status:repo` event that reloads the graph, and not re-selecting the same row. This is the
same reasoning as `isLive` in the diff store (§5.4), and it is a correctness point, not an
optimisation: the fetch nulls the details it is replacing, so a redundant one blanks the
panel to its loading state and discards its scroll position.

### 8.6 Staging and commit

```
git add -- <paths>
git restore --staged -- <paths>     # unstage: index ← HEAD, working tree untouched
git restore --worktree -- <paths>   # discard: working tree ← index, index untouched
git commit -F -            # message via stdin, avoids arg-escaping pain
```

The two `restore`s are one flag apart and opposite in which half they keep, so the flags
are a named constant in `commit.rs` with a test on them. `--staged --worktree` together
would be a third thing again — it moves the source to HEAD and destroys both halves — and
is what §5.2's Discard must never become.

### 8.7 Remote operations

```
git fetch --prune --no-tags --quiet
git pull --no-rebase              # explicit: user config may set pull.rebase=true
git push
git push -u origin HEAD           # "Publish branch" — see below
```

`HEAD`, never the branch's name: a name can only come from cached status, and `git push
origin <name>` resolves the *local ref of that name* rather than what is checked out, so a
cache that had not caught up with an external `git switch` would push some other branch
while reporting success. `HEAD` is whatever git sees at the moment it runs.

**Publish is offered in two states, not one** (`needsPublish`):

1. **No upstream configured** — the obvious case.
2. **An upstream whose branch name differs from the local branch's** — `feature-x`
   tracking `origin/main`. Under git's default `push.default = simple` a bare `git push`
   refuses outright, so Push is the one button guaranteed to fail, while publish both
   succeeds and re-points the upstream at the matching remote branch.

The second case is not hypothetical: Corgit created it itself until `branch.rs` grew
`--no-track`. Git's `branch.autoSetupMerge` defaults to `true`, so `git switch -c <name>
origin/main` silently sets the new branch's upstream to `origin/main` — and no later fix
repairs a branch that already has it, only a publish does. Note the failure was safe only
by luck of the default: under `push.default = upstream` that same Push would have pushed a
feature branch onto `main`.

The accepted cost is that a *deliberately* mismatched upstream — local `feature` tracking
`origin/jk/feature` — is re-pointed by the next publish. A four-verb dashboard is better
off tidying an unusual config than offering a button that cannot work.

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

### 8.8 One file's diff

```
git diff          --no-ext-diff --no-color --no-renames -U3 -- <path>   # worktree vs index
git diff --cached --no-ext-diff --no-color --no-renames -U3 -- <path>   # index vs HEAD
git diff-tree --no-commit-id -p -r --no-ext-diff --no-color --no-renames -U3 <hash> -- <path>
```

`--no-ext-diff` because a configured external diff driver prints something we have no
parser for and may be interactive; `--no-color` because `color.diff=always` in a user's
config would salt the output with escape sequences; `--no-renames` because the pathspec
already pins one path and half a rename pair reads worse than the add/delete it falls back
to. The commit case deliberately uses the same `diff-tree` family as §8.5, so the diff and
the file list it was clicked in cannot disagree — including agreeing to show nothing for a
merge commit.

An **untracked** file never shells out: `git diff` reports nothing for one and
`--no-index` exits 1 by design, which is indistinguishable from a real failure. Read the
file and call every line an addition — that *is* the diff against nothing — subject to a
size cap first, and a NUL in the first 8000 bytes (git's own heuristic) means binary.

Parsing stops at **20 000 body lines** and reports the diff as truncated. Past that it is
no longer something a human reads, and rendering it means an IPC payload and a DOM row
count that neither the §1 budget nor the reader survives.

---

## 9. Roots & persistence

### 9.1 Roots

The window opens **one root folder** and shows every repo discovered beneath it — the same
mental model as opening a folder in VS Code. The root is remembered and reopened on next
launch, along with the last selected repo.

- **First run, or saved root missing** (renamed folder, disconnected drive): show a welcome
  screen with *Open Folder…* and the recent list. Never an empty repo list, never a crash.
- *File → Open Folder…* **replaces** the root in the current window (VS Code's default).
  With one window in v1, that is the only way to change root.
- A **recent roots** list backs *File → Open Recent*.

### 9.2 Exactly one process

**One process, enforced.** `tauri-plugin-single-instance` routes a second launch into the
running process — which, in v1, raises the existing window rather than spawning another.

This is not a preference — a second *process* silently breaks §7, because those guarantees
are process-local:

| | One process | N processes |
| --- | --- | --- |
| Global git semaphore | One, honoured (8 total) | One each → 8×N spawns |
| Per-repo write queue | Shared, correct | Independent → two `git fetch` on one repo, `index.lock` contention |
| Cache file | One writer | Concurrent writers, corruption |

Note what this table does *not* say: nothing here is bought by having many windows. The
single-process rule is the whole of the protection, and it holds with one window.

**v1 ships one window.** Multiple `WebviewWindow`s over the one backend are a v2 candidate
(§2), and the cost is in `AppState`, not in Tauri: the open root, its statuses, its pins
and its selection are currently one set of fields for the process, and each would have to
become per-window with every command told which window is asking.

Two things are built for that future and cost nothing now: write queues and the semaphore
are already keyed by **canonicalised repo path** rather than by root, and every event
already carries the root it describes so a receiver can ignore one it isn't showing.
Overlapping roots — `C:\dev` and `C:\dev\microservices` — are therefore safe by
construction whenever the second window does arrive.

### 9.3 Ownership

**Rust owns all state.** The 77 repos' status lives in a Rust structure behind a lock; the
frontend is a pure view fed by Tauri events. No mirrored state in JS — otherwise
reconciliation logic gets written twice and the cache becomes a third source of truth.

### 9.4 Selection is a set

`selected: HashSet<RepoId>` scoped to the open root, with a v1 invariant of `len() <= 1`.
Costs nothing now and keeps the v2 multi-repo commit from requiring a rewrite. (It becomes
per-window along with the rest of the root state if multi-window ships — §9.2.)

### 9.5 Files

| File | Location (Tauri) | Contents |
| --- | --- | --- |
| `settings.json` | `app_config_dir` | Global: pane widths, the diff view's column split (§5.4), scan depth, sweep intervals, recent roots |
| `roots/<hash>.json` | `app_config_dir` | Per root: pins, last selected repo |
| `cache/<hash>.json` | `app_cache_dir` | Per root: branch, dirty, ahead/behind, `last_fetch_at` |

`<hash>` is a short hash of the canonicalised root path. **Cache and pins must be per-root**
— a single shared `cache.json` would be rewritten wholesale every time *Open Folder…*
changed root, so the previous root's statuses would be gone by the time you went back to
it, and its pins with them.

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
`--lane-1..8`, `--diff-add-bg`/`--diff-del-bg` and their gutter and filler variants for
§5.4). **No hex literals in component styles.** Adding a light theme then means
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

Rule 3 has a corollary that the changed-files badge (§5.1) was the first thing to find:
**a quantity is not a state, and takes no hue.** Every colour in the palette is spoken for
— amber dirty, orange behind, green ahead, red failed, indigo the accent — so a filled
badge tried in any of them inherits a meaning it does not have. Amber read as a warning
about a repo that was merely edited; blue read as the accent whatever its hue angle, because
at a glance down 77 rows "saturated and cool" *is* the accent. It fills with `--count-bg`,
a neutral one step lighter than `--bg-active`, and inks with `--count-text`. The state is
already carried by the badge existing at all; the colour was saying it a second time.

This is the rule for any counter added later. Status colours mean "this repo is in state X";
neutral means "here is a number".

One token is an exception to rule 3's "one accent, status colours are semantic" framing:
`--titlebar-close-hover` (`#c42b1c`) is Windows' own close-button red, and it exists
because the caption is drawn by us now (§4.1). It sits close to `--status-error` and is
kept separate on purpose — that token means "a git operation failed", this one means
"this is the close button", and the two must be free to move apart. It is the one place
the app defers to the OS palette instead of its own, because the muscle memory being
served was trained by every other window on the desktop.

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
deferred (§12) · one dirty badge carrying a changed-files count (§5.1) · row-level Pull · close quits · one root, one window,
one process, with a recent list (§9) · absolute `dd-MM-yyyy HH:mm:ss` timestamps · native
menu bar (§4.1) · window restores the last selected repo · multi-window deferred to v2
(§2, §9.2).

· scan depth 1 (§8.1) · *Open Folder…* replaces the current root.

Still open, none blocking:
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
4. **Middle pane** — staging, commit
5. **Remotes** — push, publish branch, pull, background fetch sweep, ahead/behind
6. **Graph** — log parsing, lane layout, virtualized rows, Uncommitted Changes node
7. **Commit info panel** — commit details (built as a middle-pane mode, moved to its own column; §5.2)
8. **Branch switching + conflict detection**
9. **Polish** — pins, filter, watchers on hot repos, context menus, error translation,
   native menu bar, single-instance enforcement + recent roots
10. **Ship** — GitHub Releases, Tauri updater keypair, auto-update wired end to end
    (Authenticode deferred per §12)
11. **Diff view** (§5.4) — added after the first ten were laid out, and numbered last
    rather than slotted in beside step 7 so that the "build step N" citations already in
    the code keep pointing at the same steps they were written against

Measure against the §1 budget at step 3 and again at step 6. If cold start exceeds 500 ms,
stop and fix it — that number is the reason this project exists.
