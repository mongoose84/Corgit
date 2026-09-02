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

**And nothing on the startup path runs on the main thread.** That thread runs the event
loop, so work on it is not slow painting, it is no window at all — the window is created
before the setup hook but cannot be composited by a thread that is not pumping messages.
A `#[tauri::command]` without `(async)` is dispatched there, which is what `open_root`,
`refresh_root` and `initial_root` all used to do: a directory read plus two stats per
child, a cache file, and a subtree watch handle per repo, all in front of the first frame.
Warm that is ~120 ms and invisible; cold, with every open going through a filter driver,
it is the whole of "Corgit took ages to open". The rule generalises past startup: a
command that touches the filesystem or spawns a process takes `(async)`, including the
ones that only write a small file (`save_settings`, `toggle_pin`, `set_selected_repo`) —
`%APPDATA%` is redirectable to a network share by policy, and a freeze on every click in
the repo list is a worse failure than a slow save. The exception is `menu_command`, which
closes windows and exits the app: that work *is* the main thread's.

---

## 2. Scope

### In (v1)

- Repo discovery by scanning configured root folders
- Per-repo status: branch, changed-files count, ahead/behind
- Stage / unstage files (file-level)
- Add untracked files to `.gitignore`, or delete them from disk, from the file list's
  context menu (§5.2)
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

**"At startup" means started at startup, not waited for.** The probe is a process spawn,
and process spawn is the one startup cost with no upper bound worth trusting: an
anti-malware scan of a rarely-run binary, a revocation check with no route to its
responder. Blocking the setup hook on it — which this originally did — spent that wait
with the event loop stopped, so there was no window on screen to spend it in, and hitting
the 5 s probe timeout produced five seconds of nothing followed by the *wrong* screen,
since a git that is slow to answer is not a git that is absent. It is now warmed into a
cell that `git_info` awaits; nothing else on the startup path depends on the answer, and
the frontend starts optimistic so the welcome screen draws without it.

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
| **Repository** | *Root scope:* Fetch All · Pull All Behind · Rescan Folder — then a separator — *selected repo:* Fetch · Pull · Push. See below. |
| **Help** | About · Check for Updates · Recent Problems… · Open Log Folder · Reset Dismissed Warnings |

**Repository carries two scopes, and the separator is what says so.** The lower group acts on
the selected repo, mirroring *Changes*' two icons and its Push button for discoverability;
disabled when no repo is selected, and Push reads *Publish Branch* on a branch with no upstream
(§8.7). The upper group acts on the whole root, mirroring the repo list's header and strip
(§5.1) — the same rule, applied to a pane whose subject happens to be all 77 repositories.

*Pull All Behind* names its scope in the label rather than relying on the menu's position: it
pulls the repos that are behind, not all 77, and carries the count the strip shows. *Rescan
Folder* is the `⟳` that left the repo list's header, and it sits with the root group rather than
under **File** because the group is now "acts on every repository" and rediscovery is exactly
that — even though it is the one item here that runs no git at all.

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

**Root actions.** The pane header acts on the pane's subject, which here is the whole root —
the same rule that puts the selected repo's Fetch and Pull in *Changes* (§5.2) and nowhere
else. Two controls, and the split between them is by weight:

- **Fetch all** is a hover-revealed icon in the header, beside the sweep-timing readout.
  Fetch is safe, silent and idempotent, so it earns an icon and nothing more. It is the only
  icon in this header — see *Rescan Folder* below for why.
- **Pull all** is a strip **between the header and the filter box**, always reserved. Actions
  on top, list manipulation below: everything under the filter is what the filter scopes, and
  the strip is visibly outside it.

The strip is **reserved, never inserted**, and this is the whole reason it is not conditional.
An appearing strip would be inserted by the fetch sweep — on its own, unprompted — putting a
button that writes to every behind working tree exactly where a repo row was a moment ago.
Reserving it removes that at the source: nothing moves, so nothing can be mis-clicked, and no
pointer-deferral rule is needed to make it safe. It is the same instinct as *the list never
reorders itself* below.

Its two states, and neither is dead chrome:

| | Left | Right |
| --- | --- | --- |
| Something behind | `↓ 7 behind` | **Pull all**, accented |
| Nothing behind | `All 77 in sync` | *Pull all*, disabled |

The disabled state needs no new styling: `button.primary:not(:disabled)` is what carries the
accent (§5.2), so a disabled Pull all goes neutral by itself and the accent stays reserved for
when there is something to do. The greyed line is a **root status readout** — "is the whole
herd in sync?" is the question this app exists to answer, and answering it costs nothing extra
once the row is reserved.

