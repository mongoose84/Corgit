# Corgit — mascot brief

The decision record for the dog: who he is, where he is allowed to appear, and what has
been drawn so far. The first round of artwork now exists — a single illustrated contact
sheet, sliced into per-pose PNGs in [mascot/](mascot/) — so this document describes what
was made as much as what is still missing.

Naming rationale and the placement rule live in [SPEC.md](SPEC.md) §14 and §14.1. This
document expands them; where the two disagree, SPEC wins.

---

## 1. The character

A Pembroke Welsh Corgi. His job is herding a folder of git repositories: dozens of them,
about five active at a time, and his whole purpose is knowing which ones need attention.

**He is:** attentive, cheerful, industrious, slightly derpy, unbothered by bad news.
Competent without being slick — a working dog that enjoys the work.

**He is not:** cool, edgy, sarcastic, corporate, cute-for-its-own-sake, or sad. He never
scolds the user and he is never the bearer of alarm; when something has gone wrong he is
sympathetic, not worried.

The tone target is TunnelBear: the mascot is expected to appear often and without
embarrassment, and the brand is willing to look a little silly. What makes that survivable
in a dense three-pane app is the placement rule below, not restraint in the drawing.

## 2. Where he may appear

The rule, in full in SPEC §14.1: **dead space and dead time, never over live data.** Density
is the product's promise, so the dog never costs a row of repositories, files or commits.

Permitted: app identity, empty states, transitions, failure notices, and the all-clean
resting state. Not permitted: inside repository rows, file rows, graph rows, or the commit
info panel. The single exception is a mascot glyph used as a *button* icon where the verb
fits (Fetch), because that reads as chrome rather than decoration.

## 3. Style

An earlier draft of this brief specified flat vector: no strokes, four fills, everything
from the app's design tokens. The drawn mascot is not that, and the drawn mascot wins.
He is illustrated — a dark tapering outline, warm fur with soft shading, and a hand-drawn
looseness that the flat treatment could not carry at this size. Anything added to the set
has to sit next to the poses that exist, so match those.

| | |
| --- | --- |
| Medium | Illustrated line-and-shade. Dark outline of varying weight around every shape, soft shading inside it |
| Format | PNG, cut from the contact sheet. Each pose also exists as `*-alpha.png` with the paper, halo and floor shadow removed |
| Backdrop | Dark (`--bg-app`, `#141517`) is the normal case; the icon and README uses must also hold on white. Both are checked — see §7 |
| Floor | Must still read at **24px**. The full poses do not survive that; the app mark (head crop) and the mini indicators are what small sizes get |

The collar is the one piece of the original app mark that survived into the character: a
**merge node hanging off its line**, drawn on the bandana. It should stay recognisable as
such in anything new.

### Palette

The artwork carries its own palette. It is close to the app's but not drawn from
`src/app.css`, and the difference is visible if you put them side by side — the fur is a
deeper orange than `--lane-5` (`#ffa657`) and the bandana a desaturated teal rather than
`--lane-2` green.

| Role | Value |
| --- | --- |
| Fur | `#D08A3C` |
| Markings — muzzle, blaze, chest, paws, inner ear | `#F4F1ED` |
| Outline, eyes, nose | `#1F2937` |
| Success badge | `#3BA16E` |
| Error accent | `#E15D44` |
| Muted — motion lines, halo, shadow | `#9CA3AF` |

Two consequences worth knowing. The mascot will not follow a future light theme, because
its colours are baked into raster; and the visual tie between the dog's fur and the commit
graph's lane colours — the original reason for pulling from `--lane-*` — no longer holds.
Both were accepted when this artwork was.

## 4. The artwork

| | |
| --- | --- |
| Source | [mascot/corgit.png](mascot/corgit.png) — one 1536×1024 contact sheet with every cell on it |
| Slices | [mascot/](mascot/) — 23 PNGs, plus a README listing each one |
| App assets | `src/lib/mascot/` — the seven the app imports, copied out of the slices |
| Icon source | `src-tauri/icon-source.png` — 512×512, the app mark on a square canvas |
| Script | `scripts/extract-mascot.py` — writes all three from the sheet |

The slices are exact pixels off the sheet, never redrawn, so re-running the script is
always safe:

```sh
python scripts/extract-mascot.py          # then: npm run icon
```

**It is not currently safe, and this is not why.** On Pillow 12.3 / numpy 2.5 a full run
regenerates `pose-resting-alpha.png` with the knock-out eaten through the cream markings —
muzzle, chest and paws come back transparent, alpha coverage drops from 40,697 px to
35,770, and the pose that ships is the broken one. Only resting fails, which fits: it is
the marginal case §4's last paragraph warns about, so a shift in `ImageDraw.floodfill`'s
threshold semantics reaches it first. Diff the app assets after any full run until this is
fixed. `--eyes` is unaffected — it never re-cuts from the sheet.

