<script lang="ts">
  import Pane from './Pane.svelte';
  import ChangesList from './ChangesList.svelte';
  import EmptyState from '../EmptyState.svelte';
  import Glyph from '../Glyph.svelte';
  import { hasConflict, needsPublish, repos } from '../repos.svelte';

  // Mode A (working tree) — SPEC.md §5.2. Commit details (Mode B) live in
  // their own panel (graph.svelte.ts + CommitInfoPanel.svelte) rather than
  // taking over this pane, so staging/commit stays visible at all times.
  //
  // The file lists are `ChangesList`. This pane keeps the compose box and the
  // two header verbs — the half that acts on the repo rather than on rows —
  // plus the loading and error states, since those replace the lists rather
  // than living inside them.
  let message = $state('');
  let busy = $state(false);

  const hasRepo = $derived(repos.selectedId !== undefined);
  const files = $derived(repos.files);
  const status = $derived(repos.selectedId ? repos.status(repos.selectedId) : undefined);
  // No upstream configured (§8.7) — "Push" becomes "Publish branch" rather
  // than a separate control, since exactly one of the two ever applies. Shared
  // with the repo row's local-only branch marking (§5.1).
  const publishable = $derived(status !== undefined && needsPublish(status));
  // §13: an unresolved merge conflict blocks commit and push for this repo
  // until it's resolved or aborted — exactly two ways out, never a third.
  //
  // The pane no longer *draws* the conflict: that is the blocking banner in
  // the app chrome, which can hold a headline and both buttons on one line at
  // window width and could not at this pane's 240px minimum (§4). What stays
  // here is the part that was always this pane's own — the guard on its two
  // buttons. Via the shared predicate, so the banner and the disabled Commit
  // cannot disagree about whether this repo is wedged.
  const conflicted = $derived(status !== undefined && hasConflict(status));

  const canCommit = $derived(
    hasRepo && !busy && !conflicted && message.trim().length > 0 && (files?.stagedTotal ?? 0) > 0,
  );
  const canPush = $derived(hasRepo && !busy && !conflicted);

  async function doCommit() {
    if (!canCommit) return;
    busy = true;
    try {
      if (await repos.commit(message)) message = '';
    } finally {
      busy = false;
    }
  }

  async function doFetch() {
    busy = true;
    try {
      await repos.fetch();
    } finally {
      busy = false;
    }
  }

  async function doPull() {
    busy = true;
    try {
      await repos.pull();
    } finally {
      busy = false;
    }
  }

  async function doPush() {
    busy = true;
    try {
      await (publishable ? repos.publish() : repos.push());
    } finally {
      busy = false;
    }
  }

  async function doCommitAndPush() {
    if (!canCommit) return;
    busy = true;
    try {
      if (await repos.commitAndPush(message)) message = '';
    } finally {
      busy = false;
    }
  }
</script>

