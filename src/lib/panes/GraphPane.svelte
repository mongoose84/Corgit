<script lang="ts">
  import { tick } from 'svelte';

  import Pane from './Pane.svelte';
  import GraphRow from './GraphRow.svelte';
  import DiffView from './DiffView.svelte';
  import EmptyState from '../EmptyState.svelte';
  import Glyph from '../Glyph.svelte';
  import Mascot from '../Mascot.svelte';
  import ContextMenu from '../ContextMenu.svelte';
  import CreateBranchDialog from '../CreateBranchDialog.svelte';
  import DeleteBranchDialog from '../DeleteBranchDialog.svelte';
  import PullAfterSwitchDialog from '../PullAfterSwitchDialog.svelte';
  import { repos, isDirty, canPull } from '../repos.svelte';
  import { graph, type RefBadge } from '../graph.svelte';
  import { isUnmergedBranchRefusal } from '../gitErrors';
  import { notices } from '../notices.svelte';
  import { diff } from '../diff.svelte';
  import { searchBranches } from '../branchSearch';
  import { formatCommitDate } from '../dateFormat';
  import {
    laneCount as computeLaneCount,
    laneColorVar,
    visibleWindow,
    ROW_HEIGHT,
    LANE_WIDTH,
  } from '../graphLayout';
  import { BusyIndicator } from '../busyIndicator';

  // Commit selection drives the middle pane's Mode B in build step 7 (§5.2);
  // until then, clicking a row only highlights it here.
  const hasRepo = $derived(repos.selectedId !== undefined);
  // The pane is about one repo and never said which. The repo list reorders
  // under pinning, so the row that answered the question can move — or leave
  // the viewport entirely — while the history on screen does not change at
  // all. Naming the subject here means the graph is self-describing whatever
  // the left pane is doing; RepoRow's scroll-into-view is the other half.
  const repoName = $derived(repos.selectedRepo?.name);
  const status = $derived(repos.selectedId ? repos.status(repos.selectedId) : undefined);
  const dirty = $derived(status !== undefined && isDirty(status));
  // Merging needs a destination, and Corgit's is always what is checked out.
  // On a detached HEAD there is no branch to merge into and no name to put in
  // the menu label, so the entry is simply not offered there.
  const currentBranch = $derived(status?.branch ?? null);

  // Guards the diff against this effect's own re-runs. The effect re-fires
  // whenever anything `graph.loadFor` touches changes, not only on a repo
  // change — harmless for the graph, since `loadFor` returns early, but a
  // bare `diff.close()` here would shut a diff the user had just opened.
  let lastRepoId: string | undefined = undefined;

  $effect(() => {
    const id = repos.selectedId;
    if (id) void graph.loadFor(id);
    else graph.clear();
    if (id !== lastRepoId) {
      lastRepoId = id;
      // The open diff names a file in the repo we just left; it may not even
      // exist in this one (§5.4).
      diff.close();
      // Same reasoning for the pull question: it is about a switch in the repo
      // being left, and `repos.pull()` writes to whichever repo is selected
      // *now*. The modal makes this all but unreachable by mouse — this is the
      // guard, not the mechanism.
      pendingPullCheck = null;
      pullPrompt = null;
      // The branch search does not survive a repo change (§5.3). Every part
      // of it is about the repo being left: the query was aimed at that
      // repo's branches, the cursor indexes a list that no longer exists, and
      // the out-of-reach line names a branch the new repo may not have. The
      // box reappearing over a different repo's history, already narrowed by
      // something typed about another one, is a search that has quietly
      // changed its subject — the same defect `diff.close()` two lines up
      // guards against for the open file.
      findOpen = false;
      query = '';
      cursor = 0;
      outOfReach = null;
    }
  });

  // The tab strip (§5.4). Only ever labelled by the filename — the directory
  // is in the diff's own header, where there is room for it.
  const fileName = $derived(diff.open ? (diff.open.path.split('/').pop() ?? diff.open.path) : '');

  const lanes = $derived(Math.max(1, computeLaneCount(graph.rows)));

  // Virtualized rows (§5.3): only the rows within the scrolled viewport (plus
  // a small overscan) exist in the DOM. Lane layout runs once over the whole
  // loaded set in graph.svelte.ts, and the window arithmetic lives in
  // `visibleWindow`; this only holds the two measurements they need.
  let scrollEl: HTMLElement | undefined = $state();
  let spacerEl: HTMLElement | undefined = $state();
  let scrollTop = $state(0);
  let viewportHeight = $state(0);

  const rowWindow = $derived(visibleWindow(graph.rows.length, scrollTop, viewportHeight));
  const visibleRows = $derived(graph.rows.slice(rowWindow.start, rowWindow.end));

  function onScroll() {
    if (scrollEl) scrollTop = scrollEl.scrollTop;
  }

  // Re-read the offset off the element whenever it is a *different* element,
  // because the scroll box does not survive a repo change: selecting a repo
  // empties `graph.rows` before the page lands, which swaps the whole
  // `.graph-body` out for the "Reading history…" state and back. The rebuilt
  // box starts at the top and fires no scroll event saying so, so `scrollTop`
  // kept whatever the *previous* repo had been scrolled to and the window was
  // translated that far down a graph the user is looking at the top of. The
  // clamp in `visibleWindow` bounds the damage to "the last few rows" instead
  // of blank, but only this stops it happening: nothing else corrects the
  // reading, and a repo whose history is too short to scroll never produces
  // the scroll event that would.
  $effect(() => {
    if (scrollEl) scrollTop = scrollEl.scrollTop;
  });

  // Back to the working tree, which also shuts the info column (§5.2) — there
  // is no commit left for it to be about. Two ways in, because the
  // *Uncommitted Changes* node is the semantic one but only exists
  // `{#if dirty}`, so on a clean repo clicking past the last row is the only
  // one there is.
  function deselect() {
    graph.select('working-tree');
  }

  // Only a click that landed on the scroll container or the virtualization
  // spacer, i.e. the empty space past the last row. Testing the target rather
  // than relying on rows to stop propagation keeps this independent of
  // GraphRow's internals — and of the "Load more" button, which lives in the
  // same scroll box.
  function onBackgroundClick(event: MouseEvent) {
    if (event.target === scrollEl || event.target === spacerEl) deselect();
  }

  // ── Branch search (§5.3) ──────────────────────────────────────────────
  //
  // Summoned, not standing: the pane loads 300 commits and the user is here to
  // read history, so the one control that is always on screen in the repo list
  // — its filter box — would cost 43px of the pane that wants vertical space
  // most, for a task that happens a few times a day. The header's icon button
  // and Ctrl+F are both ways in.
  //
  // What makes the feature possible at all is an asymmetry worth stating: the
  // rows on screen are one page of a long history, but `graph.refs` is *every*
  // ref `for-each-ref` returned, tip loaded or not. So the search can answer
  // instantly about a branch cut two years ago; it is only the jump that has
  // to go and read pages (`graph.reveal`).
  let findOpen = $state(false);
  let query = $state('');
  let findInputEl: HTMLInputElement | undefined = $state();

  /** Which hit the keyboard cursor is on. An index rather than a ref, because
   *  the list it indexes into is rebuilt on every keystroke and a held ref
   *  would survive its own disappearance from the results. */
  let cursor = $state(0);
  let resultsEl: HTMLElement | undefined = $state();

  /** Set when a jump gave up before finding its commit (§5.3's page cap), and
   *  cleared by the next query or the next jump. Answered here rather than
   *  through `notices.raise`: that store translates git's stderr for §13's
   *  banner, and nothing failed — a branch simply turned out to be further
   *  back than one gesture is allowed to walk. §11.1's container rule puts the
   *  answer where the question was asked. */
  let outOfReach = $state<string | null>(null);

  const hits = $derived(searchBranches(graph.refs, query));

  /** Clamped rather than stored clamped, so a shrinking result list can never
   *  point the cursor past the end between a keystroke and its `$derived`. */
  const cursorAt = $derived(hits.length === 0 ? 0 : cursor % hits.length);

  /** Which tips are already among the loaded rows — the one thing the search
   *  knows that the graph cannot show, and the difference between a jump that
   *  is instant and one that reads pages. Built only while the box is open:
   *  `graph.rows` changes on every sweep-triggered reload, and a Set over
   *  3000 rows rebuilt for a closed box is work nobody asked for. */
  const loadedTips = $derived.by(() => {
    if (!findOpen) return new Set<string>();
    return new Set(graph.rows.map((row) => row.commit.hash));
  });

  const results = $derived(
    hits.map((ref) => ({
      ref,
      loaded: loadedTips.has(ref.commit),
      date: formatCommitDate(ref.timestamp),
    })),
  );

  async function openFind() {
    findOpen = true;
    cursor = 0;
    outOfReach = null;
    await tick();
    // Select rather than merely focus: re-opening the box with a previous
    // query still in it should be typed over, not appended to. The one case
    // where keeping the text earns itself is coming back to the same hunt,
    // and that is still one keystroke away.
    findInputEl?.select();
  }

  function closeFind() {
    findOpen = false;
    outOfReach = null;
  }

  /** One jump at a time. `graph.reveal` walks pages sequentially, so a second
   *  jump started mid-walk would interleave two `loadMore` chains against one
   *  lane state — the exact aliasing `loadToken` exists to catch, arrived at
   *  from inside the pane instead.
   *
   *  A plain `let`, unlike the `busy` flag the switch gesture uses: nothing
   *  reads this from the template. What the user sees during a jump is
   *  `graph.revealing`, which the store owns because the pages are its. */
  let jumping = false;

  /**
   * Go to a branch: select its tip commit and scroll the graph to it.
   *
   * **Not a checkout.** The graph already has a switch gesture — double-click
   * a badge, or the row menu (§8.3) — and a search result that quietly wrote
   * to the working tree would be the one place in Corgit where finding
   * something changed it. Landing on the row leaves every one of those
   * gestures one click away, on a row that is now on screen.
   */
  async function jumpTo(ref: RefBadge) {
    if (jumping) return;
    jumping = true;
    outOfReach = null;
    try {
      const at = await graph.reveal(ref.commit, ref.name);
      if (at === -1) {
        // The box stays open, because the sentence needs somewhere to live and
        // because the next thing the user does is probably narrow the query.
        outOfReach = ref.name;
        return;
      }
      graph.select(ref.commit);
      findOpen = false;
      await scrollToRow(at);
    } finally {
      jumping = false;
    }
  }

  /** Centres a row in the viewport. `tick` first because a jump that had to
   *  read pages has just grown `graph.rows`, and the spacer is not tall enough
   *  to scroll that far until Svelte has applied it — without the wait the
   *  assignment is silently clamped to the old height. */
  async function scrollToRow(index: number) {
    await tick();
    if (!scrollEl) return;
    // `scrollEl.clientHeight` rather than the bound `viewportHeight`: closing
    // the find box in the same turn gives the scroll container 186px back, and
    // the binding is fed by a ResizeObserver that has not necessarily
    // delivered by `tick`. Centring against the stale value would put the row
    // three rows high — small, but wrong every single time a jump had to read
    // pages, which is the case this exists for.
    const centred = index * ROW_HEIGHT - Math.max(0, (scrollEl.clientHeight - ROW_HEIGHT) / 2);
    scrollEl.scrollTop = Math.max(0, centred);
    // Read back rather than assigned from `centred`: the browser clamps to the
    // scrollable range, and the virtualization window has to agree with where
    // the box actually is, not where it was asked to go.
    scrollTop = scrollEl.scrollTop;
  }

  // The results box is 186px and the cursor walks the whole list, so ↓ runs
  // off the bottom of it within six presses. `block: 'nearest'` rather than
  // 'center': a cursor already in view must not shunt the list on every
  // keystroke, which is the difference between following the selection and
  // fighting it. Queried rather than kept in a per-row binding array, because
  // the list is rebuilt on every keystroke and a stale array of elements is a
  // second thing to keep correct for no gain.
  $effect(() => {
    if (!findOpen) return;
    // Both dependencies earn themselves. The cursor is the obvious one; `hits`
    // is the case where it is *not* moving — narrowing the query while
    // scrolled halfway down leaves the box scrolled there over a list that is
    // now three rows long, with the cursor sitting on a row above the fold.
    void cursorAt;
    void hits;
    resultsEl?.querySelector('.hit.on')?.scrollIntoView({ block: 'nearest' });
  });

  function onFindKeys(event: KeyboardEvent) {
    if (hits.length === 0) return;
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      cursor = (cursorAt + 1) % hits.length;
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      cursor = (cursorAt + hits.length - 1) % hits.length;
    } else if (event.key === 'Enter') {
      event.preventDefault();
      void jumpTo(hits[cursorAt]);
    }
    // Esc is deliberately absent: it bubbles to the window handler, which
    // already decides what Esc closes when several things are open (§5.2).
  }

  // Branch switching (§8.3, §8.4, build step 8) — double-click a ref badge
  // or pick one from a row's right-click menu; both funnel through here.
  //
  // One in-flight write at a time from this pane. The per-repo write queue
  // (§7) already serialises them on the Rust side; this exists so a second
  // click cannot queue a switch behind a merge whose result is not on screen
  // yet, which would leave the two failures fighting over one banner.
  let busy = $state(false);

  /** The branch a switch is on its way to (§13, *Work in progress*), `null`
   *  when none is running. Pane state rather than store state because it is
   *  the *destination* — the backend's `write:begin` says this repo is busy
   *  and with which operation, but only the click knows which badge it named,
   *  and naming it is what separates a slow switch from a slow merge started
   *  from the same menu. */
  let switchingTo = $state<string | null>(null);

  /** The repo a *successful* switch is still owed a behind-check for (§8.3),
   *  `null` when there is none. Git Graph's gesture: a checkout is nearly
   *  always the start of working on that branch, so if the branch you have
   *  landed on is behind its upstream, ask right there rather than leaving the
   *  ↓ badge to be noticed later.
   *
   *  Deliberately not the branch name the badge carried. Switching to a
   *  remote-tracking badge checks out a *local* branch of a different name
   *  (`origin/x` → `x`), and one whose local counterpart already existed can
   *  land somewhere else again (`branch.rs`'s "already exists" fallback). The
   *  only honest answer to "which branch am I now on and is it behind?" is the
   *  status read the switch itself triggered, so this holds only the repo the
   *  answer has to be about. */
  let pendingPullCheck = $state<string | null>(null);

  /** Non-null while that question is on screen. Snapshotted rather than read
   *  live from status: a sweep or an FS watcher can land while the modal is
   *  up, and the sentence must keep saying what the user was asked about. */
  let pullPrompt = $state<{ branch: string; upstream: string; behind: number } | null>(null);

  /** Cleared here rather than in `switchTo` because the wait does not end when
   *  the command returns. `write_and_refresh` emits `status:repo` before it
   *  does, `graph.svelte.ts` reloads the page and refs off that event, and
   *  until *that* lands the badges on screen are still the pre-switch ones.
   *  Dropping the label at the command boundary would say "done" over a view
   *  about to swap (§13).
   *
   *  `repos.isBusy` and not just the local `busy`, and the difference is the
   *  whole reason this works: `busy` falls when the *invoke* resolves, which
   *  is unordered against event delivery, so the reload may not have started
   *  and `graph.loading` may still be false — clearing then would be the exact
   *  early drop this effect exists to prevent. `write:end` is emitted after
   *  `emit_repo_status`, so by the time it lands the status event has already
   *  been delivered and the reload it triggers has already set `loading`. */
  $effect(() => {
    const id = repos.selectedId;
    const stillWriting = busy || (id !== undefined && repos.isBusy(id));
    const settled = !stillWriting && !graph.loading;
    if (switchingTo !== null && settled) switchingTo = null;

    // The behind-check rides the same moment, and for the same reason: the
    // question is about the branch that is now checked out, and until the
    // status this write emitted has landed, `repos.status` still describes the
    // branch we left — which would ask about the wrong branch's counts, or
    // stay silent about a behind one. Asking here rather than after the
    // `await` in `switchTo` is the whole correctness argument.
    if (pendingPullCheck !== null && settled) {
      const target = pendingPullCheck;
      // Consumed either way: the check is a one-shot per switch, and a repo
      // that turns out to be up to date must not be re-examined by the next
      // unrelated write to settle.
      pendingPullCheck = null;
      // Only if it is still the repo on screen: `repos.pull()` writes to
      // whichever repo is selected, so a question raised about another one
      // would offer the right sentence over the wrong write.
      const after = id === target ? repos.status(target) : undefined;
      // The two null checks are the detached-HEAD case, not belt and braces:
      // git emits `# branch.ab` only where an upstream exists, so `behind > 0`
      // cannot happen without one — but nothing in the *type* says so, and the
      // dialog names both.
      if (after !== undefined && after.branch !== null && after.upstream !== null && canPull(after)) {
        pullPrompt = { branch: after.branch, upstream: after.upstream, behind: after.behind };
      }
    }
  });

  /** Narration for anything else the repo is doing — a merge from the same
   *  menu, a pull started from the row while the graph is open. Gated at
   *  150 ms like the repo row's spinner; the switch label above is not, since
   *  it is cleared by a condition rather than by a timer and would otherwise
   *  need two clocks agreeing. */
  let writeShown = $state(false);
  const writeIndicator = new BusyIndicator((shown) => (writeShown = shown));

  $effect(() => {
    const id = repos.selectedId;
    writeIndicator.set(id !== undefined && repos.isBusy(id));
  });

  $effect(() => () => writeIndicator.dispose());

  /** What the header says while something is running. The switch names its
   *  branch; everything else falls back to the operation's own word, which is
   *  the same one the error banner would use if it fails. */
  const workingLabel = $derived.by(() => {
    if (switchingTo !== null) return `Switching to ${switchingTo}…`;
    if (!writeShown) return null;
    const id = repos.selectedId;
    const operation = id === undefined ? undefined : repos.busyOperation(id);
    return operation === undefined ? null : `${operation}…`;
  });

  let menu = $state<{ x: number; y: number; refs: RefBadge[]; hash: string } | null>(null);
  // Non-null while the Create Branch dialog is up; the value is the start
  // point the new branch will be cut from (§8.3).
  let createFrom = $state<string | null>(null);
  // Non-null while the Delete Branch dialog is up (§8.3). `refusal` carries
  // git's "not fully merged" text once a safe delete has come back with one,
  // which is what turns the dialog's second step on; it lives here rather than
  // in the dialog because it *is* the failed write's error, and every other
  // write's error is the pane's to hold.
  let deleting = $state<{ name: string; refusal: string | null } | null>(null);

  // Esc shuts the info panel (§5.2) and leaves the row selected — it undoes
  // the *Show info* that opened the column, not the click that picked the row.
  // Guarded three ways: the panel has to be open; the graph has to be the view
  // on screen, since DiffView owns Esc while the diff tab is showing and one
  // press must not close two things; and the context menu gets it first. The
  // menu is the only overlay needing that check — both dialogs handle Esc on
  // the dialog element and stop it propagating, so it never reaches this
  // window listener while one is up.
  function onKeydown(event: KeyboardEvent) {
    // Ctrl+F opens the branch search (§5.3), guarded the same way Esc is
    // below: DiffView owns the pane while a diff is on screen, and a shortcut
    // that opened the graph's find box over a diff would be acting on a list
    // the user cannot see. `preventDefault` because Chromium's own find bar
    // would otherwise open over a pane whose rows are virtualized — it would
    // search the dozen rows in the DOM and report nothing for the rest, which
    // is worse than not offering it.
    if ((event.ctrlKey || event.metaKey) && event.key === 'f') {
      if (diff.view !== 'graph' || !hasRepo || graph.rows.length === 0) return;
      // Not while the user is typing somewhere else. This is a window
      // listener, so without the check Ctrl+F halfway through a commit message
      // would take focus out of the compose box and put it in a pane the user
      // is not looking at (§5.2). The find box itself is exempt: Ctrl+F in it
      // re-selects the query, which is what pressing it again should do.
      const target = event.target;
      const typing =
        target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement;
      if (typing && target !== findInputEl) return;

      event.preventDefault();
      void openFind();
      return;
    }

    if (event.key !== 'Escape') return;
    if (diff.view !== 'graph' || menu !== null) return;
    // Before the info column, not after: Esc closes whatever is on top, and
    // the find box is both the most recently opened thing and the one holding
    // focus. Closing the column out from under it would be answering a press
    // the user aimed somewhere else.
    if (findOpen) {
      closeFind();
      return;
    }
    if (graph.infoOpen) graph.closeInfo();
  }

  // Only local names: git rejects a new branch that collides with one, and the
  // remote-tracking badges sharing the graph are a different namespace.
  const localBranchNames = $derived(
    graph.refs.filter((ref) => ref.kind === 'local').map((ref) => ref.name),
  );

  async function switchTo(ref: RefBadge) {
    if (busy) return;
    busy = true;
    // Set before the await, not after it: this is the acknowledgement, and it
    // has to be on screen in the same frame as the double-click (§13).
    switchingTo = ref.name;
    const repoId = repos.selectedId;
    const ok = await repos.switchBranch(ref.name, ref.kind);
    // The banner is already up (§13); this only tells it something git's
    // stderr does not carry — that the tree was dirty when the checkout was
    // refused, which is what makes *Open in VS Code* the right way out.
    if (!ok && dirty) notices.overrideAction('open-vscode');
    // Only a switch that landed earns the question. A refused checkout leaves
    // HEAD where it was, and asking to pull the branch the user was already on
    // would read as if the switch had worked.
    if (ok && repoId !== undefined) pendingPullCheck = repoId;
    busy = false;
    // `switchingTo` is left standing — the effect above takes it down once the
    // graph reload triggered by this write has landed.
  }

  // Merging from the graph (§8.3) — the badge names the source, the
  // destination is always the checked-out branch.
  //
  // No action override here, unlike `switchTo`: a merge tells you why it
  // failed in its own words every time — either git's "your local changes
  // would be overwritten", which `translateGitError` already answers with
  // *Open in VS Code*, or a conflict, which raises the blocking banner from
  // the status refresh instead. Forcing the dirty-tree action would override
  // the conflict case, and a conflict leaves the tree dirty by definition, so
  // it would override it exactly when it is wrong.
  async function mergeInto(ref: RefBadge) {
    if (busy) return;
    busy = true;
    await repos.mergeBranch(ref.name);
    busy = false;
  }

  /** *Pull* from the after-switch question (§8.3, §8.7). Plain `repos.pull()`
   *  — the same command the row's Pull and the middle pane's run, so a pull
   *  started here fails, retries and reports identically (§13). No action
   *  override: `git pull` on a dirty tree already says so in words
   *  `translateGitError` answers with *Open in VS Code*, and a conflict raises
   *  the blocking banner from the status refresh instead. */
  async function pullAfterSwitch(): Promise<void> {
    if (busy) return;
    busy = true;
    await repos.pull();
    busy = false;
  }

  async function createBranch(name: string, checkout: boolean): Promise<boolean> {
    const startPoint = createFrom;
    if (!startPoint) return false;

    const ok = await repos.createBranch(name, startPoint, checkout);
    // Only a checkout can fail on a dirty tree; a plain `git branch` never
    // touches the working tree, so offering VS Code there would be noise.
    if (!ok && checkout && dirty) notices.overrideAction('open-vscode');
    return ok;
  }

  // Deleting a local branch (§8.3). Safe-first: the dialog's first button
  // passes `force: false`, and the only way to reach `true` is through the
  // *Delete anyway* that appears once git has refused an unmerged branch.
  //
  // That refusal is the one write failure this pane takes off the banner: the
  // dialog is still up and already showing it, and a banner above the scrim
  // saying the same thing would be the same error twice. Every other failure
  // closes the dialog and leaves the banner to do its job.
  async function deleteBranch(force: boolean) {
    const target = deleting;
    if (!target || busy) return;
    busy = true;

    const ok = await repos.deleteBranch(target.name, force);
    // Classified, not displayed — hence `lastWriteError` rather than anything
    // the banner owns (§8.3: this is the one git failure Corgit answers with a
    // different button instead of a headline).
    const raw = repos.lastWriteError;
    if (ok) {
      deleting = null;
    } else if (!force && raw !== null && isUnmergedBranchRefusal(raw)) {
      notices.dismiss();
      deleting = { name: target.name, refusal: raw };
    } else {
      deleting = null;
    }
    busy = false;
  }

  // Every row has a menu now, badges or not: *Show info* is the only way into
  // the info column (§5.2), so a plain commit with no refs on it must still
  // open one. That is why the old `refs.length === 0` bail is gone.
  function openMenu(event: MouseEvent, refs: RefBadge[], hash: string) {
    event.preventDefault();
    menu = { x: event.clientX, y: event.clientY, refs, hash };
  }

  // *Show info* leads because it is the one entry every row has; the branch
  // entries below it exist only on the rows carrying a badge.
  function menuItems(refs: RefBadge[], hash: string) {
    return [
      { label: 'Show info', onSelect: () => graph.showInfo(hash) },
      ...refs.flatMap((ref) => [
        {
          label: ref.kind === 'local' ? `Switch to ${ref.name}` : `Switch to ${ref.name} (new local branch)`,
          onSelect: () => switchTo(ref),
        },
        {
          label: `Create branch from ${ref.name}…`,
          onSelect: () => (createFrom = ref.name),
        },
        // Merging a branch into itself is git's own no-op, so the badge for
        // the branch you are on does not offer it. Remote-tracking badges do:
        // merging `origin/main` into your branch is the same gesture, and the
        // one Pull does not cover when the branch you want is not upstream.
        ...(currentBranch !== null && !(ref.kind === 'local' && ref.name === currentBranch)
          ? [
              {
                label: `Merge ${ref.name} into ${currentBranch}`,
                onSelect: () => mergeInto(ref),
              },
            ]
          : []),
        // Local badges only, and never the one you are standing on: git
        // refuses to delete the checked-out branch, and deleting a remote
        // badge would be a `push --delete` — a network write with a different
        // blast radius, so it is not folded into the same entry (§8.3).
        ...(ref.kind === 'local' && ref.name !== currentBranch
          ? [
              {
                label: `Delete ${ref.name}`,
                onSelect: () => (deleting = { name: ref.name, refusal: null }),
              },
            ]
          : []),
      ]),
    ];
  }