The app imports its copies rather than reaching into `docs/`, so shipping code never
depends on the documentation tree. They stay at the slices' native resolution — the largest
is 382px wide against a 150px largest on-screen use, so they are already the 2× asset.

The `*-alpha.png` variants have the paper background, the decorative halo behind the seated
poses and the floor shadow knocked out. That cut has to flood in from the border and spread
only through neutral greys: the cream markings sit within ~10 of the paper colour, so any
threshold that removes the paper also removes the dog's muzzle. If you re-cut the artwork
by some other route, check the muzzle and paws first — that is where it fails.

## 5. The set

Priority 1 is what v1 needs to ship, and it is drawn. Priority 2 can follow.

### Priority 1 — drawn

| # | Name | File | Appears | Conveys |
| --- | --- | --- | --- | --- |
| 1 | **Resting** | `pose-resting` | Graph pane, no repo selected ("Nothing to herd") | Awake, waiting, mildly attentive. The default pose; everything else is a variation on it |
| 2 | **Content** | `pose-content` | Every repo clean and in sync | Satisfied, lying down, work finished. The app's reward state. Was drawn with a green check badge overlapping it; trimmed, because all three placements sit beside copy or counts already saying the same thing |
| 3 | **Working** | `pose-working` | During a fetch or pull sweep | Trotting, stick in mouth, motion lines behind. Held for an indefinite duration |
| 4 | **Sorry** | `pose-sorry` | `GitErrorNotice`: conflict, auth failure, no upstream | Apologetic and sympathetic, scribble of confusion overhead. Does **not** read as alarmed — its whole job is softening git's worst moments |
| 5 | **App mark** | `app-mark` | Icon, taskbar, tray, installer, favicon | Head-only crop on a dark disc. Also drawn as rounded tiles (dark and light), a small-size treatment, and two single-colour monochromes |

### Also drawn — mini indicators

Not in the original brief and worth keeping: four 85–119px versions of poses 1–4
(`mini-resting`, `mini-content`, `mini-working`, `mini-sorry`), simplified enough to read
small. They are still bound by §2 — small enough to sit in a status area is not permission
to put the dog in a repository row.

### Where they are wired

`Mascot.svelte` takes a pose and a height; everything below goes through it. The two
resting placements also pass `gaze`, which is §6.1.

| Pose | Component | Shown when |
| --- | --- | --- |
| Resting + gaze, 150px | `Welcome.svelte` | The first screen, above the wordmark. Standing in for the greeting pose until that is drawn — sitting up waiting for something to herd is what this screen means anyway. The one place with nothing to compete with, so he gets the most room |
| Resting + gaze, 132px | `panes/GraphPane.svelte` | No repo selected, and something in the herd still needs attention |
| Content, 112px | `panes/GraphPane.svelte` | No repo selected and `repos.allClean` — every repo swept, clean, and neither ahead nor behind |
| Content, 75px | `panes/CommitPane.svelte` | A repo is selected, its file list has been read, and both totals are zero, sat at the foot of the pane. The same payoff state as the row above, reachable while you are actually working rather than only with nothing selected. Smaller than the 112px, not bolder: this one shows up every time you select a clean repo, which in a herd of mostly-clean repos is often, and it shares the pane with the sections above it rather than owning the whole of one. Well under the 111px pixel-crisp ceiling at 200% scaling, so unlike the earlier 128px it is a downscale |
| Mini working, 18px | `panes/RepoList.svelte` | A status sweep is in flight; the pane header's timing readout takes the slot back when it lands |
| Mini sorry, 20px | `GitErrorNotice.svelte` | Any git failure, wherever the notice is used. Kept small because the narrowest host is the 240px commit pane |
| App mark, 20px | `TitleBar.svelte` | Always, at the left end of the combined title bar (SPEC.md §4.1). Below the 24px floor above, which is the case the head crop exists for — the taskbar and the window caption have been rendering the same artwork at 16px all along. It became a UI placement only when the window's caption stopped being drawn by Windows and started being ours |
| App mark | — | Also the source for `src-tauri/icon-source.png` and the bundled icon set |

`allClean` is deliberately strict — a sweep in flight, a failed status, or a repo not yet
swept all read as *not known to be clean*, because a dog lying down over stale data is the
one way this state can lie.

The two poses that are drawn but not yet placed are **Working** (full size) and the three
remaining minis. Working has no home because the sweep indicator is a 18px slot in a pane
header; if a full-pane fetch or first-scan state ever appears, that is where he goes.

### Priority 2 — not yet drawn

| # | Name | Appears | Must convey |
| --- | --- | --- | --- |
| 6 | **Greeting** | Welcome screen, before a root is chosen | Inviting, larger, more elaborate. First thing any new user sees, so it can afford the most detail |
| 7 | **Searching** | Filter matches nothing | Nose down, hunting. Not confused — he is on the job |
| 8 | **Waiting at the door** | Update available | Anticipation. Leash-in-mouth energy |
| 9 | **Fetch glyph** | Fetch button in a pane header | A 14–16px **single-colour** silhouette. The one asset that should still be vector, so it can inherit `currentColor` at icon weight. The two monochrome app marks are the closest thing that exists and are a reasonable starting point |

