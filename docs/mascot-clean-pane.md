# Corgit — the dog in a clean commit pane

**Status: proposed, not implemented.** Design note for a change to
`src/lib/panes/CommitPane.svelte`.

Governing rules: [SPEC.md](SPEC.md) §14.1 (where the mascot may appear) and
[mascot.md](mascot.md) §2 and §5. This change needs no amendment to either — it fills a
placement both already permit.

---

## 1. The gap this closes

`content` — the lying-down payoff pose, the app's reward state — is wired to exactly one
condition today (`panes/GraphPane.svelte:229`): **no repo selected** *and* `repos.allClean`.

That means the payoff is unreachable while you are actually working. Select a clean repo
and the graph pane fills with commits, the dog goes away, and the commit pane renders two
lines of `--text-disabled` grey:

```
Staged Changes  0
  Nothing staged
Changes         0
  No changes
```

…followed by however much of the pane is left over. On a maximised window that is most of
it. Nothing occupies that space, nothing ever will, and "this repo is clean" is precisely
the moment the mascot exists to mark.

SPEC §14.1 already lists this under **Resting** — *"every repo clean and in sync: the
payoff state, dog lies down."* The state was specified; only the no-selection half of it
got built.

## 2. Trigger

Render the mascot when **all** of these hold:

| | |
| --- | --- |
| `hasRepo` | a repo is selected |
| `files` is non-null | the file list has been read at least once |
| `!repos.loadingFiles` | no read in flight |
| `repos.filesError` unset | the last read succeeded |
| `!conflicted` | §13 owns the conflict state; the banner is the message there |
| `files.stagedTotal === 0 && files.unstagedTotal === 0` | genuinely nothing to commit |

Two of these are load-bearing and easy to get wrong.

**Use the totals, not the array lengths.** `files.staged` and `files.unstaged` are capped
lists — `sectionLabel(shown, total)` at `CommitPane.svelte:33` exists because the pane
truncates. `stagedTotal`/`unstagedTotal` are the honest counts. A dog lying down over a
truncated list would be the one way this state can lie, which is the same reasoning that
made `repos.allClean` deliberately strict (mascot.md §5).

**Do not reach for `isDirty(status)`.** It reads `RepoStatus`, which comes from the sweep
cache, and the cache is never truth (§5.1) — it can be a sweep behind what this pane is
displaying. The pane should agree with the rows it just drew, so it judges from `files`.
This is the rare case where *not* reusing the shared predicate is correct; say so in a
comment, or someone will "fix" it.

Consequence worth accepting up front: the dog appears every time you select a clean repo,
which in a herd of mostly-clean repositories is often. That is the TunnelBear bargain
(SPEC §14) — he shows up without embarrassment, and §14.1 is what keeps it survivable.

### 2.1 Rejected: "fill whatever space is left"

The tempting generalisation is to show him whenever the pane has room — present alongside a
short list of changed files, gone once the list is long enough to fill the pane. It is
within the letter of §14.1, and it was considered and dropped for two reasons.

**The pose makes a claim.** `content` is the finished pose — lying down, work done, drawn
with a green check badge. Under a list of five uncommitted files, the artwork says "nothing
to do" while the pane above it says "five things to do". mascot.md §1 allows him to be
derpy and unbothered by bad news; it does not allow him to be wrong.

**The disappearance would have no cause.** A space-based trigger makes the dog vanish when
you drag the pane divider or resize the window — inputs that have nothing to do with the
repository. Decoration that comes and goes on unrelated input reads as a rendering bug
rather than a personality. The zero-file rule has legible cause and effect in both
directions: he arrives when you commit, and leaves the moment you touch a file. Both are
things the user did on purpose.

A third, more mundane problem: `Pane.svelte`'s `.body` scrolls. Under a space-based rule
he ends up below the fold on a medium-length list, and nobody scrolls to find a corgi.

**If more dog is wanted later**, the honest version is a different pose rather than a
different trigger. `resting` means awake and waiting, not finished, so it makes no false
claim above a short list of changes. If that is ever built, key it to a fixed file count
(≤ 3, say) rather than to measured space — discrete and predictable, and it does not move
when a divider does.

## 3. Placement in the markup

Inside the `{:else if files}` branch, **after** the closing `{/if}` of the unstaged section
(currently `CommitPane.svelte:380`) and before that branch's own `{/if}`. It is a sibling
of the two sections, not nested in either — it reports on both.

Keep the existing `Nothing staged` / `No changes` lines. They carry the meaning; the dog is
decoration and is `aria-hidden` (`Mascot.svelte`). Removing them would put semantic weight
on an image.

## 4. Layout — the part that actually needs thought

`Pane.svelte`'s `.body` is a plain block box with `overflow-y: auto`. Its children stack in
normal flow, so **nothing in the commit pane currently knows how tall the pane is.** A
mascot appended after the sections lands directly under "No changes" with a void below it,
which looks like a rendering bug rather than a rest state.

Two ways out.

### Option A — centre it in the leftover space (recommended)

`Pane.svelte` already exposes a `class` prop documented as *"a styling hook for a
caller-owned `:global()` rule"*. This is the case it was put there for.