**The strip ignores the filter.** The count is always the whole root, and *Pull all* always
pulls every behind repo, whatever the box is showing. This deliberately does **not** follow
*Unpin all* above, and the difference is disclosure rather than principle: *Unpin all* would
silently unpin repos the user cannot see, whereas the strip prints its number before it is
pressed. **The count is the consent.** It also keeps one stable answer to the sync question
instead of one that changes as you type.

It costs **34px permanently** — one repo row, in the pane whose whole job is fitting rows on
screen (§14.1). Spent on purpose: the row carries live root state in both of its states, which
is the bar §14.1 actually sets. If it is ever reclaimed, the fallback is the filter row, which
costs no vertical pixels and squeezes the filter box instead.

**Rescan Folder leaves the header** for `Repository ▸` (§4.1). Two circular arrows side by side
— `↻` fetch and `⟳` refresh — cannot be told apart, and only one of them belongs in a header
now. `⟳`'s unique job is *discovery*: finding a repo cloned or deleted since the folder was
opened. Its other half, re-reading status, is already covered by the watchers, the focus-gain
sweep and the every-fifth-tick full pass (§6), which makes a manual status refresh a button for
a case that mostly no longer exists. Discovery is rare enough for a menu.

A **filter box** sits between them. Typing filters both sections by substring on **repo name
only** — not branch, not path. This is the primary navigation tool for 77 repos; it is not
optional.

A **comma-separated value matches any of its terms**: `billing-worker, identity` shows both
rows. Still name-substring only — the rule above is unchanged, only made plural — and this
is what lets §13's bulk-run banner hand the user its failures by writing them into a box
they already understand (see *Root actions* above). It is independently the cheapest way to
put two projects side by side.

**The selected row is scrolled back into view whenever it moves.** Pinning and unpinning
move a repo between the two sections, and over 77 rows the new position is usually outside
the viewport — the selection is untouched, but the only thing on screen showing it has
gone, which reads as though pinning cleared it. Scroll it to the nearest edge, never to
the centre: a row already visible must not jump, so clicking a row you can see stays a
no-op. The same rule restores the startup selection (§9.5) into view.

**Row contents:** repo name · current branch · changed-files badge · ahead/behind badge.

A row whose repo has a write running shows it in the **pin gutter** (§13, *Work in
progress*) — a slot already reserved at a fixed width and already hidden and revealed, so the
indicator costs no layout and never squeezes the badge strip. It takes the pin's place for
the duration: which repos are pinned is not a question anyone is asking mid-write. Not the
mascot, which docs/mascot.md §2 keeps out of repository rows.

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
  row must be able to carry an **error badge** (click → selects the repo and raises its
  banner — §13; the badge points at the error rather than rendering a second copy of it),
  and the merge-conflict state must be renderable **on a row**, not only in the middle pane.

No fetch *button* on the row — fetch is automatic (§6), and a second hover control beside
Pull would undo the restraint above. It is on the row's **context menu**, and has to be: the
fetch sweep skips a repo once its fetch fails on auth (§8.7), so a manual fetch is the only
thing that clears an auth-needed badge, and the entry reads *Fetch now* on such a row because
retrying the fetch and making the badge go away are the same act (§13).

No row-level commit or push in v1; those need a message or a diff review, which means the
middle pane.

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
- **↑/↓ walk the rows and bring each one's diff up as they go.** Clicking the first file
  and then arrowing down is how a change set gets read before it is committed, and a click
  per file for that is the pane's second-most ordinary job. Three details:
  - **Both sections are one list**, staged first, in the order they are drawn. The user
    pressing ↓ is reading down the pane, not reasoning about which list a file is in. The
    one-section rule below is untouched: a step lands on a single row and selects it
    exactly as a plain click would.
  - **The step is measured from the far edge of the selection in the direction of travel**,
    so ↓ out of a shift-range steps past it rather than back into the middle of it. The
    ends **clamp rather than wrap** — ↓ on the last file jumping to the top would read as a
    scroll bug — and the key is then left unclaimed so the pane scrolls instead.
  - **A held arrow does not spawn a `git diff` per row it flies past.** The highlight moves
    with every repeat; the diff is read once the key settles. On this platform the spawn
    costs more than the diff does (§1), and thirty reads to show the user one is the
    ctrl-click-opens-a-diff mistake in a different key.
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
  it); *Delete N files…* for whatever untracked rows the selection holds, as its own entry
  under its own verb; *Reveal in File Explorer* on a single row only, because `explorer
  /select,` takes
  one path and N files would mean N windows rather than one window with them all picked
  out. A file that no longer exists — a `D` row — reveals the nearest folder that does,
  never a silent jump to Documents. Then, below a separator, the **ignore** entries — see
  *Ignoring a file* below.
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
  - **Never on an untracked (`?`) row** — no `↺`, and no *Discard* entry. Git has nothing to
    restore an untracked file *from*, so there is no "discard" of one to offer. Removing it
    is a different act under a different word — see *Deleting an untracked file* below.
  - **Always confirmed**, by a modal listing every path and saying what goes and what
    stays. §8.3 refuses force-checkout because it "silently discards work"; this is the
    same act done loudly. `git revert` and `git reset` stay out of v1 (§2) — this is
    neither.

