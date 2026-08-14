<script lang="ts">
  import Pane from './Pane.svelte';

  // Mode A (working tree) is build step 4; Mode B (commit details) is step 7.
  // See SPEC.md §5.2 — this pane is modal on graph selection.
  let message = $state('');
</script>

<Pane title="Changes">
  <div class="compose">
    <textarea
      bind:value={message}
      placeholder="Commit message"
      rows="3"
      disabled
      aria-label="Commit message"
    ></textarea>

    <div class="buttons">
      <button class="primary" disabled>Commit</button>
      <button disabled>Push</button>
    </div>

    <div class="secondary-buttons">
      <button disabled>Fetch</button>
      <button disabled>Pull</button>
    </div>
  </div>

  <div class="section">
    <span class="section-title">Staged Changes</span>
    <span class="count">0</span>
  </div>
  <p class="section-empty">Nothing staged</p>

  <div class="section">
    <span class="section-title">Changes</span>
    <span class="count">0</span>
  </div>
  <p class="section-empty">No changes</p>
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

  .buttons,
  .secondary-buttons {
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

  button:disabled {
    color: var(--text-disabled);
    cursor: default;
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

  .count {
    font-size: var(--text-xs);
    color: var(--text-disabled);
    font-variant-numeric: tabular-nums;
  }

  .section-empty {
    margin: 0;
    padding: 0 var(--space-3) var(--space-2);
    font-size: var(--text-sm);
    color: var(--text-disabled);
  }
</style>
