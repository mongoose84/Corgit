<script lang="ts">
  import Mascot from './Mascot.svelte';
  import { repos } from './repos.svelte';
  import { settings } from './settings.svelte';

  // First run, or the saved root is gone — a renamed folder, a disconnected
  // drive. Never an empty repo list, never a crash (SPEC.md §9.1).
  const recent = $derived(settings.data.recentRoots);

  function shorten(path: string): string {
    const parts = path.split(/[\\/]/).filter(Boolean);
    return parts[parts.length - 1] ?? path;
  }
</script>

<div class="welcome">
  <div class="card">
    <!-- Standing in for the *greeting* pose, which is not drawn yet
         (docs/mascot.md §5, priority 2). Resting is the closest thing in the
         set: sitting up, waiting to be given something to herd, which is
         exactly the state this screen describes. Full height rather than the
         app mark — this is the one screen with nothing to compete with, so it
         is where he can afford the most room. -->
    <div class="brand">
      <Mascot pose="resting" height={150} />
      <h1>Corgit</h1>
    </div>

    {#if !repos.git.available}
      <!-- Git missing is a blocking first-run screen, not a failure repeated
           once per operation (§3). -->
      <p class="blocked">Corgit needs git, and there is none on your PATH.</p>
      <p class="hint selectable">Install it from https://git-scm.com/download/win, then reopen Corgit.</p>
    {:else}
      <p class="lede">Open the folder your repositories live in.</p>

      <button type="button" class="primary" onclick={() => void repos.openFolder()}>
        Open Folder…
      </button>

      {#if repos.openError}
        <p class="error selectable">{repos.openError}</p>
      {/if}

      {#if recent.length > 0}
        <h2>Recent</h2>
        <ul>
          {#each recent as root (root)}
            <li>
              <button type="button" class="recent" onclick={() => void repos.open(root)}>
                <span class="folder">{shorten(root)}</span>
                <span class="path">{root}</span>
              </button>
            </li>
          {/each}
        </ul>
      {/if}

      <p class="hint">
        Corgit looks one level down, so pick the parent folder — not a single repository.
      </p>
    {/if}
  </div>
</div>

<style>
  .welcome {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    padding: var(--space-5);
    background: var(--bg-app);
  }

  .card {
    width: 100%;
    max-width: 460px;
  }

  /* Stacked, not side by side: at this size he is a full-body pose rather
     than a glyph, and sitting him next to the wordmark would push the whole
     card off centre. */
  .brand {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-2);
  }

  h1 {
    margin: 0;
    font-size: 28px;
    font-weight: 600;
    letter-spacing: -0.01em;
  }

  .lede {
    margin: var(--space-2) 0 var(--space-4);
    color: var(--text-muted);
  }

  .primary {
    padding: var(--space-2) var(--space-4);
    border: 0;
    border-radius: var(--radius-md);
    background: var(--accent);
    color: var(--accent-text);
    font-size: var(--text-md);
    cursor: default;
  }

  .primary:hover {
    background: var(--accent-hover);
  }

  h2 {
    margin: var(--space-5) 0 var(--space-2);
    font-size: var(--text-xs);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .recent {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    width: 100%;
    padding: var(--space-1) var(--space-2);
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    text-align: left;
    cursor: default;
  }

  .recent:hover {
    background: var(--bg-raised);
  }

  .folder {
    flex: 0 0 auto;
    color: var(--text-primary);
  }

  .path {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-size: var(--text-sm);
    color: var(--text-disabled);
  }

  .hint {
    margin: var(--space-5) 0 0;
    font-size: var(--text-sm);
    color: var(--text-disabled);
  }

  .blocked {
    margin: var(--space-2) 0 var(--space-2);
    color: var(--status-error);
  }

  .error {
    margin: var(--space-3) 0 0;
    font-size: var(--text-sm);
    color: var(--status-error);
  }
</style>
