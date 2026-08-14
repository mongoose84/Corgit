<script lang="ts">
  import Pane from './Pane.svelte';
  import FileRow from './FileRow.svelte';
  import EmptyState from '../EmptyState.svelte';
  import { repos } from '../repos.svelte';

  // Mode A (working tree) is build step 4; Mode B (commit details) is step 7.
  // See SPEC.md §5.2 — this pane is modal on graph selection, but until the
  // graph exists (step 6) it always shows Mode A for the selected repo.
  let message = $state('');
  let busy = $state(false);

  const hasRepo = $derived(repos.selectedId !== undefined);
  const files = $derived(repos.files);

  const canCommit = $derived(
    hasRepo && !busy && message.trim().length > 0 && (files?.stagedTotal ?? 0) > 0,
  );

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
    <!-- Fetch and Pull arrive with remotes in build step 5 — icon-only and
         hover-revealed per feedback, rather than a full button row, since
         they act on the selected repo the same way the menu bar's
         Repository ▸ Fetch/Pull do (§4.1). -->
    <button type="button" class="icon-action" disabled title="Fetch" aria-label="Fetch">↻</button>
    <button type="button" class="icon-action" disabled title="Pull" aria-label="Pull">⇩</button>
  {/snippet}

  {#if !hasRepo}
    <EmptyState message="No repository selected" hint="Select a repository to stage and commit changes" />
  {:else}
    <div class="compose">
      <textarea
        bind:value={message}
        placeholder="Commit message"
        rows="3"
        disabled={busy}
        aria-label="Commit message"
      ></textarea>

      <div class="buttons">
        <button class="primary" disabled={!canCommit} onclick={doCommit}>Commit</button>
        <!-- Push / Publish branch arrives with remotes in build step 5. -->
        <button disabled>Push</button>
      </div>

      <!-- Commit and push in one step arrives with remotes in build step 5. -->
      <button type="button" class="primary wide" disabled title="Commit, then push in one step">
        Commit + Push
      </button>

      {#if repos.writeError}
        <p class="error selectable">{repos.writeError}</p>
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
  .compose {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3);
    border-bottom: 1px solid var(--border);
  }

  textarea {
    width: 100%;
    padding: var(--space-2);
    font: inherit;
    font-size: var(--text-sm);
    color: var(--text-primary);
    background: var(--bg-raised);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    resize: vertical;
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

  button.primary:not(:disabled) {
    color: var(--accent-text);
    background: var(--accent);
    border-color: var(--accent);
  }

  button.primary:hover:not(:disabled) {
    background: var(--accent-hover);
    border-color: var(--accent-hover);
  }

  button.wide {
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

  .error {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--status-error);
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
