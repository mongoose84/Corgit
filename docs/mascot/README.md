# Mascot sheet, sliced

Everything here was cut out of the single contact sheet, [corgit.png](corgit.png), by
`scripts/extract-mascot.py`. Nothing was redrawn — these are exact pixels off the sheet, so
regenerating is safe:

```sh
python scripts/extract-mascot.py
```

The same run copies the seven poses the app imports into `src/lib/mascot/` and writes
`src-tauri/icon-source.png`. Both are generated — never hand-edit them, since the next run
overwrites whatever is there.

`*-alpha.png` is the same crop with the paper background, the decorative halo behind the
seated poses and the floor shadow removed. The cream markings survive that because the
knock-out floods in from the border and only spreads through neutral greys — the fur is
warm and bright, the greys are not. Use the alpha versions in the UI; keep the plain crops
as the untouched reference.

## Poses

| File | Size | Brief |
| --- | --- | --- |
| `pose-resting.png` | 266×328 | Pose 1 — nothing to herd |
| `pose-content.png` | 349×222 | Pose 2 — all in sync (includes the green check badge) |
| `pose-working.png` | 382×245 | Pose 3 — fetching / pulling |
| `pose-sorry.png` | 232×295 | Pose 4 — something went wrong |

## App mark

| File | Size | Use |
| --- | --- | --- |
| `app-mark.png` | 250×246 | The mark itself, head crop on its dark disc |
| `app-mark-tile-dark.png` | 166×160 | Rounded-square tile, dark |
| `app-mark-tile-light.png` | 167×160 | Rounded-square tile, light |
| `app-mark-tile-small.png` | 77×75 | The 16–24px treatment, at sheet resolution |
| `app-mark-mono-light-on-dark.png` | 116×114 | Single-colour, light on dark |
| `app-mark-mono-dark-on-light.png` | 122×117 | Single-colour, dark on light |

The five tiles are crop-only — their backgrounds are part of the artwork, and the light
tile sits too close to the paper colour to knock out.

## Mini indicators

`mini-resting.png`, `mini-content.png`, `mini-working.png`, `mini-sorry.png` — 85–119px
wide, the compact row from the sheet.

## Palette on the sheet

`#D08A3C` fur · `#F4F1ED` markings · `#1F2937` line · `#3BA16E` ok · `#E15D44` error ·
`#9CA3AF` muted.

This is the mascot's own palette, not the app's design tokens — see
[mascot.md](../mascot.md) §3 for what that costs and why it was accepted. Adding a pose
means adding a cell to the sheet and registering it in the script, not cropping by hand.
