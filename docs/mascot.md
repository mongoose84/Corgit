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

The app imports its copies rather than reaching into `docs/`, so shipping code never
depends on the documentation tree. They stay at the slices' native resolution — the largest
is 382px wide against a ~130px largest on-screen use, so they are already the 2× asset.

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
| 2 | **Content** | `pose-content` | Every repo clean and in sync | Satisfied, lying down, work finished. The app's reward state. Drawn with a green check badge overlapping it — trim that if the state already shows one |
| 3 | **Working** | `pose-working` | During a fetch or pull sweep | Trotting, stick in mouth, motion lines behind. Held for an indefinite duration |
| 4 | **Sorry** | `pose-sorry` | `GitErrorNotice`: conflict, auth failure, no upstream | Apologetic and sympathetic, scribble of confusion overhead. Does **not** read as alarmed — its whole job is softening git's worst moments |
| 5 | **App mark** | `app-mark` | Icon, taskbar, tray, installer, favicon | Head-only crop on a dark disc. Also drawn as rounded tiles (dark and light), a small-size treatment, and two single-colour monochromes |

### Also drawn — mini indicators

Not in the original brief and worth keeping: four 85–119px versions of poses 1–4
(`mini-resting`, `mini-content`, `mini-working`, `mini-sorry`), simplified enough to read
small. They are still bound by §2 — small enough to sit in a status area is not permission
to put the dog in a repository row.

### Where they are wired

`Mascot.svelte` takes a pose and a height; everything below goes through it.

| Pose | Component | Shown when |
| --- | --- | --- |
| Resting, 150px | `Welcome.svelte` | The first screen, above the wordmark. Standing in for the greeting pose until that is drawn — sitting up waiting for something to herd is what this screen means anyway. The one place with nothing to compete with, so he gets the most room |
| Resting, 132px | `panes/GraphPane.svelte` | No repo selected, and something in the herd still needs attention |
| Content, 112px | `panes/GraphPane.svelte` | No repo selected and `repos.allClean` — every repo swept, clean, and neither ahead nor behind |
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
named groups to animate against, so a pose is a single image: ears, tail and eyes cannot be
driven independently without redrawing that pose as SVG or delivering it as frames. What is
available is whole-image transform and opacity — a slow bob, a settle on entry, a cross-fade
between two poses — and that is enough for everything currently planned.

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
