<script lang="ts">
  /*
   * The Corgit mascot — dead space and dead time only (SPEC.md §14.1). The
   * poses and where each one belongs are in docs/mascot.md §5; the artwork is
   * cut from the contact sheet by scripts/extract-mascot.py into ./mascot.
   *
   * Raster, so there are no named parts to animate: a pose is one image, and
   * the only motion available is a transform on the whole of it (§6). Working
   * is the sole pose that takes it, because it stands in for a wait of unknown
   * length; the rest are still.
   *
   * The one exception is `gaze` — see §6.1. It does not break the rule above,
   * it sidesteps it: the pupils are cut into their own images by
   * `extract-mascot.py --eyes`, so the part that moves is still a whole image
   * being transformed, just a much smaller one.
   *
   * Decorative in every placement — the surrounding copy carries the meaning,
   * so this is hidden from assistive tech.
   */
  import resting from './mascot/resting.png';
  import content from './mascot/content.png';
  import working from './mascot/working.png';
  import sorry from './mascot/sorry.png';
  import mark from './mascot/mark.png';
  import miniWorking from './mascot/mini-working.png';
  import miniSorry from './mascot/mini-sorry.png';
  import restingEyeless from './mascot/resting-eyeless.png';
  import restingPupilNear from './mascot/resting-pupil-near.png';
  import restingPupilFar from './mascot/resting-pupil-far.png';
  import { RESTING_EYES, type Eye } from './mascot/eyes';

  type Pose = 'resting' | 'content' | 'working' | 'sorry' | 'mark' | 'mini-working' | 'mini-sorry';

  const SOURCES: Record<Pose, string> = {
    resting,
    content,
    working,
    sorry,
    mark,
    'mini-working': miniWorking,
    'mini-sorry': miniSorry,
  };

  /** The poses that have been split for `gaze`. Resting is the only one so far:
   *  it is the pose with open eyes that gets the most room (150px on the
   *  welcome screen), and at these sizes an eye is under 10px, so anything
   *  smaller would not read. */
  const RIGS: Partial<Record<Pose, { base: string; pupils: string[]; eyes: Eye[] }>> = {
    resting: {
      base: restingEyeless,
      pupils: [restingPupilNear, restingPupilFar],
      eyes: RESTING_EYES,
    },
  };

  interface Props {
    pose: Pose;
    /** Rendered height in px. Width follows the pose's own aspect ratio —
     *  they differ (the sleeping dog is wide, the sitting one tall), and
     *  matching heights is what makes them look like one set. */
    height: number;
    /** Let the eyes wander. Only for the long-lived empty states, and only on
     *  a pose in RIGS — asking for it elsewhere is silently the still pose. */
    gaze?: boolean;
  }

  let { pose, height, gaze = false }: Props = $props();

  const rig = $derived(gaze ? RIGS[pose] : undefined);
</script>

{#if rig}
  <span class="alive" style="height: {height}px" aria-hidden="true">
    <img class="mascot" src={rig.base} alt="" draggable="false" />
    {#each rig.eyes as eye, i (i)}
      <!-- The opening the pupil is allowed to move inside. Same job as the
           clip path in a vector rig: without it the pupil slides out over the
           fur at the extremes instead of stopping at the lid. -->
      <span
        class="socket"
        style="clip-path: ellipse({eye.clipRx}% {eye.clipRy}% at {eye.clipCx}% {eye.clipCy}%)"
      >
        <img
          class="pupil"
          src={rig.pupils[i]}
          alt=""
          draggable="false"
          style="left: {eye.left}%; top: {eye.top}%; width: {eye.width}%;
                 --travel-x: {eye.travelX}%; --travel-y: {eye.travelY}%"
        />
      </span>
    {/each}
  </span>
{:else}
  <img
    class="mascot"
    class:trotting={pose === 'working' || pose === 'mini-working'}
    src={SOURCES[pose]}
    alt=""
    aria-hidden="true"
    draggable="false"
    style="height: {height}px"
  />
{/if}

<style>
  .mascot {
    display: block;
    width: auto;
    /* Never a drop target, never a text selection, never in the way of a
       click meant for what is underneath. */
    pointer-events: none;
    user-select: none;
    -webkit-user-drag: none;
  }

  /* inline-block, not block: every position and size below is a percentage of
     this box, so it has to shrink to the pose rather than fill its parent. */
  .alive {
    position: relative;
    display: inline-block;
    pointer-events: none;
    user-select: none;
  }

  .alive .mascot {
    height: 100%;
  }

  .socket {
    position: absolute;
    inset: 0;
  }

  .pupil {
    position: absolute;
    -webkit-user-drag: none;
    /* 37s against the blink's-worth of travel available (§6.1) is deliberately
       far slower than a real eye. A saccade would be the loudest thing in an
       empty pane; this is meant to be caught out of the corner of yours. */
    animation: gaze 37s ease-in-out infinite;
  }

  /* Percentages here are of the pupil sprite, and the sprite is a fixed
     fraction of the pose, so the whole rig scales with `height` on its own.
     The holds are long and unequal so the loop cannot be counted; every move
     is ~1.3s, which at this size is a drift rather than a flick. */
  @keyframes gaze {
    0%,
    20% {
      transform: translate(0, 0);
    }
    23.5%,
    38% {
      transform: translate(calc(var(--travel-x) * -1), var(--travel-y));
    }
    41.5%,
    60% {
      transform: translate(0, 0);
    }
    63.5%,
    80% {
      transform: translate(var(--travel-x), calc(var(--travel-y) * -1));
    }
    83.5%,
    100% {
      transform: translate(0, 0);
    }
  }

  /* The drawn motion lines do most of the work; this is the gait under them.
     Continuous rather than idle-infrequent because it reports a live
     operation — when it stops, the sweep is over. */
  .trotting {
    animation: trot 0.68s ease-in-out infinite;
  }

  @keyframes trot {
    0%,
    100% {
      transform: translate(0, 0);
    }
    50% {
      transform: translate(1.5%, -4%);
    }
  }

  /* Idle decoration is exactly what this setting exists to stop. The mascot
     still renders; it just holds still — and a pupil at rest is exactly where
     it was drawn, so the gaze rig falls back to the original artwork. */
  @media (prefers-reduced-motion: reduce) {
    .trotting,
    .pupil {
      animation: none;
    }
  }
</style>
