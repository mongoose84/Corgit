<script lang="ts">
  import Pane from './Pane.svelte';
  import FileRow from './FileRow.svelte';
  import EmptyState from '../EmptyState.svelte';
  import GitErrorNotice from '../GitErrorNotice.svelte';
  import { repos } from '../repos.svelte';

  // Mode A (working tree) — SPEC.md §5.2. Commit details (Mode B) live in
  // their own panel (graph.svelte.ts + CommitInfoPanel.svelte) rather than
  // taking over this pane, so staging/commit stays visible at all times.
  let message = $state('');
  let busy = $state(false);

  const hasRepo = $derived(repos.selectedId !== undefined);
  const files = $derived(repos.files);
  const status = $derived(repos.selectedId ? repos.status(repos.selectedId) : undefined);
  // No upstream configured (§8.7) — "Push" becomes "Publish branch" rather
  // than a separate control, since exactly one of the two ever applies.
  const needsPublish = $derived(status !== undefined && status.upstream === null);
  // §13: an unresolved merge conflict blocks commit and push for this repo
  // until it's resolved or aborted — exactly two ways out, never a third.
  const conflicted = $derived(status !== undefined && status.conflicted > 0);

  const canCommit = $derived(
    hasRepo && !busy && !conflicted && message.trim().length > 0 && (files?.stagedTotal ?? 0) > 0,
  );
  const canPush = $derived(hasRepo && !busy && !conflicted);

  function sectionLabel(shown: number, total: number): string {
    return shown === total ? `${total}` : `${shown} of ${total}`;
  }

  async function doCommit() {
    if (!canCommit) return;
    busy = true;
    try {
      if (await repos.commit(message)) message = '';
    } finally {
      busy = false;
    }
  }

  async function doMergeAbort() {
    busy = true;
    try {
      await repos.mergeAbort();
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
      await (needsPublish ? repos.publish() : repos.push());
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

  async function stagePath(path: string) {
    busy = true;
    try {
      await repos.stagePaths([path]);
    } finally {
      busy = false;
    }
  }

  async function unstagePath(path: string) {
    busy = true;
    try {
      await repos.unstagePaths([path]);
    } finally {
      busy = false;
    }
  }

  async function stageAll() {
    busy = true;
    try {
      await repos.stageAll();
    } finally {
      busy = false;
    }
  }

  async function unstageAll() {
    busy = true;
    try {
      await repos.unstageAll();
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
    {#if conflicted}
      <!-- §13: exactly two buttons, never a third — never force-anything. -->
      <div class="conflict-banner">
        <p class="selectable">This repository has a merge conflict. Commit and push are blocked until it's resolved or aborted.</p>
        <div class="conflict-actions">
          <button type="button" disabled={busy} onclick={doMergeAbort}>Abort merge</button>
          <button type="button" disabled={busy} onclick={() => repos.openInVSCode()}>Open in VS Code</button>
        </div>
      </div>
    {/if}

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
          >×</button>
        {/if}
      </div>

      <div class="buttons">
        <button class="primary" disabled={!canCommit} onclick={doCommit}>Commit</button>
        <button disabled={!canPush} onclick={doPush}>
          {needsPublish ? 'Publish branch' : 'Push'}
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

      {#if repos.writeError}
        <GitErrorNotice
          error={repos.writeError}
          onPull={doPull}
          onOpenVSCode={() => repos.openInVSCode()}
          onDismiss={() => (repos.writeError = null)}
        />
      {/if}
    </div>

    {#if repos.loadingFiles && !files}
      <EmptyState message="Reading repository…" />
    {:else if repos.filesError}
      <EmptyState message="Could not read this repository" hint={repos.filesError} />
    {:else if files}
      <div class="section">
        <span class="section-title">Staged Changes</span>
        <span class="section-actions">
          <span class="count">{sectionLabel(files.staged.length, files.stagedTotal)}</span>
          {#if files.staged.length > 0}
            <button type="button" class="link" disabled={busy} onclick={unstageAll}>
              − unstage all
            </button>
          {/if}
        </span>
      </div>
      {#if files.staged.length === 0}
        <p class="section-empty">Nothing staged</p>
      {:else}
        <ul>
          {#each files.staged as entry (entry.path)}
            <li>
              <FileRow {entry} action="unstage" disabled={busy} onToggle={() => unstagePath(entry.path)} />
            </li>
          {/each}
        </ul>
      {/if}

      <div class="section">
        <span class="section-title">Changes</span>
        <span class="section-actions">
          <span class="count">{sectionLabel(files.unstaged.length, files.unstagedTotal)}</span>
          {#if files.unstaged.length > 0}
            <button
              type="button"
              class="link"
              disabled={busy}
              onclick={stageAll}
              title="Stages every change, including any hidden by the cap above"
            >
              + stage all
            </button>
          {/if}
        </span>
      </div>
      {#if files.unstaged.length === 0}
        <p class="section-empty">No changes</p>
      {:else}
        <ul>
          {#each files.unstaged as entry (entry.path)}
            <li>
              <FileRow {entry} action="stage" disabled={busy} onToggle={() => stagePath(entry.path)} />
            </li>
          {/each}
        </ul>
      {/if}
    {/if}
  {/if}
</Pane>

<style>
  .conflict-banner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--border);
    background: var(--bg-raised);
  }

  .conflict-banner p {
    margin: 0;
    min-width: 0;
    font-size: var(--text-sm);
    color: var(--status-conflict);
  }

  .conflict-actions {
    display: flex;
    flex: 0 0 auto;
    gap: var(--space-2);
  }

  .conflict-actions button {
    height: 22px;
    padding: 0 var(--space-2);
    font-size: var(--text-xs);
    color: var(--text-primary);
    background: var(--bg-hover);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
  }

  .conflict-actions button:hover:not(:disabled) {
    background: var(--bg-active);
  }

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
    font-size: var(--text-md);
    line-height: 1;
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
     list's refresh button (RepoList.svelte's `.refresh`). */
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

  .section {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3) var(--space-1);
  }

  .section-title {
    font-size: var(--text-xs);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .section-actions {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .count {
    font-size: var(--text-xs);
    color: var(--text-disabled);
    font-variant-numeric: tabular-nums;
  }

  .link {
    flex: 0 0 auto;
    height: auto;
    padding: 0;
    border: 0;
    background: none;
    color: var(--text-muted);
    font-size: var(--text-xs);
  }

  .link:hover:not(:disabled) {
    color: var(--text-primary);
    background: none;
    border-color: transparent;
  }

  .section-empty {
    margin: 0;
    padding: 0 var(--space-3) var(--space-2);
    font-size: var(--text-sm);
    color: var(--text-disabled);
  }

  ul {
    margin: 0;
    padding: 0 0 var(--space-2);
    list-style: none;
  }
</style>
