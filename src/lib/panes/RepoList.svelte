<script lang="ts">
  import Pane from './Pane.svelte';
  import EmptyState from '../EmptyState.svelte';

  // Populated in build step 2 (SPEC.md §16): discovery + status.
  let filter = $state('');
</script>

<Pane title="Repositories">
  <div class="filter">
    <input
      type="search"
      placeholder="Filter repositories…"
      bind:value={filter}
      disabled
      aria-label="Filter repositories by name"
    />
  </div>

  <EmptyState message="No folder open" hint="File → Open Folder…" />
</Pane>

<style>
  .filter {
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--border);
  }

  input {
    width: 100%;
    height: 26px;
    padding: 0 var(--space-2);
    font: inherit;
    font-size: var(--text-sm);
    color: var(--text-primary);
    background: var(--bg-raised);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }

  input::placeholder {
    color: var(--text-disabled);
  }

  input:disabled {
    color: var(--text-disabled);
    cursor: default;
  }

  input:focus-visible {
    border-color: var(--accent);
    outline: none;
  }
</style>