**Deleting an untracked file.** A `?` row's context menu offers *Delete N files…*, which
removes those files from disk. This is the only thing Corgit does that **git cannot undo at
all**: a discarded change came out of the index and an abandoned commit is still in the
reflog, but an untracked file has never been in the index, so no object git holds has a copy
of it.

Earlier revisions of this spec said flatly that *Corgit does not delete files*. That was
reversed deliberately, not eroded: with `-uall` listing every untracked file individually,
a pane that could stage and ignore them but never remove one sent the user to a terminal for
the third of the three obvious verbs. The rules below are what the old blanket ban is
traded for, and each of them is load-bearing:

- **The word is *Delete*, never *Discard*.** They sit next to each other in the same menu on
  a mixed selection, and one is reversible while the other is not. Sharing a verb would make
  the safe one teach the wrong lesson about the dangerous one.
- **Two entries, never one merged entry.** A selection spanning both kinds gets *Discard
  changes to N files…* and *Delete N files…*, each scoped to the rows it applies to. One
  entry doing two different irreversible things to two halves of a list cannot be confirmed
  honestly, because the dialog would have to describe both.
- **Untracked rows only**, which is also what keeps this away from tracked work entirely.
- **`git clean`, never a filesystem unlink** (§8.6). Clean removes only what git considers
  untracked, so a tracked path arriving through a bug is skipped rather than deleted — the
  frontend filter is then the *second* guard rather than the only one.
- **Confirmed by the same modal** as Discard, in its delete wording: every path listed,
  Cancel focused, no scrim-click dismissal, and a sentence that says the file has never been
  committed and nothing can bring it back.
- **No row button.** `↺` stays absent on `?` rows and gains no `×` counterpart. Discard's
  hover button is defensible because git can undo it; a one-click permanent delete on a row
  the pointer merely passes over is not, and the menu is a deliberate enough act to carry
  this on its own.

**Ignoring a file.** A `?` row's context menu can append a pattern to the repo's root
`.gitignore`, below a separator — the point at which the menu stops being about these files
and starts being about what git sees at all:

```
Changes (312)
  ?  index.js   node_modules/react       [+]   ← right-clicked

              ┌──────────────────────────────────┐
              │ Stage 1 file                     │
              │ Delete 1 file…                   │
              │ Reveal in File Explorer          │
              │ ──────────────────────────────   │
              │ Ignore index.js                  │  → /node_modules/react/index.js
              │ Ignore *.js                      │  → *.js
              │ Ignore node_modules/react/       │  → /node_modules/react/
              │ Ignore node_modules/             │  → /node_modules/
              └──────────────────────────────────┘
```

- **Untracked rows only**, which also means *Changes* only. A `.gitignore` line for a
  tracked file does nothing at all — git keeps tracking what it already tracks — so the row
  would sit exactly where it was while the entry that produced it reported success. That is
  the same class of lie as a Discard that silently ate staged work. A mixed selection drops
  its tracked rows and says so, the way Discard does with untracked ones, with the filter
  running the other way round.
- **Four entries for one row, narrowest first**, so the broadest thing the menu can do is
  never the first thing under the pointer. The two folder entries are the parent and the
  top-level folder, and the second appears only when it is a different directory. Nothing
  between them is offered: a line per path segment would be a folder picker, and the two
  ends are the two questions anyone has.
- **The top-level entry is the one that answers `-uall`.** "Never a folder row" above means
  a wholly-untracked `node_modules` arrives as several hundred file rows, whose immediate
  parents are `node_modules/react/`, `node_modules/lodash/` and so on — ignoring those one
  at a time is not a feature. It is offered *as well as* the parent rather than instead of
  it, because the same shape reaches `src/generated/out.js`, where the broad reading would
  ignore the whole source tree. Both are on the menu, both state in full what they cover,
  and the narrow one is on top.
