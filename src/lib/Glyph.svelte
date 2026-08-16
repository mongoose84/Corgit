<script lang="ts">
  /**
   * A drawn `+`, `−` or `×` for an icon button.
   *
   * These three are typed as characters everywhere else and it is a mistake:
   * they are *math* glyphs, positioned on the font's math axis — about a
   * quarter of an em above the baseline — rather than centred on the em box.
   * Centring a button's line box therefore leaves the ink roughly 1.7px low,
   * which is small enough to read as a mistake rather than a style and is not
   * fixable with `align-items`. The alternatives were a per-font nudge or
   * drawing them; drawn stays centred at any size and in any font.
   *
   * Two bars, rotated. Both start horizontal so they are guaranteed the same
   * length — a cross built from one horizontal and one vertical bar is only
   * symmetrical if the box is square, and the boxes here are not all square.
   *
   * Decorative: every caller is a button that already carries an `aria-label`.
   */
  interface Props {
    kind: 'plus' | 'minus' | 'cross';
  }

  let { kind }: Props = $props();
</script>

<span class="glyph {kind}" aria-hidden="true"></span>

<style>
  .glyph {
    position: relative;
    display: block;
    width: 9px;
    height: 9px;
  }

  .glyph::before,
  .glyph::after {
    content: '';
    position: absolute;
    top: 50%;
    left: 50%;
    width: 9px;
    height: 1px;
    background: currentColor;
    transform: translate(-50%, -50%);
  }

  .plus::after {
    transform: translate(-50%, -50%) rotate(90deg);
  }

  .minus::after {
    content: none;
  }

  .cross::before {
    transform: translate(-50%, -50%) rotate(45deg);
  }

  .cross::after {
    transform: translate(-50%, -50%) rotate(-45deg);
  }
</style>
