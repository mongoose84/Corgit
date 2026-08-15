<script lang="ts">
  /*
   * The twogit mascot — shown in the graph pane when nothing is checked out.
   *
   * Built from the app mark's own parts so it reads as branding rather than
   * clip-art: the two blue nodes of the mark become eyes, and the green merge
   * node keeps its relationship to them, branching off to the right as an
   * antenna. Colours come from the lane tokens, so it belongs to the graph it
   * is standing in for.
   *
   * Decorative only — the surrounding EmptyState copy carries the meaning, so
   * this is hidden from assistive tech.
   */

  interface Props {
    /** Rendered width in px. Height follows the square viewBox. */
    size?: number;
  }

  let { size = 104 }: Props = $props();
</script>

<svg
  class="mascot"
  width={size}
  height={size}
  viewBox="0 0 120 120"
  fill="none"
  aria-hidden="true"
>
  <!-- The merge node, branching off the head exactly as it branches off the
       lane in the app mark. -->
  <g class="antenna">
    <line x1="83" y1="40" x2="95" y2="24" stroke="var(--lane-2)" stroke-width="4" stroke-linecap="round" />
    <circle cx="97" cy="21" r="7" fill="var(--lane-2)" />
  </g>

  <!-- Head: the icon's squircle, softened. -->
  <rect
    x="24"
    y="36"
    width="72"
    height="56"
    rx="18"
    fill="var(--bg-raised)"
    stroke="var(--border-strong)"
    stroke-width="2"
  />

  <!-- Feet, planted just under the head. -->
  <g stroke="var(--border-strong)" stroke-width="4" stroke-linecap="round">
    <line x1="45" y1="92" x2="45" y2="101" />
    <line x1="75" y1="92" x2="75" y2="101" />
  </g>

  <!-- The two lane nodes, side by side. `.glance` translates, `.eye` blinks,
       kept on separate elements so the two transforms don't fight. -->
  <g class="glance">
    <circle class="eye" cx="47" cy="59" r="6" fill="var(--lane-1)" />
    <circle class="eye" cx="73" cy="59" r="6" fill="var(--lane-1)" />
  </g>

  <path
    d="M51 76 Q60 84 69 76"
    stroke="var(--text-muted)"
    stroke-width="2.5"
    stroke-linecap="round"
  />
</svg>

<style>
  .mascot {
    display: block;
    overflow: visible;
  }

  /* Blink. The long flat stretch is the point — a mascot that blinks often
     reads as broken rather than alive. */
  .eye {
    transform-box: fill-box;
    transform-origin: center;
    animation: blink 6.4s infinite;
  }

  /* An idle look toward the repo list, on a period that shares no common
     factor with the blink so the two never lock into a visible pattern. */
  .glance {
    transform-box: view-box;
    animation: glance 11s infinite;
  }

  .antenna {
    transform-box: view-box;
    transform-origin: 83px 40px;
    animation: bob 5.3s ease-in-out infinite;
  }

  @keyframes blink {
    0%,
    93%,
    100% {
      transform: scaleY(1);
    }
    95.5%,
    96.5% {
      transform: scaleY(0.1);
    }
  }

  @keyframes glance {
    0%,
    28%,
    52%,
    100% {
      transform: translateX(0);
    }
    36%,
    46% {
      transform: translateX(-2.5px);
    }
  }

  @keyframes bob {
    0%,
    100% {
      transform: rotate(0deg);
    }
    50% {
      transform: rotate(-7deg);
    }
  }

  /* Idle decoration is exactly the kind of motion this setting exists to
     stop — the mascot still renders, it just holds still. */
  @media (prefers-reduced-motion: reduce) {
    .eye,
    .glance,
    .antenna {
      animation: none;
    }
  }
</style>
