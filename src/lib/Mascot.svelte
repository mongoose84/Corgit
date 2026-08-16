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

  interface Props {
    pose: Pose;
    /** Rendered height in px. Width follows the pose's own aspect ratio —
     *  they differ (the sleeping dog is wide, the sitting one tall), and
     *  matching heights is what makes them look like one set. */
    height: number;
  }

  let { pose, height }: Props = $props();
</script>

<img
  class="mascot"
  class:trotting={pose === 'working' || pose === 'mini-working'}
  src={SOURCES[pose]}
  alt=""
  aria-hidden="true"
  draggable="false"
  style="height: {height}px"
/>

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
     still renders; it just holds still. */
  @media (prefers-reduced-motion: reduce) {
    .trotting {
      animation: none;
    }
  }
</style>