- **A selection of several rows collapses to one entry**, *Ignore N files*, writing one
  exact path each. There is no honest single extension or folder for six files. The rich
  form needs a selection that *is* one row, not one that merely has one untracked row left
  in it — otherwise the menu would describe rows the user can see are picked and it is not
  acting on.
- **Every pattern is anchored** with a leading `/` except the extension one, which is
  deliberately repo-wide. The anchor pays for itself twice: it means the row that was
  clicked rather than any file of that name at any depth, and it keeps a file named
  `#notes.txt` or `!notes.txt` from producing a line that is a comment or a negation.
  Paths are escaped for git's matcher — `logs[1].txt` written verbatim is a character class
  matching nothing — and the menu label and the appended line come from one function so
  they cannot drift.
- **Append only, and never staged.** The file is created if absent, a pattern already in it
  is skipped rather than duplicated, and its existing comments, grouping and line endings
  survive untouched: a `.gitignore` is written by hand and Corgit rewriting one is not
  recoverable from the UI that did it. **No confirmation** — unlike Discard this destroys
  nothing, the file stays on disk and merely stops being listed, and the `.gitignore` edit
  lands in *Changes* as an ordinary row that can be read, discarded or committed. That row
  is the confirmation, after the fact and reversible. Staging it automatically would be a
  second act the entry never named, and `+` is right there on it.

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

**Opening it is a deliberate act, and selecting a row is not one.** *Show info* on a row's
context menu is the only way in. Selection used to be: clicking any commit opened the
column, which meant reading the graph — the ordinary thing to do in that pane — cost a
320 px reflow every click, and shelled out to `git show` for a commit the user was only
scrolling past. Right-click ▸ Show info separates "I am looking at the graph" from "tell
me about this one".

Every row's menu carries *Show info*, including rows with no ref badges on them, which before
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

Selected repo only — one repo at a time, so graph cost never multiplies by 77. **The
header names it**, right-aligned opposite the tab strip and in the repo's own case, so the
pane says what it is showing without the left pane having to. It is not inside the tab
strip: that is a tablist, and a name sitting beside the tabs would read as a third tab
whenever a diff is open.

- **Synthetic "Uncommitted Changes" node** pinned at the top when the working tree is dirty.
  Clicking it selects the working tree, which also closes the info panel (§5.2). This is
  what ties the panes into one coherent surface.
- Rows: graph lanes · ref badges (branches, tags, `origin/*`) · message · author · date.
  **No hash column**: the short oid is a lookup key, not something read while scanning
  history, and it cost 56 px on every row to say what the info panel (§5.2), one context-menu
  entry away, already answers on demand. The badges come **before** the message, not after
  it: trailing a variable-length subject puts each branch name at a different x, so
  answering "where is `main`" means reading every row. Anchored right after the lanes they
  line up in a column next to the dots they name, which is also the pairing that makes a
  badge and its dot read as one thing.
- **The author column is right-aligned**, the one text column that is. Left-aligned in its
  fixed 110 px box a short name leaves most of the box empty, and the date reads as adrift
  from the row rather than as the pair it forms with the name.
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
  open on selection** (§5.2); *Show info* on the row's context menu opens it.