<Pane title="Changes">
  {#snippet actions()}
    <!-- Icon-only and hover-revealed per feedback, rather than a full button
         row, since they act on the selected repo the same way the menu bar's
         Repository ▸ Fetch/Pull do (§4.1). -->
    <button
      type="button"
      class="icon-action"
      disabled={!hasRepo || busy}
      title="Fetch"
      aria-label="Fetch"
      onclick={doFetch}
    >↻</button>
    <button
      type="button"
      class="icon-action"
      disabled={!hasRepo || busy}
      title="Pull"
      aria-label="Pull"
      onclick={doPull}
    >⇩</button>
  {/snippet}

  {#if !hasRepo}
    <EmptyState message="No repository selected" hint="Select a repository to stage and commit changes" />
  {:else}
    <div class="compose">
      <div class="message-field">
        <textarea
          bind:value={message}
          placeholder="Commit message"
          rows="3"
          disabled={busy}
          aria-label="Commit message"
        ></textarea>
        {#if message.length > 0}
          <button
            type="button"
            class="clear-message"
            disabled={busy}
            title="Clear commit message"
            aria-label="Clear commit message"
            onclick={() => (message = '')}
          >
            <Glyph kind="cross" />
          </button>
        {/if}
      </div>

      <div class="buttons">
        <button class="primary" disabled={!canCommit} onclick={doCommit}>Commit</button>
        <button disabled={!canPush} onclick={doPush}>
          {publishable ? 'Publish branch' : 'Push'}
        </button>
      </div>

      <button
        type="button"
        class="primary wide"
        disabled={!canCommit}
        title="Commit, then push in one step"
        onclick={doCommitAndPush}
      >
        Commit + Push
      </button>
    </div>

    {#if repos.loadingFiles && !files}
      <EmptyState message="Reading repository…" />
    {:else if repos.filesError}
      <EmptyState message="Could not read this repository" hint={repos.filesError} />
    {:else if files}
      <ChangesList {files} bind:busy />
    {/if}
  {/if}
</Pane>

<style>
  /* `Pane`'s `.body` is left as the plain block box it ships as. It was a flex
     column while the rest-state mascot lived here — that was the only thing in
     the pane that needed to know how tall the pane is — and with it gone the
     `flex-shrink: 0` guard every sibling needed goes too. Normal flow already
     lets `.body` scroll, which is what that guard was protecting. */

  .compose {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3);
    border-bottom: 1px solid var(--border);
  }

  .message-field {
    position: relative;
  }

  textarea {
    width: 100%;
    padding: var(--space-2);
    /* Room for the clear button so it never overlaps typed text. */
    padding-right: calc(var(--space-2) + 18px + var(--space-1));
    font: inherit;
    font-size: var(--text-sm);
    color: var(--text-primary);
    background: var(--bg-raised);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    resize: vertical;
  }

  .clear-message {
    position: absolute;
    top: var(--space-1);
    right: var(--space-1);
    display: flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text-muted);
  }

  .clear-message:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .clear-message:disabled {
    color: var(--text-disabled);
  }

  textarea::placeholder {
    color: var(--text-disabled);
  }

  textarea:disabled {
    color: var(--text-disabled);
  }

  textarea:focus-visible {
    border-color: var(--accent);
    outline: none;
  }

  .buttons {
    display: flex;
    gap: var(--space-2);
  }

  button {
    flex: 1 1 0;
    height: 28px;
    padding: 0 var(--space-3);
    font-size: var(--text-sm);
    color: var(--text-primary);
    background: var(--bg-raised);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    cursor: pointer;
  }

  button:hover:not(:disabled) {
    background: var(--bg-hover);
    border-color: var(--border-strong);
  }

  /* Tinted at rest rather than a solid accent fill — full brightness read as
     mismatched next to the neutral Push button. Brightens to the solid fill
     on hover, so it still reads as the primary action. */
  button.primary:not(:disabled) {
    color: var(--accent-text);
    background: var(--accent-muted);
    border-color: var(--accent);
  }

  button.primary:hover:not(:disabled) {
    color: var(--accent-text);
    background: var(--accent);
    border-color: var(--accent-hover);
  }

  button.wide {
    /* Overrides the base rule's `flex: 1 1 0` — this button is a direct flex
       item of `.compose` (a column flex container), not of `.buttons`, so
       without this it stretches to fill the leftover vertical space instead
       of staying the same height as Commit/Push. */
    flex: 0 0 auto;
    width: 100%;
  }

  button:disabled {
    color: var(--text-disabled);
    cursor: default;
  }

  /* Header icon actions (Fetch/Pull) — hover-revealed, matching the repo
     list's Fetch all (RepoList.svelte's `.icon-action`). */
  .icon-action {
    flex: 0 0 auto;
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
    font-size: var(--text-md);
    line-height: 1;
  }

  .icon-action:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .icon-action:disabled {
    color: var(--text-disabled);
  }

</style>
