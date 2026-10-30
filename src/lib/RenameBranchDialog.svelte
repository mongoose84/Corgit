<script lang="ts">
  // Rename branch (§8.3) — opened from an unpublished local ref badge's
  // right-click menu in the graph. Built on CreateBranchDialog rather than
  // DeleteBranchDialog: a rename destroys nothing and is undone by renaming it
  // back, so it is a form with a primary button, not a question with a danger
  // one — and a stray outside click dismissing it costs only the typing.
  import { validateBranchName } from './branchName';

  interface Props {
    /** The branch being renamed, exactly as its badge names it. */
    from: string;
    /** Local branch names already in the graph, for the duplicate check. May
     *  include `from` itself — keeping the name exactly is "unchanged", and
     *  checked before the duplicate rule, but `from` must stay in the list so
     *  a case-only rename of it is caught like any other case clash. */
    existingLocal: readonly string[];
    /** Resolves to whether the rename happened. As with Create, the failure
     *  is reported by the pane alongside every other git write's. */
    onRename: (to: string) => Promise<boolean>;
    onClose: () => void;
  }

  let { from, existingLocal, onRename, onClose }: Props = $props();

  // Starts as the old name, all of it selected: a rename is usually an edit
  // of the name (a typo, a missing prefix), and typing over the selection
  // still replaces it outright when it is not.
  // svelte-ignore state_referenced_locally
  let name = $state(from);
  let busy = $state(false);
  let inputEl: HTMLInputElement | undefined = $state();

  // `from` is not filtered out of the list: `feature` → `Feature` is the one
  // rename git on Windows refuses against the branch's own name (§8.3), and
  // only the case-insensitive half of the duplicate check sees it.
  const unchanged = $derived(name.trim() === from);
  const problem = $derived(unchanged ? null : validateBranchName(name, existingLocal));
  const canRename = $derived(!busy && name.trim().length > 0 && !unchanged && problem === null);

  $effect(() => {
    inputEl?.focus();
    inputEl?.select();
  });

  async function rename() {
    if (!canRename) return;
    busy = true;
    try {
      if (await onRename(name.trim())) onClose();
    } finally {
      busy = false;
    }
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.stopPropagation();
      onClose();
    } else if (event.key === 'Enter' && event.target === inputEl) {
      // Only from the name field, as in CreateBranchDialog: Enter on Cancel
      // must still cancel.
      event.preventDefault();
      void rename();
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="scrim" role="presentation" onmousedown={onClose}>
  <div
    class="dialog"
    role="dialog"
    tabindex="-1"
    aria-modal="true"
    aria-label="Rename branch"
    onmousedown={(event) => event.stopPropagation()}
    onkeydown={onKeydown}
  >
    <p class="title">Rename branch</p>
    <p class="from"><span class="old-name selectable">{from}</span></p>

    <input
      bind:this={inputEl}
      bind:value={name}
      type="text"
      placeholder="New branch name"
      spellcheck="false"
      autocapitalize="off"
      autocorrect="off"
      disabled={busy}
      aria-label="New branch name"
      aria-invalid={problem !== null}
    />

    <p class="problem" class:shown={problem !== null}>{problem ?? ''}</p>

    <div class="buttons">
      <button type="button" disabled={busy} onclick={onClose}>Cancel</button>
      <button type="button" class="primary" disabled={!canRename} onclick={rename}>
        {busy ? 'Renaming…' : 'Rename'}
      </button>
    </div>
  </div>
</div>

<style>
  /* The same box as CreateBranchDialog, minus the checkout checkbox — the
     two read as one family of branch forms. */
  .scrim {
    position: fixed;
    inset: 0;
    /* Above ContextMenu's 100, since the menu that opened this dialog is
       still unwinding its own close when the dialog first paints. */
    z-index: 200;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.45);
  }

  .dialog {
    display: flex;
    flex-direction: column;
    width: 320px;
    padding: var(--space-3);
    background: var(--bg-raised);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5);
  }

  .title {
    margin: 0;
    font-size: var(--text-md);
    font-weight: 600;
    color: var(--text-primary);
  }

  .from {
    margin: var(--space-1) 0 var(--space-3);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--text-sm);
    color: var(--text-muted);
  }

  .old-name {
    font-family: var(--font-mono);
    color: var(--text-secondary);
  }

  input[type='text'] {
    width: 100%;
    height: 28px;
    padding: 0 var(--space-2);
    font: inherit;
    font-size: var(--text-sm);
    color: var(--text-primary);
    background: var(--bg-surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }

  input[type='text']:focus-visible {
    border-color: var(--accent);
    outline: none;
  }

  input[type='text'][aria-invalid='true'] {
    border-color: var(--status-conflict);
  }

  input[type='text']::placeholder {
    color: var(--text-disabled);
  }

  /* Always in the layout, only sometimes visible — otherwise typing a bad
     character makes the whole dialog jump. */
  .problem {
    min-height: 16px;
    margin: var(--space-1) 0 var(--space-3);
    font-size: var(--text-xs);
    color: var(--status-conflict);
    visibility: hidden;
  }

  .problem.shown {
    visibility: visible;
  }

  .buttons {
    display: flex;
    gap: var(--space-2);
  }

  .buttons button {
    flex: 1 1 0;
    height: 28px;
    padding: 0 var(--space-3);
    font-size: var(--text-sm);
    color: var(--text-primary);
    background: var(--bg-surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    cursor: pointer;
  }

  .buttons button:hover:not(:disabled) {
    background: var(--bg-hover);
    border-color: var(--border-strong);
  }

  .buttons button.primary:not(:disabled) {
    color: var(--accent-text);
    background: var(--accent-muted);
    border-color: var(--accent);
  }

  .buttons button.primary:hover:not(:disabled) {
    color: var(--accent-text);
    background: var(--accent);
    border-color: var(--accent-hover);
  }

  .buttons button:disabled {
    color: var(--text-disabled);
    cursor: default;
  }
</style>