1. `<Pane title="Changes" class="commit-pane">`
2. In `CommitPane.svelte`'s style block: `:global(.commit-pane .body) { display: flex;
   flex-direction: column; }`
3. Wrap the mascot in a `.rest` div with `flex: 1 1 auto` and flex-centre its contents.

**The gotcha that will bite:** turning `.body` into a flex column makes every existing
child a flex item, and flex items default to `flex-shrink: 1`. In a scrolling container
that lets the file `<ul>` compress below its content height instead of scrolling. Every
sibling — `.conflict-banner`, `.compose`, `.section`, `.section-empty`, `ul`,
`.selection-bar` — needs `flex-shrink: 0`. Miss one and it only shows up on a repo with
enough files to overflow, which is not the repo you will be testing on.

### Option B — fixed top margin, no flex

Append the mascot with a large `margin-top` (`var(--space-5)` × 2 or so) and centre it
horizontally only. Touches no other rule and cannot squash anything.

Worse on a tall window — he sits high with dead space beneath — but it is ten lines and
carries no regression risk. A reasonable first cut if Option A's shrink audit looks like
more than it is worth.

## 5. Pose and size

**Pose:** `content`. Already imported by `Mascot.svelte`, and already means exactly this.
It ships drawn with a green check badge overlapping it (mascot.md §5); since the two `0`
counts beside "Staged Changes" and "Changes" already say the same thing, check whether the
badge reads as redundant here.

**Size: 128px**, against the 112px the same pose gets in the graph pane. Deliberately
bolder — this is a payoff state and it should feel like one — but the two numbers it sits
between are both real.

*The upper bound is the artwork.* `content.png` is **349 × 222** native, and mascot.md §3
notes the slices are already the 2× asset. 222 ÷ 2 = 111, so the graph pane's 112px is
precisely the largest render that is still pixel-crisp on a 200%-scaled display — which is
most Windows laptops. 128px is a 1.28× upscale there. That is safe for this artwork
specifically: it is soft-shaded illustration with a hand-drawn outline, which tolerates
mild upscaling far better than crisp vector or pixel art would. Around 180px it starts to
look mushy, and there is no way to fix that by asking for a bigger number — the slices are
exact crops from a 1536×1024 contact sheet, so more pixels means regenerating the sheet at
higher resolution and re-cutting.

*The tighter bound is width.* `content` is the widest, shortest pose in the set at 1.57:1,
so 128px tall is **201px wide**. `App.svelte:19` sets `MIN_MIDDLE = 240`, and the default
middle pane is 20% of usable width (`DEFAULT_PANE_WIDTHS` in `settings.svelte.ts:31`) —
roughly 290px at a 1440px window, 380px at 1920px. Comfortable at the default, snug at the
minimum. **Check it at 240px, not on a dragged-wide pane.**

Because of that, add a shrink guard. `Mascot.svelte` sets a fixed height with `width: auto`,
so it cannot respond to a narrow pane at all today — at `MIN_MIDDLE` the dog will sit
within ~20px of each edge and clip if anything shifts. In the `.rest` wrapper:

```css
.rest :global(img) {
  max-width: 100%;
  height: auto;
}
```

This lets him scale down with the pane instead of overflowing it, and is a no-op at any
width where 201px fits. Prefer it over pushing the size logic into `Mascot.svelte`, whose
height-only API is the thing that keeps the poses looking like one set.

**Copy:** one line under the dog, `--text-muted`, in the voice of "All in sync" / "Nothing
to herd" rather than a status report. `Nothing to commit` is the plain option; `Working
tree clean` is git's phrasing and reads more technical than the dog does. Avoid a second
hint line — `EmptyState`'s two-line shape is for panes that need to tell you what to do
next, and here there is nothing to do.

`EmptyState.svelte` itself is the wrong component to reuse: it is `height: 100%` and centres
against the whole pane, which would fight the sections above it. Write the small block
inline.

## 6. Motion and accessibility

Nothing to add. `content` is a still pose, and `Mascot.svelte` already sets `alt=""`,
`aria-hidden`, `pointer-events: none` and honours `prefers-reduced-motion`. Do not animate
it — mascot.md §6 reserves motion for waits of unknown length, and a finished state is the
opposite of that.

## 7. Update in the same change

- **`docs/mascot.md` §5, "Where they are wired"** — add the row: `Content, 128px |
  panes/CommitPane.svelte | A repo is selected, its file list has been read, and both
  totals are zero`. Worth also amending §3's "largest on-screen use is ~130px", which this
  change makes exactly true rather than approximately.
- **`docs/SPEC.md`** — no change. §14.1 already permits it; §5.2 describes the pane's
  sections and does not enumerate its empty states.

No frontend test is needed: `src/lib/*.test.ts` covers logic modules only, and there is no
component-test harness in the project. `npm run check` is the gate.

Check it in the running app against four cases, in this order — the first two are the
feature, the last two are the regression risk from §4:

1. A clean repo — dog centred in the space below "No changes".
2. A repo with two changed files — no dog, and no layout shift versus today.
3. A repo with ~200 changed files — list scrolls, nothing squashed.
4. The middle pane dragged to `MIN_MIDDLE` — dog scales down, does not clip or overflow.

`bash scripts/make-demo-root.sh` builds a root with dirty and clean repos in it, which
covers 1 and 2 directly.

## 8. Ruled out, on purpose

**The commit info panel.** SPEC §14.1 names it in the not-permitted list, and that stays.
Its content — author, date, hash, changed files — is a dense readout the user opened
deliberately, and its only art-shaped moments are `Reading commit…` and the error state.
`EmptyState.svelte`'s own comment settles those: art is *"never the loading or error ones,
where art would compete with the thing the user actually needs to read."*

The distinction that separates it from this proposal: the commit pane's empty region is
space that will never hold anything, in a state the user should feel good about. The info
panel's is space that is about to hold what they asked for.
