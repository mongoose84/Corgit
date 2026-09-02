<script lang="ts">
  // The error banner (SPEC.md §13) — the one surface every git failure that
  // needs a decision now goes through.
  //
  // It is app chrome rather than a pane's child, and that is the point of the
  // rewrite: the notices it replaces lived inside panes whose minimums are
  // 240px and 190px (§4), where a headline, an action and *Details* could not
  // share a line and the component ended up shaped around the shortage. Here
  // it has the window's width, so the layout is decided by the message.
  //
  // Not a modal, deliberately. The failure has already happened; freezing a
  // dashboard over *many* repositories to report that one of them failed to
  // push would be punishment. Modals stay where §13 leaves them — confirming
  // an irreversible act *before* it happens, which is `DiscardDialog` and
  // `DeleteBranchDialog`.
  import Mascot from './Mascot.svelte';
  import Glyph from './Glyph.svelte';
  import type { GitErrorAction, GitErrorTier } from './gitErrors';

  interface Props {
    tier: GitErrorTier;
    /** Plain-language headline, or git's own stderr when no rule matched. */
    message: string;
    /** The repo this is about, when it is about one. Row-level Pull (§5.1)
     *  can fail in a repo that is not selected, so a banner that does not name
     *  its subject is ambiguous across a 77-row list. */
    repoName?: string;
    /** Full stderr for the *Details* disclosure. Absent when it would only
     *  repeat the headline — an untranslated failure is already raw. */
    details?: string;
    action?: GitErrorAction;
    /** A bulk run's failures, as *Show the N* (§5.1). Present only on a run
     *  summary, where the banner is about several repos at once and the way to
     *  them is the filter box rather than a selection. */
    showRepos?: { count: number; onShow: () => void };
    /** Drawn only when the notice matched a rule; `null` ids cannot be
     *  suppressed and so must not appear suppressible (§13). */
    canSuppress?: boolean;
    onPull?: () => void;
    onOpenVSCode?: () => void;
    onRetry?: () => void;
    onAbortMerge?: () => void;
    onSelectRepo?: () => void;
    onDismiss?: () => void;
    onSuppress?: () => void;
  }

  let {
    tier,
    message,
    repoName,
    details,
    action,
    showRepos,
    canSuppress = false,
    onPull,
    onOpenVSCode,
    onRetry,
    onAbortMerge,
    onSelectRepo,
    onDismiss,
    onSuppress,
  }: Props = $props();

  let detailsOpen = $state(false);

  // A blocking banner renders a condition rather than an event, so it has
  // neither of the two ways to make it go away by hand: dismissing it would
  // only produce chrome that disagrees with the repository until the next
  // sweep repaints it, and suppressing it would hide a repo that cannot commit
  // or push. §13: a notification may be suppressed, a condition may never be.
  const blocking = $derived(tier === 'blocking');
</script>

