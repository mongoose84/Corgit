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
    /** Short oid of the commit HEAD points at (§8.2's `branch.oid`), `null` in
     *  a repo with no commits — the row it names is drawn as "you are here". */
    headHash: string | null;
    onSelect: () => void;
    /** Double-clicking a ref badge (§8.3, §8.4) — checks out that exact
     *  branch directly, skipping any dropdown. */
    onSwitchBranch: (ref: RefBadge) => void;
    /** Right-clicking the row, or a single badge on it (§8.3, §5.2) — the
     *  badge passes just itself, so its branch entries are about that branch
     *  alone rather than every ref sharing the commit. The hash goes with it
     *  either way: *Info* is about the commit under the pointer, and a row
     *  with no badges at all still has that one entry. */
    onContextMenu: (event: MouseEvent, refs: RefBadge[], hash: string) => void;
  }

  let { row, laneCount, refs, selected, currentBranch, headHash, onSelect, onSwitchBranch, onContextMenu }: Props =
    $props();

  const cx = (lane: number) => lane * LANE_WIDTH + LANE_WIDTH / 2;
  const cy = ROW_HEIGHT / 2;

  const date = $derived(formatCommitDate(row.commit.timestamp));

  const isCurrent = (ref: RefBadge) => ref.kind === 'local' && ref.name === currentBranch;

  // Matched on the hash rather than on the current branch's ref badge: a
  // detached HEAD has no branch to match, and it is exactly the state where
  // "which commit am I on" is hardest to answer from the graph alone.
  const isHead = $derived(headHash !== null && row.commit.hash.startsWith(headHash));

  // Same lane hue as the dot and the current-branch badge, at low alpha — a
  // tint, not a second selection colour, which §11 reserves for --accent. Set
  // as a custom property rather than `background` directly so hover and
  // selection still win through the normal cascade instead of losing to an
  // inline style.
  const headTint = $derived(
    `--head-tint: color-mix(in srgb, ${laneColorVar(row.lane)} 12%, transparent);`,
  );

  // Ties the badge to the exact dot it names, rather than inventing a ninth
  // hue: same `laneColorVar` the row's own circle/lines already use (§11 —
  // never a new lane colour, and never --accent either).
  function currentBadgeStyle(lane: number): string {
    const color = laneColorVar(lane);
    return `color: ${color}; border-color: ${color}; background: color-mix(in srgb, ${color} 22%, var(--bg-raised));`;
  }

  function badgeContextMenu(event: MouseEvent, ref: RefBadge) {
    // Without this the row's own handler also runs and offers every ref on the
    // commit; right-clicking a specific badge is a statement about that one.
    event.stopPropagation();
    onContextMenu(event, [ref], row.commit.hash);
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
  class:head={isHead}
  style={isHead ? headTint : undefined}
  onclick={onSelect}
  oncontextmenu={(event) => onContextMenu(event, refs, row.commit.hash)}
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

    {#if isHead}
      <!-- A halo the lane is wide enough to hold (16 px), so the HEAD dot
           reads as bigger from across the pane without colliding with its
           neighbours or with the lines crossing the row. -->
      <circle cx={cx(row.lane)} cy={cy} r="7.5" fill={laneColorVar(row.lane)} opacity="0.25" />
    {/if}
    <circle cx={cx(row.lane)} cy={cy} r={isHead ? 5.5 : 4} fill={laneColorVar(row.lane)} />
  </svg>

  <!-- Badges sit before the message, not after it: a name that trails a
       variable-length subject lands in a different place on every row, so
       finding "where is `main`" means reading the whole column. Here they
       start at a fixed x, right beside the dot they belong to, and scanning
       branches down the graph is one straight eye path. -->
  {#each refs as ref (ref.kind + ref.name)}
    <span
      class="ref ref-{ref.kind}"
      class:current={isCurrent(ref)}
      style={isCurrent(ref) ? currentBadgeStyle(row.lane) : undefined}
      role="button"
      tabindex="-1"
      ondblclick={(event) => badgeDblclick(event, ref)}
      oncontextmenu={(event) => badgeContextMenu(event, ref)}
      title={isCurrent(ref)
        ? `${ref.name} — current branch, right-click for actions`
        : `${ref.name} — double-click to switch, right-click for actions`}
    >{ref.name}</span>
  {/each}
  <span class="subject">{row.commit.subject}</span>
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

  /* Declared before hover and selection so both still override it — being on
     HEAD is a standing fact about the row, not a transient state. */
  .graph-row.head {
    background: var(--head-tint);
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

  /* Basis 8rem, not `auto`: with `auto` the subject's basis is the *untruncated*
     message, commonly 400-800 px, which makes almost every row overflow on
     paper. Flex then hands out the shortfall in proportion to basis, so the ref
     badges gave up width alongside it even on rows with half the pane empty —
     which is why branch names were arriving pre-truncated. At a fixed basis the
     badges are laid out at their natural width first and the message grows into
     whatever is left, and the two only negotiate on rows that are genuinely
     crowded. */
  .subject {
    flex: 1 1 8rem;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-size: var(--text-sm);
    color: var(--text-primary);
  }

  /* Still shrinkable, unlike the other fixed columns: on a commit carrying five
     remote badges the message would otherwise be squeezed to nothing, and the
     row would push the date column off the edge of a narrow pane.

     Both bounds are sized against the house naming convention — `item/12345`,
     `Releses/R2026-08` — at --text-xs, where Segoe UI averages ~5.9 px a
     character and the badge adds 10 px of padding and border. 72 px holds a
     ten-character name whole, so the *floor* alone is enough for the common
     branch and the crowded-row case never truncates one. 180 px holds the
     longest shape the pair produces, `origin/Releses/R2026-08` at 23 characters
     (~146 px), with room to spare; past that a badge would start eating the
     message column to show a name nobody here writes. */
  .ref {
    flex: 0 1 auto;
    min-width: 72px;
    max-width: 180px;
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
    min-width: 88px;
    max-width: 200px;
    padding: 2px var(--space-2);
    font-size: var(--text-sm);
    font-weight: 700;
  }

  /* Right-aligned, unlike every other text column: left-aligned in a fixed
     110 px box, a short name leaves most of that box empty and the date reads
     as floating far off on its own. Ragged on the left is the cheaper edge to
     lose — the eye pairs each name with the date it sits against. */
  .author {
    flex: 0 0 auto;
    width: 110px;
    text-align: right;
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
