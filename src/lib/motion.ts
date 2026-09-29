import type { Transition } from "motion/react";

/** Springs shared across the app so everything moves with the same character. */
export const spring = {
  /** Snappy, for hover and small UI. */
  snap: { type: "spring", stiffness: 520, damping: 32, mass: 0.7 } satisfies Transition,
  /** Soft, for panels and cards entering. */
  soft: { type: "spring", stiffness: 300, damping: 30, mass: 0.9 } satisfies Transition,
  /** Bouncy, for the wheel blooming open. */
  bloom: { type: "spring", stiffness: 380, damping: 22, mass: 0.8 } satisfies Transition,
  /** Progress bars: follow the value smoothly without overshoot. */
  progress: { type: "spring", stiffness: 120, damping: 24 } satisfies Transition,
};

export const fade = { duration: 0.18, ease: [0.22, 1, 0.36, 1] } satisfies Transition;