</script>

<svelte:window onkeydown={onKeydown} />

<Pane title="Graph">
  {#snippet tabs()}
    <button
      type="button"
      class="tab"
      class:active={diff.view === 'graph'}
      role="tab"
      aria-selected={diff.view === 'graph'}
      onclick={() => diff.select('graph')}
    >Graph</button>
    {#if diff.open}
      <!-- Two buttons rather than a close button nested inside the tab: a
           button inside a button is invalid, and the close target has to be
           separately clickable anyway. -->
      <div class="tab tab-file" class:active={diff.view === 'diff'}>
        <button
          type="button"
          class="tab-label"
          role="tab"
          aria-selected={diff.view === 'diff'}
          title={diff.open.path}
          onclick={() => diff.select('diff')}
        >{fileName}</button>
        <button
          type="button"
          class="tab-close"
          title="Close diff"
          aria-label="Close diff"
          onclick={() => diff.close()}
        >
          <Glyph kind="cross" />
        </button>
      </div>
    {/if}
  {/snippet}

  {#snippet actions()}
    {#if workingLabel}
      <!-- Takes the repo name's slot rather than sitting beside it, the same
           mechanic RepoList's header already uses for the sweep (§13): the two
           never fight for the space, and the name comes back the moment the
           write lands. The dog is permitted here and nowhere near the rows —
           docs/mascot.md §2 draws that line at "dead space and dead time", and
           a pane header is chrome. -->
      <span class="working" aria-live="polite">
        <Mascot pose="mini-working" height={18} />
        <span>{workingLabel}</span>
      </span>
    {:else if repoName}
      <!-- Right-hand side rather than beside the tabs: the tab strip is a
           tablist, and a label that is not a tab does not belong inside it —
           nor should the repo name grow into a third tab-looking thing when a
           diff is open. -->
      <span class="repo-name" title={repos.selectedRepo?.path}>{repoName}</span>
    {/if}
    <!-- §11.1's middle rung — a borderless 22px icon, "safe, idempotent, press
         it again" — and the same box as *Fetch all* one pane over. Opening a
         find box is exactly that kind of act.

         After the repo name rather than before it, unlike the repo list's
         Fetch all: there the icon is the only thing in the header's actions,
         here the name is the pane's subject and the button acts on it. Reading
         "Corgit ⌕" is the order the sentence is in.

         Drawn rather than typed. ⌕ (U+2315) is missing from Segoe UI Variable
         and falls back to a different face at a different weight, which is the
         same class of defect Glyph.svelte records for +/−/×. -->
    <button
      type="button"
      class="icon-action"
      class:on={findOpen}
      disabled={!hasRepo || graph.rows.length === 0}
      onclick={() => (findOpen ? closeFind() : void openFind())}
      title="Find a branch (Ctrl+F)"
      aria-label="Find a branch"
      aria-expanded={findOpen}
    >
      <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" aria-hidden="true">
        <circle cx="5" cy="5" r="3.4" />
        <line x1="7.6" y1="7.6" x2="10.6" y2="10.6" stroke-linecap="round" />
      </svg>
    </button>
  {/snippet}

  <!-- Both views stay mounted and laid out (§5.4), stacked rather than swapped:
       each is a scroll container, and the graph carries scroll position and
       however many pages the user loaded. Unmounting it — or even `display:
       none`, which destroys the scroll box — would make a glance at a diff
       cost a scroll back down to where they were. -->
  <div class="views">
  <div class="view" class:hidden={diff.view !== 'graph'}>
    {#if !hasRepo}
      <!-- The dog lives here (SPEC §14.1). The commit pane sits empty at the
           same moment and is deliberately left bare, because two of him on
           screen at once stops being charming. Its own `content` placement is
           the opposite case — a repo selected and clean — so the two can never
           both be showing.

           Two poses share the slot: he lies down once the whole herd is clean
           and in sync, and sits up waiting otherwise (docs/mascot.md §5). -->
      {#if repos.allClean}
        <EmptyState message="All in sync" hint="Nothing needs you — select a repository to browse its history">
          {#snippet art()}
            <Mascot pose="content" height={112} />
          {/snippet}
        </EmptyState>
      {:else}
        <EmptyState message="Nothing to herd" hint="Select a repository to see its history">
          {#snippet art()}
            <Mascot pose="resting" height={132} gaze />
          {/snippet}
        </EmptyState>
      {/if}
    {:else if graph.loading && graph.rows.length === 0}
      <EmptyState message="Reading history…" />
    {:else if graph.error}
      <EmptyState message="Could not read history" hint={graph.error} />
    {:else}
      <!-- Its own flex column, mirroring Pane's internal layout, so `.scroll`
           gets a definite height to virtualize against regardless of whether
           the Uncommitted Changes node is showing above it. -->
      <div class="graph-body">
        {#if findOpen}
          <!-- A --bg-app band cut into the pane, which is the shape §5.1
               already uses for chrome that scopes what sits under it — the
               root strip and both section bands. Above the *Uncommitted
               Changes* node because that node is a row, and this is chrome
               over the rows. -->
          <div class="find">
            <input
              bind:this={findInputEl}
              bind:value={query}
              type="text"
              placeholder="Find a branch…"
              spellcheck="false"
              autocapitalize="off"
              autocorrect="off"
              aria-label="Find a branch in this repository"
              oninput={() => {
                cursor = 0;
                outOfReach = null;
              }}
              onkeydown={onFindKeys}
            />
            <span class="find-count">
              {query.trim() === ''
                ? `${graph.refs.length} branches`
                : `${hits.length} of ${graph.refs.length}`}
            </span>
            <button
              type="button"
              class="find-close"
              onclick={closeFind}
              title="Close (Esc)"
              aria-label="Close branch search"
            >
              <Glyph kind="cross" />
            </button>
          </div>

          <!-- In flow, not floating. Pane.svelte's `.body` is a scroll
               container, so an absolutely-positioned popover anchored here
               would be clipped by it and would scroll away with the pane —
               which is why ContextMenu.svelte is `position: fixed` with
               viewport coordinates and a measure-then-nudge pass. A strip that
               shrinks the graph needs none of that machinery, and the rows are
               virtualized, so the height costs only rows you can see. -->
          <div class="results" bind:this={resultsEl}>
            {#if results.length === 0}
              <p class="no-hits">No branch matches</p>
            {:else}
              <div class="group">
                <span>{query.trim() === '' ? 'Every branch' : `Matching “${query.trim()}”`}</span>
                <span class="note">tip commit</span>
              </div>
              {#each results as hit, index (hit.ref.kind + hit.ref.name)}
                <button
                  type="button"
                  class="hit"
                  class:on={index === cursorAt}
                  onclick={() => void jumpTo(hit.ref)}
                  title={hit.loaded
                    ? `Go to ${hit.ref.name}`
                    : `Go to ${hit.ref.name} — its tip is not in the loaded history yet`}
                >
                  <span
                    class="name"
                    class:remote={hit.ref.kind === 'remote'}
                    class:current={hit.ref.kind === 'local' && hit.ref.name === currentBranch}
                    >{hit.ref.name}</span
                  >
                  <span class="tip">{hit.ref.subject}</span>
                  <!-- The one thing the search knows that the graph cannot
                       show. Muted, because it is a fact about the pane and not
                       a state of the branch — the status colours all mean
                       something about a repo (§11). -->
                  {#if !hit.loaded}
                    <span class="unloaded">not loaded</span>
                  {/if}
                  <span class="when">{hit.date}</span>
                </button>
              {/each}
            {/if}
            <!-- One slot, three sentences, so the list never changes height
                 under the pointer while a jump runs. -->
            <p class="foot" class:said={graph.revealing !== null || outOfReach !== null}>
              {#if graph.revealing !== null}
                Reading history to reach {graph.revealing}…
              {:else if outOfReach !== null}
                {outOfReach} is further back than one jump reads — load more history, then search again.
              {:else}
                Enter goes to the branch tip; it does not switch to it.
              {/if}
            </p>
          </div>
        {/if}

        {#if dirty}
          <button
            type="button"
            class="uncommitted"
            class:selected={graph.selection === 'working-tree'}
            onclick={() => graph.select('working-tree')}
          >
            <svg class="lanes" width={LANE_WIDTH} height={ROW_HEIGHT} viewBox="0 0 {LANE_WIDTH} {ROW_HEIGHT}">
              {#if graph.rows.length > 0}
                <line
                  x1={LANE_WIDTH / 2}
                  y1={ROW_HEIGHT / 2}
                  x2={LANE_WIDTH / 2}
                  y2={ROW_HEIGHT}
                  stroke={laneColorVar(0)}
                />
              {/if}
              <circle cx={LANE_WIDTH / 2} cy={ROW_HEIGHT / 2} r="4" fill={laneColorVar(0)} />
            </svg>
            <span class="subject">Uncommitted Changes</span>
          </button>
        {/if}

        {#if graph.rows.length === 0}
          <EmptyState message="No commits yet" hint="Make the first commit to see history here" />
        {:else}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <!-- The keyboard equivalent is Esc (`onKeydown`), which is why this
               needs no key handler of its own: there is nothing to focus here,
               the click target *is* the absence of a row. -->
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div
            class="scroll"
            bind:this={scrollEl}
            bind:clientHeight={viewportHeight}
            onscroll={onScroll}
            onclick={onBackgroundClick}
          >
            <div class="spacer" bind:this={spacerEl} style="height: {rowWindow.totalHeight}px">
              <div class="window" style="transform: translateY({rowWindow.topOffset}px)">
                {#each visibleRows as row (row.commit.hash)}
                  <GraphRow
                    {row}
                    laneCount={lanes}
                    refs={graph.refsByHash.get(row.commit.hash) ?? []}
                    selected={graph.selection === row.commit.hash}
                    {currentBranch}
                    headHash={status?.head ?? null}
                    {switchingTo}
                    onSelect={() => graph.select(row.commit.hash)}
                    onSwitchBranch={switchTo}
                    onContextMenu={openMenu}
                  />
                {/each}
              </div>
            </div>
            {#if graph.hasMore}
              <button type="button" class="load-more" disabled={graph.loadingMore} onclick={() => graph.loadMore()}>
                {graph.loadingMore ? 'Loading…' : 'Load more'}
              </button>
            {/if}
          </div>
        {/if}
      </div>
    {/if}
  </div>

  {#if diff.open}
    <div class="view" class:hidden={diff.view !== 'diff'}>
      <DiffView />
    </div>
  {/if}
  </div>
</Pane>

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} items={menuItems(menu.refs, menu.hash)} onClose={() => (menu = null)} />
{/if}

{#if createFrom}
  <CreateBranchDialog
    startPoint={createFrom}
    existingLocal={localBranchNames}
    onCreate={createBranch}
    onClose={() => (createFrom = null)}
  />
{/if}

{#if pullPrompt}
  <PullAfterSwitchDialog
    branch={pullPrompt.branch}
    upstream={pullPrompt.upstream}
    behind={pullPrompt.behind}
    onPull={pullAfterSwitch}
    onClose={() => (pullPrompt = null)}
  />
{/if}

{#if deleting}
  <DeleteBranchDialog
    name={deleting.name}
    refusal={deleting.refusal}
    {busy}
    onDelete={deleteBranch}
    onClose={() => (deleting = null)}
  />
{/if}

<style>
  .views {
    position: relative;
    height: 100%;
  }

  /* Stacked, not swapped — see the markup. `visibility: hidden` is doing real
     work here: it keeps the box laid out (so the hidden view's scroll offset
     and virtualization measurements survive) while taking it out of the paint,
     out of hit-testing and out of the tab order. */
  .view {
    position: absolute;
    inset: 0;
  }

  .view.hidden {
    visibility: hidden;
  }

  /* Tabs read as a segmented control rather than browser tabs — the pane
     header is 34 px and already carries a border, so a full tab shape would
     be two competing edges. The active one is marked by surface lightness,
     matching §11 rule 2, not by the accent (rule 3: selection in the *lists*
     is what the accent is for). */
  .tab {
    display: flex;
    align-items: center;
    flex: 0 1 auto;
    min-width: 0;
    height: 22px;
    padding: 0 var(--space-2);
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text-muted);
    font-size: var(--text-xs);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    white-space: nowrap;
  }

  .tab:hover {
    color: var(--text-primary);
    background: var(--bg-hover);
  }

  .tab.active {
    color: var(--text-primary);
    background: var(--bg-raised);
    border-color: var(--border-strong);
  }

  /* The filename keeps its own case — it is a name, not a label. */
  .tab-file {
    gap: var(--space-1);
    padding-right: var(--space-1);
    text-transform: none;
    letter-spacing: 0;
  }

  .tab-label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    padding: 0;
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
    font-family: var(--font-mono);
  }

  /* A drawn cross, not a `×` character — see Glyph.svelte for why. Centring a
     block child is exact; centring a glyph's line box is not. */
  .tab-close {
    display: flex;
    align-items: center;
    justify-content: center;
    flex: 0 0 auto;
    width: 16px;
    height: 16px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text-muted);
  }

  .tab-close:hover {
    background: var(--bg-active);
    color: var(--text-primary);
  }

  /* A name, so it keeps its own case — deliberately unlike the uppercase
     pane titles and tabs around it, which are labels. */
  /* Same bounds as .repo-name below, whose slot it takes — a label that
     changed the header's width on every switch would move the tab strip. */
  .working {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    max-width: 22ch;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    font-size: var(--text-sm);
    color: var(--text-secondary);
  }

  .working span {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .repo-name {
    /* Bounded rather than free-growing: Pane's `.actions` never shrinks, so
       without a cap a long directory name would squeeze the tab strip — and
       a tab you cannot read is worse than a name you cannot. */
    max-width: 22ch;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-size: var(--text-sm);
    color: var(--text-secondary);
  }

  .graph-body {
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  /* The branch search (§5.3). Deliberately identical to RepoList's
     `.icon-action`, down to the values — §11.1's borderless rung is one
     control at several scopes, and two copies that merely look alike is how
     they stop looking alike. */
  .icon-action {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text-muted);
    line-height: 1;
    cursor: default;
  }

  .icon-action:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .icon-action:disabled {
    color: var(--text-disabled);
  }

  /* While the box is open this button is what closes it, so it holds the
     pressed state rather than going on advertising itself as pressable. The
     tab strip's `.active` is the same idea and cannot be reused: that is a
     tab, and this must not grow a border and read as a third one. */
  .icon-action.on {
    background: var(--bg-active);
    color: var(--text-primary);
  }

  /* A --bg-app band, like the root strip and the section bands (§5.1, §11.1):
     it reads as a band cut across the pane rather than as its first row. */
  .find {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex: 0 0 auto;
    padding: var(--space-1) var(--space-3);
    background: var(--bg-app);
    border-bottom: 1px solid var(--border);
  }

  /* RepoList's filter input, to the value: 26px, --bg-raised on a --bg-app
     band, 1px --border, --radius-sm. One box for "type to narrow a list",
     whichever pane it is in. */
  .find input {
    flex: 1 1 auto;
    min-width: 0;
    height: 26px;
    padding: 0 var(--space-2);
    font: inherit;
    font-size: var(--text-sm);
    color: var(--text-primary);
    background: var(--bg-raised);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }

  .find input::placeholder {
    color: var(--text-disabled);
  }

  .find input:focus-visible {
    border-color: var(--accent);
    outline: none;
  }

  .find-count {
    flex: 0 0 auto;
    font-size: var(--text-xs);
    font-variant-numeric: tabular-nums;
    color: var(--text-disabled);
  }

  .find-close {
    display: flex;
    align-items: center;
    justify-content: center;
    flex: 0 0 auto;
    width: 22px;
    height: 22px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text-muted);
    cursor: default;
  }

  .find-close:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  /* Bounded so the graph never disappears behind its own search: at 186px the
     list holds five results and scrolls, and eight rows of history stay
     visible in the shortest pane §4 allows. */
  .results {
    flex: 0 0 auto;
    max-height: 186px;
    overflow-y: auto;
    background: var(--bg-app);
    border-bottom: 1px solid var(--border);
  }

  /* The picker's section head (SwitchPullDialog), at the pane's own 12px
     gutter rather than the dialog's 8px. */
  .group {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: 6px var(--space-3) var(--space-1);
    font-size: var(--text-xs);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .group .note {
    text-transform: none;
    letter-spacing: 0.04em;
    font-weight: 400;
    color: var(--text-disabled);
  }

  .hit {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    height: 26px;
    padding: 0 var(--space-3);
    border: 0;
    background: none;
    text-align: left;
    cursor: default;
  }

  .hit:hover {
    background: var(--bg-hover);
  }

  /* The keyboard cursor, and deliberately not the accent: §11 rule 3 keeps
     that for selection, and nothing is selected until Enter — pressing it is
     what paints a row in --accent-muted, one list down. */
  .hit.on {
    background: var(--bg-active);
  }

  /* GraphRow's `.ref` box, so a branch name means the same thing in the search
     as it does on the row the search is about to land on. Wider, because the
     column it sits in is the whole list rather than a row already carrying a
     message, an author and a date. */
  .hit .name {
    flex: 0 0 auto;
    max-width: 250px;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    padding: 1px var(--space-1);
    font-size: var(--text-xs);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background: var(--bg-raised);
    color: var(--text-secondary);
  }

  .hit .name.remote {
    color: var(--text-muted);
    font-style: italic;
  }

  /* Bold like GraphRow's `.current`, but without the lane colour: lane hues
     belong to a row's dot, and there is no dot here to tie one to. */
  .hit .name.current {
    color: var(--text-primary);
    border-color: var(--text-muted);
    font-weight: 700;
  }

  .hit .tip {
    flex: 1 1 6rem;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-size: var(--text-sm);
    color: var(--text-muted);
  }

  .hit .unloaded {
    flex: 0 0 auto;
    font-size: var(--text-xs);
    color: var(--text-disabled);
  }

  /* The graph's own date column, at the same width and the same alignment, so
     the two lists read as one surface (§5.3). */
  .hit .when {
    flex: 0 0 auto;
    width: 140px;
    text-align: right;
    font-variant-numeric: tabular-nums;
    font-size: var(--text-xs);
    color: var(--text-disabled);
  }

  /* The picker's own words and its own metrics, kept (SwitchPullDialog). */
  .no-hits {
    margin: 0;
    padding: var(--space-1) var(--space-3) 6px;
    font-size: var(--text-xs);
    color: var(--text-disabled);
  }

  .foot {
    margin: 0;
    padding: var(--space-1) var(--space-3) 6px;
    border-top: 1px solid var(--border);
    font-size: var(--text-xs);
    color: var(--text-disabled);
  }

  /* One step up when the line stops being a standing hint and starts being an
     answer to something the user just did. Not a status colour: every hue in
     §11 already means something about a repo, and this is the pane talking
     about itself. */
  .foot.said {
    color: var(--text-secondary);
  }

  .uncommitted {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    height: var(--row-height);
    flex: 0 0 auto;
    padding: 0 var(--space-3);
    border: 0;
    border-bottom: 1px solid var(--border);
    background: none;
    text-align: left;
    cursor: default;
  }

  .uncommitted:hover {
    background: var(--bg-hover);
  }

  .uncommitted.selected {
    background: var(--accent-muted);
  }

  .uncommitted .lanes {
    flex: 0 0 auto;
  }

  .uncommitted .subject {
    font-size: var(--text-sm);
    color: var(--text-primary);
  }

  .scroll {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    overflow-x: hidden;
  }

  .spacer {
    position: relative;
  }

  .window {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
  }

  .load-more {
    display: block;
    width: 100%;
    height: var(--row-height);
    border: 0;
    border-top: 1px solid var(--border);
    background: none;
    color: var(--text-muted);
    font-size: var(--text-sm);
    cursor: default;
  }

  .load-more:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .load-more:disabled {
    color: var(--text-disabled);
  }
</style>