## 6. Motion

Most poses are static, and the raster format decides how the rest can move. There are no
named groups to animate against, so a pose is a single image: ears and tail cannot be
driven independently without redrawing that pose as SVG or delivering it as frames. What is
available is whole-image transform and opacity — a slow bob, a settle on entry, a cross-fade
between two poses — and that is enough for everything currently planned.

The eyes turned out to be the exception, and §6.1 is why.

The conventions that still hold:

- **Motion is idle, infrequent and non-looping-looking.** A mascot that moves constantly
  reads as broken, and in an empty pane it becomes the loudest thing on screen.
- **Periods should share no common factor**, so separate animations never lock into a
  visible repeating pattern.
- **`prefers-reduced-motion: reduce` must hold everything still.** Idle decoration is exactly
  what that setting exists to stop. The mascot still renders; it just stops moving.

Working (pose 3) is the one that wants real animation, since it covers an indefinite wait.
Sliding the whole image with the drawn motion lines carries it; if that proves too thin,
that pose is the first candidate for a frame sequence.

### 6.1 The idle gaze

Resting's eyes wander. It is the one piece of motion here that moves a *part* of the dog,
and the way round the raster problem is that the part was already a separate object in the
drawing: each pupil is a closed dark oval on a pale socket, so it can be cut out rather
than redrawn. `extract-mascot.py --eyes` writes an eyeless base with the socket filled in
sclera plus one small sprite per pupil, and `Mascot.svelte` lays the sprites back over the
base inside a `clip-path` shaped like the eye opening. Nothing is redrawn, and with
`prefers-reduced-motion` the pupils sit exactly where they were drawn — the rig at rest is
the original artwork.

That construction is lifted from Lumo's cat, which does the same thing as a Lottie: every
feature its own group, pupils clipped to the eye. The difference is that we needed it for
two objects rather than forty, so it is two `<img>` tags and a keyframe instead of a
player. **Do not add a Lottie runtime for this.** Its SVG renderer rewrites the whole
group tree every animation frame, and the state this decorates is the one a dashboard can
sit in for hours — it would spend CPU continuously, in the empty pane, on a dog. A
compositor-only `transform` costs nothing between keyframes.

**What it cannot do, measured.** The pupil nearly fills the opening in this drawing:
14×18 source px inside roughly 20×18. That leaves ±3 source px of horizontal travel and
none worth having vertically, which at the 150px welcome render is **±1.4 CSS px** on a
9px-wide eye. The tell is not the pupil, it is the cream crescent flipping from one side
to the other. Lumo's cat gets a visibly wider look-around because its eye is ~18px on
screen against our 9, so it has both the size and the room. Getting that here is not a
tuning problem — it needs the eyes drawn larger, which is a change to the character.

Consequences worth keeping in mind:

- **It is deliberately far slower than an eye.** 37s cycle, ~1.3s per move, long unequal
  holds. A real saccade is instant, and instant is the loudest thing that can happen in an
  empty pane. This is meant to be caught out of the corner of yours, or missed entirely.
- **The geometry is measured against one file.** `EYES` in the script is in the pixel
  coordinates of `pose-resting.png`; redraw the head and the four ellipses must be measured
  again. Nothing checks this — a stale opening shows as sclera leaking onto fur.
- **Only poses in `RIGS` have eyes to move.** Asking for `gaze` anywhere else is silently
  the still pose, which is the right failure: the mascot never renders wrong, it just
  renders still.
- **No blink.** It would read better than the gaze at this size, since a closing lid is a
  contrast event rather than a 1px slide, and the sleepy sheet
  (`docs/mascot/sleepy mascot.png`, middle row) already has open, half-lidded and closed
  versions of the lying dog drawn. It wants a second pose split rather than a lid faked
  over this one; if it is built, give it a period coprime with 37 so the two never lock.

## 7. Delivery

For anything added to the set:

- Match §3 — the new pose has to sit next to the drawn ones without looking like a different
  dog. Adding it to the contact sheet and re-cutting keeps that honest.
- Register the cell in `scripts/extract-mascot.py` rather than cropping by hand, and list it
  in [mascot/README.md](mascot/README.md). Add it to that script's `APP_ASSETS` if the app
  is going to import it.
- If the mark itself is redrawn, re-run `npm run icon` after the script — it regenerates the
  bundled `.ico`/`.png` set from `src-tauri/icon-source.png`. `tauri icon` also emits
  Android and iOS folders; this project bundles NSIS only, so those get deleted.
- Check at 104px, 40px and 24px on both `#141517` and white before hand-off; most problems
  in this design only show up small.
