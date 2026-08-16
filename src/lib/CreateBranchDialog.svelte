<script lang="ts">
  // Create Branch (§8.3) — opened from a ref badge's or a commit row's
  // right-click menu in the graph. A centred modal rather than an anchored
  // popover like `ContextMenu`/`Popover`: those close on the next outside
  // mousedown, which is right for a menu and wrong for a form you are halfway
  // through typing into.
  import { validateBranchName } from './branchName';

  interface Props {
    /** What the new branch starts at, as the user sees it — a branch name or
     *  a short hash. Passed to git verbatim as the start point. */
    startPoint: string;
    /** Local branch names already in the graph, for the duplicate check. */
    existingLocal: readonly string[];
    /** Resolves to whether the branch was actually created; the dialog stays
     *  open (showing nothing itself) only until this settles — the failure is
     *  reported by the pane, alongside every other git write's. */
    onCreate: (name: string, checkout: boolean) => Promise<boolean>;
    onClose: () => void;
  }

  let { startPoint, existingLocal, onCreate, onClose }: Props = $props();

  let name = $state('');
  // Checked by default: creating a branch you do not then work on is the rarer
  // of the two intents, and unchecking is one click.
  let checkout = $state(true);
  let busy = $state(false);
  let inputEl: HTMLInputElement | undefined = $state();

  const problem = $derived(validateBranchName(name, existingLocal));
  const canCreate = $derived(!busy && name.trim().length > 0 && problem === null);

  $effect(() => {
    inputEl?.focus();
  });

  async function create() {
    if (!canCreate) return;
    busy = true;
    try {
      if (await onCreate(name.trim(), checkout)) onClose();
    } finally {
      busy = false;
    }
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.stopPropagation();
      onClose();
    } else if (event.key === 'Enter' && event.target === inputEl) {
      // Only from the name field: Enter on Cancel must still cancel, and
      // preventing its default here would swallow that activation.
      event.preventDefault();
      void create();
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
    aria-label="Create branch"
    onmousedown={(event) => event.stopPropagation()}
    onkeydown={onKeydown}
  >
    <p class="title">Create branch</p>
    <p class="from">from <span class="start-point">{startPoint}</span></p>

    <input
      bind:this={inputEl}
      bind:value={name}
      type="text"
      placeholder="Branch name"
      spellcheck="false"
      autocapitalize="off"
      autocorrect="off"
      disabled={busy}
      aria-label="Branch name"
      aria-invalid={problem !== null}
    />

    <p class="problem" class:shown={problem !== null}>{problem ?? ''}</p>

    <label class="checkout">
      <input type="checkbox" bind:checked={checkout} disabled={busy} />
      Check out after creating
    </label>

    <div class="buttons">
      <button type="button" disabled={busy} onclick={onClose}>Cancel</button>
      <button type="button" class="primary" disabled={!canCreate} onclick={create}>
        {busy ? 'Creating…' : 'Create'}
      </button>
    </div>
  </div>
</div>

<style>
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
    font-size: var(--text-sm);
    color: var(--text-muted);
  }

  .start-point {
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
    margin: var(--space-1) 0 0;
    font-size: var(--text-xs);
    color: var(--status-conflict);
    visibility: hidden;
  }

  .problem.shown {
    visibility: visible;
  }

  .checkout {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: var(--space-2) 0 var(--space-3);
    font-size: var(--text-sm);
    color: var(--text-secondary);
  }

  .checkout input {
    accent-color: var(--accent);
  }

  .buttons {
    display: flex;
    gap: var(--space-2);
  }

  /* Same button treatment as the compose pane's Commit/Push pair (§11). */
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
