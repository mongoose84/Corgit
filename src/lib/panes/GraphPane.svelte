<script lang="ts">
  import Pane from './Pane.svelte';
  import EmptyState from '../EmptyState.svelte';

  // Lane layout and virtualized rows arrive in build step 6 (SPEC.md §5.3):
  // SVG lanes plus DOM rows, 300 commits at a time.
</script>

<Pane title="Graph">
  <EmptyState message="No repository selected" hint="Select a repository to see its history" />
</Pane>