- Right-click a commit → **Show info** first, then the branch entries for any ref badges the
  row carries (§8.3). Copy hash, Copy message and Open in VS Code belong here too. (Thin by
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
  closes it. A commit's diff is immutable and is never re-read.
- **The tab lives as long as its row.** A live diff is a view of a middle-pane row (§5.2),
  so discarding or committing the open file closes the tab, and staging or unstaging it
  re-points the tab at the section the row moved to rather than closing it. Neither case is
  covered by the re-read below: git compares the two sides of a file that is no longer
  there, finds them identical, and correctly reports nothing — which leaves the tab
  selected over an empty diff. Keeping the last content instead is worse, being a
  working-tree diff that no longer describes the working tree with nothing on screen saying
  so. **A truncated file list decides nothing**: the lists stop at 100 rows per section
  (§5.2), a missing path there is not a missing file, and closing a tab someone is reading
  on a guess is the worse of the two mistakes.
- **An open working-tree diff is live.** Anything that moves the file re-reads it — our own
  writes (stage, unstage, commit), an editor saving over it, a terminal `git checkout`.
  This costs nothing extra to arrange: the diff re-reads on the per-repo status event, so
  every path that publishes one (§6) already feeds it. What it *does* require is that those
  paths never lose a change, which is the reason for both the trailing-edge coalesce and
  the selected-repo republish below — a diff is the one view where a missed refresh is not
  merely late, because nothing else on screen will contradict it.

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
   arrive. A minute-long build costs that repo ≤30 reads, not thousands. **Deferred, not
   dropped:** an event turned away inside the window schedules the read for the end of it
   instead. Leading-edge-only coalescing is what turns "late" into "wrong" — the last write
   of a burst is the one describing the file as it now stands, and it is the write most
   likely to arrive inside its predecessor's window, so dropping it leaves an open diff
   (§5.4) showing the previous save indefinitely.
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

**A sweep that covered the selected repo republishes it in full.** The sweep's own event
carries counts, which is all the rows need; the middle pane's file list and the open diff
are fed by the per-repo status event instead, and a change that moves no count — editing a
file already counted as modified — is invisible to everything except them. So the pass ends
with one more read of that one repo. This is what makes a change reach the diff at all when
there were no watchers to notice it: they are dropped on blur, so every edit made in an
editor while Corgit is in the background arrives on the focus-gain sweep and nowhere else.

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
17 ms, and the focus-gain sweep already covers whatever changed while they were gone —
rows from the sweep itself, the selected repo's files and open diff from the republish
above.

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
4. **A bulk run takes at most 4 of those 8.** *Pull All Behind* and *Fetch All* (§5.1) are one
   click that queues dozens of writes; uncapped they hold every slot and the app stops
   answering — the status sweep, the graph, whatever the user clicks next, all behind a run
   they cannot see the end of. Leaving half the global budget free is what keeps the window
   usable while the herd comes down. The number is not new: the fetch sweep already runs at 4
   (§6) for the same reason, and a pull is that work with a merge on the end, holding its slot
   longer. Headroom rather than a priority queue — the simplest thing that keeps the promise.
5. Never touch `index.lock` directly. If git reports a lock error, surface it — the user is
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
git branch -d <branch>                           # delete, refuses if unmerged
git branch -D <branch>                           # delete anyway, only after -d refused
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

**A switch onto a behind branch offers the pull** (Git Graph's gesture). When a switch
lands and the branch now checked out is behind its upstream, a small modal asks — *Pull
`<branch>`?*, naming the count and the upstream, with **Pull** and **Not now**. A checkout
is nearly always the start of working on that branch, and "3 behind `origin/main`" is
knowable at exactly that moment and forgotten one second later; the alternative Corgit
already had is switch, notice the ↓ badge, cross the window to Pull — the trip §5.1 exists
to remove.

The question is asked of the *status the switch itself produced*, never of the badge that
was double-clicked: switching to a remote-tracking badge checks out a local branch of a
different name, and one whose local counterpart already exists lands somewhere else again.
So it waits for the post-switch status refresh and asks about whatever HEAD is on. A
refused checkout asks nothing — HEAD did not move.

This is an offer, not a failure, so §13's *Don't show this again* does not apply and there
is no checkbox: it appears only in response to a gesture the user just made, and *Not now*
is an answer rather than a dismissal. Pull is the ordinary `git pull --no-rebase` of §8.7,
failing and reporting like any other write. The count comes from the last fetch and may be
stale (§5.1) — the modal says so rather than pretending otherwise.

**A switch that takes time has to say so.** Checking out a branch that differs by thousands
of files is seconds of work, and the gesture that starts it — a double-click on a ref badge
— leaves no trace of itself: nothing on screen changes until the checkout lands, so a silent
pane reads as a double-click that missed rather than as work in progress. See §13's *Work in
progress*, which covers every write. Switching is only the one reliably slow enough to make
the absence obvious.

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

**Deleting a branch** (same menu): right-click a *local* ref badge in the graph → *Delete
`<ref>`*. Local badges only — a remote badge names a branch on the server, and removing
that is `push --delete`, a network write with a different blast radius that is not folded
into the same entry. The badge for the checked-out branch does not offer it either: git
refuses to delete the branch HEAD is on, and an entry that can only fail is not an entry.

The confirmation is **two-stepped, in one dialog**, and this is where §8.3's
"never force-checkout" rule is honoured rather than broken. The first press always runs
`git branch -d`; *Delete anyway* (`-D`) appears only after git has refused, with git's own
"not fully merged" text above it. So the destructive button is never the one on screen when
the dialog opens, and the reason for it is git's rather than a warning Corgit guessed at in
advance. Forcing has to be reachable at all because a squash-merged branch is unmerged to
git forever — which is the most common branch a user wants to delete.

Everything else — a delete that fails for any other reason — closes the dialog and surfaces
as an ordinary write error (§13), exactly like a failed switch or merge.

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
git clean --force -- :(literal)<paths>   # delete untracked files, §5.2
git commit -F -            # message via stdin, avoids arg-escaping pain
```

`clean` carries **no `-d`, no `-x`, no `-X`**, and those absences are a named constant with
a test on them in `commit.rs`, exactly as the two `restore`s are. `-d` would recurse into
directories that the pane never shows a row for; `-x`/`-X` would reach ignored files, which
turns a confirmed two-row delete into a sweep that takes `node_modules` and every build
artefact on the disk with it. None of the three changes anything visible before it happens.

**`:(literal)` on every path is the other half.** Git reads a pathspec as a glob, so a file
honestly named `report[1].txt` or `draft*.md` is a *pattern* — and for a delete that is the
difference between removing the row the user confirmed and removing everything beside it.
The prefix disables pathspec magic for that entry, and incidentally stops a leading `:` in a
filename from parsing as a directive.

The two `restore`s are one flag apart and opposite in which half they keep, so the flags
are a named constant in `commit.rs` with a test on them. `--staged --worktree` together
would be a third thing again — it moves the source to HEAD and destroys both halves — and
is what §5.2's Discard must never become.

**Ignore has no command here, and that is not an oversight.** There is no `git ignore`;
`git check-ignore` only asks. §5.2's ignore entries append to a text file, which makes
`ignore.rs` the one write in the app that spawns nothing — and the one whose *existing*
contents must be preserved rather than merged by git. It still takes the repo's write-queue
lock (§7 rule 1): a read-modify-write of a file two windows can reach is the same race as
two `git add`s, and it still publishes status afterwards, which is what makes the ignored
rows leave the pane.

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

The corollary, which the first implementation got wrong: **how loudly a failure is shown is
decided by what the user must do about it, not by which pane happened to run the command.**
That version grew four presentations of the same git error — a popover behind the row's `!`
badge, an inline notice in the compose pane, another above the graph, and a bespoke conflict
banner — and which one you got depended on whether you pushed from the row or from the
button. Worse, two of them lived in panes whose minimums are 240px and 190px (§4), so a
headline, an action and *Details* never fit on one line; the notice ended up shaped around
the shortage rather than around the message.

### Three tiers, three surfaces

| Tier | Means | Surface |
| --- | --- | --- |
| **Warning** | Nothing is broken; something didn't happen | Row badge only |
| **Error** | An operation you asked for failed; the repo is unchanged | Banner, dismissible |
| **Blocking** | The repo is in a state git will not leave on its own | Banner, not dismissible |

**The banner is app chrome: full window width, directly under the title bar (§4.1), and it
always names the repo it is about.** Full width is not a style preference — it is the only
place in the layout wide enough to hold a headline, an action and *Details* on one line,
which is precisely what the pane-local notices could not do. One line is the target and not a guarantee: when the controls stop fitting the row wraps and they take a second line. The alternative is worse than a two-line banner — a row that simply refuses to fit is clipped by `body { overflow: hidden }`, and the rightmost control is *Dismiss*. Naming the repo is load-bearing
for a different reason: row-level Pull (§5.1) can fail in a repo that is not selected, so a
banner reading only "Remote has commits you don't have" is ambiguous across a 77-row list.

**An error report is never modal.** The failure has already happened; freezing a dashboard
over *many* repositories to announce that one of them failed to push is punishment, not
help. Modals stay reserved for what they already do — `DiscardDialog`, `DeleteBranchDialog` —
**confirming an irreversible act before it happens.** So a merge conflict is a
non-dismissible banner offering **Abort merge…**, and that ellipsis leads to a modal which
confirms the discard. The report is a banner; the *recovery* may be a modal.

### Work in progress

§13's rule has a mirror image: **never leave the user unable to tell whether Corgit heard
them.** Every write can take long enough to need saying so — a checkout across thousands of
files, a pull over a slow link, a push of a large history — and none of them changes anything
on screen until it lands.

In-progress is **not a fourth tier, and it never uses the banner.** The banner reports
something that already happened, and it is chrome the full width of the window; borrowing it
for "this is happening" would put a full-width bar on screen for every stage and unstage —
the four-pane-local-notices mistake above, run in reverse. Nor is it ever a modal: those stay
reserved for confirming an irreversible act before it happens.

Two things are shown, and they answer different questions:

| | Where | When |
| --- | --- | --- |
| **Acknowledgement** | The control that was used — the ref badge, the row's Pull chevron | Immediately |
| **Narration** | The repo row, and the pane the operation was started from | After a delay |

Splitting them is the point. "Did it hear me" and "how long will this take" are different
complaints, and the first one is answered by the thing under the pointer changing in the same
frame as the click. Delaying *that* is what produced the original bug.

Rules that follow:

- **Narration is the backend's signal, not the pane's.** Rust owns which repos have a write
  in flight (§7, §9.2) and publishes it per repo, so a write on a repo that is not selected —
  row-level Pull (§5.1) — still marks its row, and a second window is not silent while the
  first is busy. A pane-local flag structurally cannot do either.
- **The window covers the whole wait**: from the moment the command is accepted, including
  time queued behind an earlier write on that repo (§7 rule 1), until the status refresh and
  any view reload that follows have landed. An indicator that clears while the graph is still
  being rebuilt is worse than none — it says "done" over a view about to swap.
- **Don't paint a wait no one notices.** Reveal after ~150 ms, and once revealed hold for
  ~300 ms, so the common fast write leaves the list perfectly still and a write landing either
  side of the threshold does not flash.
- **It is a state, not an event**, so it is never dismissible (below). It goes when the write
  does.
- **No Cancel.** A checkout in progress cannot be stopped without leaving a half-written
  working tree, and a button that claims otherwise is worse than no button.

**In the graph**, the destination row additionally becomes *pending HEAD*. A switch is HEAD
moving from one commit to another and both ends are usually on screen, so the row HEAD is
still genuinely on keeps its full treatment while the destination takes a weaker copy of the
same one — the tint at 5% instead of 12%, a dashed ring where the filled halo goes, and the
dot left at its normal radius. Landing is then the row *finishing* rather than changing:
nothing appears, nothing switches meaning. A failed switch is the same three receding, which
is what makes a failure read as "that did not happen" rather than as an undo of something
that did.

What the graph must **not** do is dim or overlay itself during a switch, however common that
is elsewhere. A switch changes no commit on screen — only which badge is HEAD — so greying
the history claims the view is untrustworthy at the one moment it is entirely true. Pull and
merge do bring new commits, and the reload already covers those.

*Deliberately absent:* a percentage. `git switch --progress` will report `Updating files:
47% (2823/6000)` on stderr without a terminal, so it is available — but collecting it means
streaming stderr rather than reading it at the end, and filtering those lines back out before
the raw text is retained for *Details*. Worth revisiting only if measurement shows switches
are routinely multi-second; not worth an animation asserting precision Corgit has not
measured.

### Merge conflict

`pull` is `fetch` + `merge`, and merge is precisely what produces conflicts. Detect via
`u` records in `status --porcelain=v2`, or the presence of `.git/MERGE_HEAD`.

It is the archetypal **blocking** failure, and it is handled by the ordinary banner rather
than by a component of its own:
- A blocking banner *and* a conflict marker on the repo row — row-level Pull (§5.1) means a
  conflict can be created in a repo that isn't selected, so the row must be able to show it
- Exactly two buttons: **Abort merge** (`git merge --abort`) and **Open in VS Code**
- **Block commit and push** for that repo until resolved or aborted

Blocking is the one tier with no *Dismiss*. The banner is a rendering of the condition, so it
goes when the condition does and not before — a dismiss button here could only produce a UI
that disagrees with the repository until the next sweep repaints it.

### Dismissal: events can be dismissed, states cannot

An indicator that renders **current repo state** must not be dismissible; one that records **a
past event** may be. Dismissing a state is a button that makes the UI lie for up to one sweep
interval (§5.1) and then loses the argument anyway. Sorting what exists:

| Indicator | Backed by | Dismissible |
| --- | --- | --- |
| `!` row error | `rowErrors[id]` — a row Fetch/Pull that failed | **Yes** — frontend-owned; nothing regenerates it |
| `⚿` auth needed | the `auth_needed` set | No — it is a *Fetch now* (below) |
| `!` status error | `errors[id]` from the sweep | No — republished every sweep |
| `⚠` conflict | `status.conflicted > 0` | No — derived state |
| counts, ↑/↓ | derived | No — not notifications |
| spinner, in progress | a write in flight, from the backend | No — derived state |

`⚿` looks like the second dismissible case and is not. `auth_needed` is a **scheduling** flag,
not a display flag: the fetch sweep filters those repos out (§6, §8.7) until a manual fetch
clears it. "Dismissing" it therefore re-arms background fetching, which walks into the same
auth wall and re-badges within one fetch sweep. That is a retry wearing the wrong label, so
the row's menu item says **Fetch now** and does what it says.

The implementation constraint behind the table: `errors` and `auth_needed` are replaced
wholesale from sweep events, so dismissing either would have to be a backend command or the
badge flickers back on the next tick. `rowErrors` is the only one the frontend owns outright
— which is exactly the one the rule permits dismissing.

### Suppression: "don't show this again"

Some failures are routine rather than interesting. Living in a terminal beside Corgit makes
`index.lock` collisions a daily event, and a banner for each is friction carrying no
information. So an error banner may offer a **Don't show this again** checkbox — under one
rule:

> **A notification may be suppressed. A condition may never be.**

- **Warnings** and **errors** are suppressible. The row badge survives regardless, so the
  state stays discoverable; only the banner is silenced.
- **Blocking** is never suppressible, and renders no checkbox at all.

Keyed by **rule id, not by stderr text** — the translation table below gains an id per row.
That makes unmatched stderr unsuppressible for free: a failure Corgit has no rule for has no
id, so it cannot be silenced and the checkbox is simply not drawn. Falling out of the design
beats being enforced by a rule.

Suppressions live in settings (§9.5) as a set of rule ids, and **must be escapable**: *Help ▸
Reset Dismissed Warnings*. A suppression with no way back is a trap, and the user who set it
is by definition no longer seeing the thing that would remind them.

### Other failures

Translate the common cases into plain language, with raw stderr always available in a
collapsible "Details":

| id | Case | Message | Tier | Action |
| --- | --- | --- | --- | --- |
| `non-fast-forward` | Push rejected | "Remote has commits you don't have" | Error | Pull |
| `dirty-tree` | Pull with dirty tree | "Commit or discard your changes first" | Error | Open in VS Code |
| `merge-conflict` | Merge stopped in conflict | "Merge stopped with conflicts" | Blocking | Abort merge… · Open in VS Code |
| `index-lock` | `index.lock` present | "Another git process is running" | Error | Retry |
| `timed-out` | Corgit killed a wedged git (§7.3) | "Git stopped responding, so Corgit cancelled it" | Error | Retry |
| — | Checkout blocked by local changes | git's own stderr | Error | Open in VS Code |
| — | Auth failure (background) | Repo badged "auth needed" | Warning | Fetch now (may prompt) |
| — | No upstream | Button becomes "Publish branch" | — | — |

Checkout-blocked keeps git's own stderr rather than a translation, and so has no id and no
suppression — the paths git names are the useful part of that message.

### The log, and Recent Problems

`corgit.log` and *Help ▸ Open Log Folder* (§4.1) already exist, and recorded the wrong half of
the story: Corgit's own plumbing — a settings write that failed, repos no watcher would take,
a git process killed at its budget — but **not one failing git command.** A non-zero exit is a
successful *call* at the process layer (`git.rs` returns `Output { ok: false, stderr }`), so it
went to the UI and nowhere else. The one thing you would open a log to investigate was the one
thing not in it.

Two changes, which are one feature seen from both ends:

1. **Every non-`ok` git invocation is logged** at `warn` — repo, args, stderr — at the single
   chokepoint that sees them all. This is what makes the existing menu item worth clicking.
2. **Recent Problems**, a window onto an in-memory ring of the last ~50 such records, so the
   app can show what the log holds without parsing the file back.

This is also the precondition for everything above. Dismissal and suppression both throw
information away, and the sweeps fail while nobody is watching; both are only defensible
because nothing they hide is actually lost. Build the log first.

Stderr is logged **verbatim**, matching the rule that errors keep their raw text. It can carry
remote URLs and the occasional credential-helper line, so the log folder is diagnostic output
— worth a glance before pasting into a bug report, and not worth scrubbing at the point of
capture, where scrubbing would mostly delete the detail that made it useful.

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
| Failures | the error banner — the dog softens git's worst moments (§13) |
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

- [x] **Should the status sweep emit incrementally rather than in one batch?**
      **Batch, minus its stragglers.** Cache-first paint did solve the case
      this was raised for — rows arrive filled in from disk and no longer wait
      branch-less — but it did not solve the case where a *stale* row is the
      wrong one: the batch lands when its slowest repo does, so one repo taking
      the whole 30 s `READ_TIMEOUT` on a cold cache held all 69 rows and the
      sweeping indicator for 30 s after every launch. Measured on the 69-repo
      root, first launch of the day, repeatably one repo.

      So `collect()` waits `SWEEP_PATIENCE` (3 s — above a healthy full pass at
      ~1.2 s, far below a killed read) and publishes what it has. Repos still
      reading keep their process and their read guard, and publish themselves
      through the per-repo `status:repo` event a watcher already uses (§6). The
      re-entrancy guard is held until they land, so §6's "a sweep never starts
      while one is in flight" holds and a straggler cannot be overtaken by the
      next tick's read of the same repo.

      This is not the 77-round-trip incremental sweep the question was about.
      The healthy case is still exactly one event; the extra ones are one per
      repo that was genuinely stuck, which is the number worth paying.

Still open: none.

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