<div class="banner" class:blocking role="status" aria-live="polite">
  <div class="row">
    <!-- Sorry, not alarmed (docs/mascot.md §5). He fits here at a readable
         size for the first time — this is full window width, not the 240px
         compose pane that forced him down to 20px. -->
    <Mascot pose="mini-sorry" height={24} />

    {#if repoName}
      {#if onSelectRepo}
        <button type="button" class="repo" title="Select {repoName}" onclick={onSelectRepo}>
          {repoName}
        </button>
      {:else}
        <span class="repo static">{repoName}</span>
      {/if}
    {/if}

    <p class="message selectable" title={message}>{message}</p>

    <div class="actions">
      <!-- Blocking's "exactly two buttons, never a third" (§13) — never a
           force-anything. Abort leads to a confirmation rather than acting on
           the click: it discards a merge in progress, which is the kind of act
           §13 does route through a modal. -->
      {#if blocking && onAbortMerge}
        <button type="button" class="primary" onclick={onAbortMerge}>Abort merge…</button>
      {/if}
      {#if action === 'pull' && onPull}
        <button type="button" onclick={onPull}>Pull</button>
      {:else if action === 'retry' && onRetry}
        <button type="button" onclick={onRetry}>Retry</button>
      {/if}
      {#if (action === 'open-vscode' || blocking) && onOpenVSCode}
        <button type="button" onclick={onOpenVSCode}>Open in VS Code</button>
      {/if}

      <!-- Ahead of *Details* because it is the way forward rather than the way
           deeper in: the run named its failures in the headline, and this puts
           those rows in front of the user. -->
      {#if showRepos}
        <button type="button" class="primary" onclick={showRepos.onShow}>
          Show the {showRepos.count}
        </button>
      {/if}

      {#if details}
        <button type="button" class="link" onclick={() => (detailsOpen = !detailsOpen)}>
          {detailsOpen ? 'Hide details' : 'Details'}
        </button>
      {/if}

      {#if !blocking && canSuppress && onSuppress}
        <!-- A checkbox rather than a button, because it is a setting being
             changed and not an action being taken — and it says *warn* rather
             than *show* on purpose: the row badge and the Problems list are
             unaffected, so nothing here stops being visible, it only stops
             interrupting. -->
        <label class="suppress">
          <input type="checkbox" onchange={onSuppress} />
          Don't warn me again
        </label>
      {/if}

      {#if !blocking && onDismiss}
        <button type="button" class="close" title="Dismiss" aria-label="Dismiss" onclick={onDismiss}>
          <Glyph kind="cross" />
        </button>
      {/if}
    </div>
  </div>

  {#if details && detailsOpen}
    <pre class="raw selectable">{details}</pre>
  {/if}
</div>

<style>
  /* Sits between the title bar and the panes as a sibling of both, so it
     pushes the layout down rather than overlaying it. An overlay would cover
     repository rows, and §14.1's rule that nothing may cost a row of live data
     is about more than the mascot. */
  .banner {
    flex: 0 0 auto;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: var(--space-2) var(--space-3);
    background: var(--bg-raised);
    border-bottom: 1px solid var(--status-error);
    /* A left edge in the status colour rather than a tinted fill: the fill
       would have to be a red wash across the full window width, which reads as
       far more alarm than a failed fetch has earned. */
    box-shadow: inset 3px 0 0 var(--status-error);
  }

  .banner.blocking {
    border-bottom-color: var(--status-conflict);
    box-shadow: inset 3px 0 0 var(--status-conflict);
  }

  /* Wraps, and only under duress. §13 wants the headline, the action and
     *Details* on one line, and at any ordinary window width that is what this
     is — but "one line" was being held by making the row *unable* to fit,
     which under `body { overflow: hidden }` put the rightmost control (the
     Dismiss cross) past the window edge with no scrollbar to reach it. A row
     that breaks is the graceful end of that; a row that cannot be dismissed
     is not. */
  .row {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  /* Named before the message, and clickable: §5.1's error badge points here
     rather than rendering its own copy of the notice, and this is the trip
     back — the repo whose push failed is one click from being the selected
     one. */
  .repo {
    /* Shrinkable, unlike the actions beside it: when the banner runs out of
       room the name is the part that can lose characters and still do its job
       — the ellipsis leaves enough of it to disambiguate a row, whereas half
       a Dismiss button is nothing. */
    flex: 0 1 auto;
    min-width: 0;
    max-width: 220px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    padding: 0;
    border: 0;
    background: none;
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--text-primary);
  }

  .repo:not(.static):hover {
    text-decoration: underline;
  }

  .message {
    /* `1 1 0`, not `1 1 auto`. An `auto` basis is the message's max-content
       width, and in a wrapping row that is a demand: an untranslated stderr
       would throw itself onto a line of its own in a 1400px window. A zero
       basis asks for nothing and takes the slack, so the row breaks only when
       the parts that *cannot* shrink stop fitting. */
    flex: 1 1 0;
    min-width: 0;
    margin: 0;
    /* Three lines then an ellipsis, rather than one line then an ellipsis.
       Truncating at one line was the wrong half of the trade: the messages
       long enough to need it are the untranslated ones, where git's own
       wording is all there is to go on. Three still bounds the banner — this
       is chrome above a list whose rows are the point (§14.1) — and `title`
       carries the rest, as does Recent Problems. */
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    overflow: hidden;
    /* A remote URL in stderr is one unbreakable word and would otherwise set
       the banner's floor width all by itself. */
    overflow-wrap: anywhere;
    font-size: var(--text-sm);
    color: var(--status-error);
  }

  .banner.blocking .message {
    color: var(--status-conflict);
  }

  .actions {
    display: flex;
    align-items: center;
    flex: 0 0 auto;
    /* Redundant while the message is taking the slack beside them, load-
       bearing once the row has wrapped and they are alone on the second. */
    margin-left: auto;
    gap: var(--space-2);
  }

  .actions button {
    height: 24px;
    padding: 0 var(--space-2);
    font-size: var(--text-xs);
    color: var(--text-primary);
    background: var(--bg-hover);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
  }

  .actions button:hover {
    background: var(--bg-active);
  }

  /* The accent is for selection and primary actions (§11), and *Abort merge*
     is the primary way out of the one state that blocks commit and push. */
  .actions button.primary {
    color: var(--text-primary);
    background: var(--accent);
    border-color: var(--accent);
  }

  .actions button.primary:hover {
    background: var(--accent-hover);
    border-color: var(--accent-hover);
  }

  .actions button.link {
    height: auto;
    padding: 0;
    border: 0;
    background: none;
    color: var(--text-muted);
  }

  .actions button.link:hover {
    background: none;
    color: var(--text-primary);
  }

  .actions button.close {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    padding: 0;
    color: var(--text-muted);
  }

  .actions button.close:hover {
    color: var(--text-primary);
  }

  .suppress {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    font-size: var(--text-xs);
    color: var(--text-muted);
    white-space: nowrap;
    cursor: pointer;
  }

  .suppress:hover {
    color: var(--text-secondary);
  }

  .raw {
    margin: 0;
    padding: var(--space-2);
    max-height: 200px;
    overflow: auto;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    white-space: pre-wrap;
    word-break: break-word;
    color: var(--text-muted);
    background: var(--bg-app);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }
</style>
