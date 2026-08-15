<script lang="ts">
  import type { RowLayout } from '../graphLayout';
  import { laneColorVar, ROW_HEIGHT, LANE_WIDTH } from '../graphLayout';
  import type { RefBadge } from '../graph.svelte';
  import { formatCommitDate } from '../dateFormat';

  interface Props {
    row: RowLayout;
    laneCount: number;
    refs: RefBadge[];
    selected: boolean;
    /** The selected repo's checked-out branch, `null` on detached HEAD — the
     *  local badge matching it renders bolder and bigger so HEAD reads at a
     *  glance instead of blending into the rest of the row's badges. */
    currentBranch: string | null;
    onSelect: () => void;
    /** Double-clicking a ref badge (§8.3, §8.4) — checks out that exact
     *  branch directly, skipping any dropdown. */
    onSwitchBranch: (ref: RefBadge) => void;
    /** Right-clicking the row: only meaningful when `refs` is non-empty, so
     *  the parent decides whether to actually open a menu. */
    onContextMenu: (event: MouseEvent, refs: RefBadge[]) => void;
  }

  let { row, laneCount, refs, selected, currentBranch, onSelect, onSwitchBranch, onContextMenu }: Props = $props();

  const cx = (lane: number) => lane * LANE_WIDTH + LANE_WIDTH / 2;
  const cy = ROW_HEIGHT / 2;

  const shortHash = $derived(row.commit.hash.slice(0, 7));
  const date = $derived(formatCommitDate(row.commit.timestamp));

  const isCurrent = (ref: RefBadge) => ref.kind === 'local' && ref.name === currentBranch;

  // Ties the badge to the exact dot it names, rather than inventing a ninth
  // hue: same `laneColorVar` the row's own circle/lines already use (§11 —
  // never a new lane colour, and never --accent either).
  function currentBadgeStyle(lane: number): string {
    const color = laneColorVar(lane);
    return `color: ${color}; border-color: ${color}; background: color-mix(in srgb, ${color} 22%, var(--bg-raised));`;
  }

  function badgeDblclick(event: MouseEvent, ref: RefBadge) {
    // Otherwise this also fires the row's own onclick (harmless, since
    // selecting the row a switch is about to leave is a no-op) and the
    // browser's default text-selection behaviour on the badge label.
    event.stopPropagation();
    event.preventDefault();
    onSwitchBranch(ref);
  }
</script>

<button
  type="button"
  class="graph-row"
  class:selected
  onclick={onSelect}
  oncontextmenu={(event) => onContextMenu(event, refs)}
  title={row.commit.subject}
>
  <svg class="lanes" width={laneCount * LANE_WIDTH} height={ROW_HEIGHT} viewBox="0 0 {laneCount * LANE_WIDTH} {ROW_HEIGHT}">
    {#each row.edges as edge (edge.lane + edge.kind)}
      {#if edge.kind === 'through'}
        <line x1={cx(edge.lane)} y1="0" x2={cx(edge.lane)} y2={ROW_HEIGHT} stroke={laneColorVar(edge.lane)} />
      {:else if edge.kind === 'merge-in'}
        <line x1={cx(edge.lane)} y1="0" x2={cx(row.lane)} y2={cy} stroke={laneColorVar(edge.lane)} />
      {:else}
        <line x1={cx(row.lane)} y1={cy} x2={cx(edge.lane)} y2={ROW_HEIGHT} stroke={laneColorVar(edge.lane)} />
      {/if}
    {/each}

    {#if row.hasIncomingSameLane}
      <line x1={cx(row.lane)} y1="0" x2={cx(row.lane)} y2={cy} stroke={laneColorVar(row.lane)} />
    {/if}
    {#if row.hasOutgoingSameLane}
      <line x1={cx(row.lane)} y1={cy} x2={cx(row.lane)} y2={ROW_HEIGHT} stroke={laneColorVar(row.lane)} />
    {/if}

    <circle cx={cx(row.lane)} cy={cy} r="4" fill={laneColorVar(row.lane)} />
  </svg>

  <span class="hash">{shortHash}</span>
  <span class="subject">{row.commit.subject}</span>
  {#each refs as ref (ref.kind + ref.name)}
    <span
      class="ref ref-{ref.kind}"
      class:current={isCurrent(ref)}
      style={isCurrent(ref) ? currentBadgeStyle(row.lane) : undefined}
      role="button"
      tabindex="-1"
      ondblclick={(event) => badgeDblclick(event, ref)}
      title={isCurrent(ref) ? `${ref.name} — current branch` : `${ref.name} — double-click to switch`}
    >{ref.name}</span>
  {/each}
  <span class="author">{row.commit.author}</span>
  <span class="date">{date}</span>
</button>

<style>
  .graph-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    height: var(--row-height);
    padding: 0 var(--space-3);
    border: 0;
    background: none;
    text-align: left;
    cursor: default;
  }

  .graph-row:hover {
    background: var(--bg-hover);
  }

  .graph-row.selected {
    background: var(--accent-muted);
  }

  .lanes {
    flex: 0 0 auto;
  }

  .lanes line {
    stroke-width: 1.5;
    stroke-linecap: round;
  }

  .hash {
    flex: 0 0 auto;
    width: 56px;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--text-muted);
  }

  .subject {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-size: var(--text-sm);
    color: var(--text-primary);
  }

  .ref {
    flex: 0 0 auto;
    max-width: 140px;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    padding: 1px var(--space-1);
    font-size: var(--text-xs);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background: var(--bg-raised);
  }

  /* Neither badge kind may borrow the accent — it is reserved for selection
     and primary actions only (SPEC.md §11 rule 3). */
  .ref-local {
    color: var(--text-secondary);
  }

  .ref-remote {
    color: var(--text-muted);
    font-style: italic;
  }

  /* HEAD's own branch (§8.3) — bold and a size up from the other badges so
     "which branch am I on" reads without hunting through the row, but still
     not the accent (SPEC.md §11 rule 3: reserved for selection). */
  .ref.current {
    max-width: 180px;
    padding: 2px var(--space-2);
    font-size: var(--text-sm);
    font-weight: 700;
  }

  .author {
    flex: 0 0 auto;
    width: 110px;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-size: var(--text-sm);
    color: var(--text-muted);
  }

  .date {
    flex: 0 0 auto;
    width: 140px;
    text-align: right;
    font-variant-numeric: tabular-nums;
    font-size: var(--text-xs);
    color: var(--text-disabled);
  }
</style>
