import { AnimatePresence, motion } from "motion/react";
import { memo, useId } from "react";
import type { Mode, WheelOption } from "../../lib/types";
import { toolIcon } from "../../components/ToolIcon";
import { spring } from "../../lib/motion";
import { CX, CY, R_CORE, R_INNER, R_OUTER, R_SKIN, WIN_H, WIN_W, jitter, polar, sectorPath, segmentAngle } from "./geometry";

export interface WheelProps {
  options: WheelOption[];
  mode: Mode;
  hover: number;
  chosen: number | null;
  thumb: string | null;
  count: number;
  sizeLabel: string | null;
  interactive?: boolean;
  onHover?: (index: number) => void;
  onChoose?: (index: number) => void;
  reduceMotion?: boolean;
}

const LABEL_R = (R_INNER + R_OUTER) / 2 + 8;

/** The unit vector pointing out through the middle of segment `i`. */
function outward(i: number, n: number, distance: number): { x: number; y: number } {
  const a = (segmentAngle(i, n) * Math.PI) / 180;
  return { x: Math.sin(a) * distance, y: -Math.cos(a) * distance };
}

const Segment = memo(function Segment(props: {
  index: number;
  n: number;
  option: WheelOption;
  mode: Mode;
  hot: boolean;
  chosen: boolean;
  dimmed: boolean;
  ids: Record<string, string>;
  interactive: boolean;
  reduceMotion: boolean;
  onHover?: (i: number) => void;
  onChoose?: (i: number) => void;
}) {
  const { index: i, n, option, mode, hot, chosen, dimmed, ids } = props;
  const gold = mode === "tools";
  const push = chosen ? 14 : hot ? 8 : 0;
  const rest = outward(i, n, push);
  const start = outward(i, n, -46);
  const angle = segmentAngle(i, n);
  const Icon = toolIcon(option.icon);
  const [lx, ly] = polar(LABEL_R, angle);
  // Two seeds per segment, placed with a little deterministic scatter.
  const seeds = [-0.22, 0.2].map((f, k) => {
    const a = angle + f * (360 / n) + (jitter(i * 7 + k) - 0.5) * 4;
    const r = R_INNER + 16 + jitter(i * 13 + k) * 10;
    const [sx, sy] = polar(r, a);
    return { sx, sy, a };
  });
  const ink = gold ? "#4a3406" : "#1c3a0b";

  return (
    <motion.g
      initial={props.reduceMotion ? { opacity: 0 } : { opacity: 0, x: start.x, y: start.y }}
      animate={{ opacity: dimmed ? 0.35 : 1, x: rest.x, y: rest.y }}
      exit={props.reduceMotion ? { opacity: 0 } : { opacity: 0, x: start.x * 0.6, y: start.y * 0.6 }}
      transition={{ ...spring.bloom, delay: props.reduceMotion ? 0 : i * 0.018 }}
      style={{ cursor: props.interactive ? "pointer" : "default" }}
      onPointerEnter={() => props.onHover?.(i)}
      onPointerLeave={() => props.onHover?.(-1)}
      onClick={() => props.onChoose?.(i)}
    >
      <path d={sectorPath(i, n, R_INNER, R_OUTER, 3.2)} fill={`url(#${hot || chosen ? (gold ? ids.goldHot : ids.greenHot) : gold ? ids.gold : ids.green})`} />
      <motion.path
        d={sectorPath(i, n, R_INNER, R_OUTER, 3.2)}
        fill="white"
        initial={false}
        animate={{ opacity: hot || chosen ? 0.16 : 0 }}
        transition={{ duration: 0.15 }}
      />
      {seeds.map((s, k) => (
        <ellipse
          key={k}
          cx={s.sx}
          cy={s.sy}
          rx={2.6}
          ry={5.6}
          fill="var(--color-seed)"
          transform={`rotate(${s.a} ${s.sx} ${s.sy})`}
          opacity={0.9}
        />
      ))}
      {Icon ? (
        <g>
          <Icon x={lx - 11} y={ly - 22} width={22} height={22} color={ink} strokeWidth={2.1} />
          <text x={lx} y={ly + 14} textAnchor="middle" fontSize={n > 9 ? 9.5 : 10.5} fontWeight={700} letterSpacing="0.06em" fill={ink}>
            {option.label}
          </text>
        </g>
      ) : (
        <text x={lx} y={ly + 5} textAnchor="middle" fontSize={n > 10 ? 12.5 : 14} fontWeight={800} letterSpacing="0.04em" fill={ink}>
          {option.label}
        </text>
      )}
    </motion.g>
  );
});

export function Wheel(props: WheelProps) {
  const uid = useId().replace(/:/g, "");
  const ids = {
    green: `g${uid}`,
    greenHot: `gh${uid}`,
    gold: `o${uid}`,
    goldHot: `oh${uid}`,
    skin: `s${uid}`,
    core: `c${uid}`,
    clip: `k${uid}`,
    shadow: `f${uid}`,
  };
  const n = props.options.length;
  const chosen = props.chosen;
  const leaving = chosen !== null;

  return (
    <svg width={WIN_W} height={WIN_H} viewBox={`0 0 ${WIN_W} ${WIN_H}`} style={{ overflow: "visible" }}>
      <defs>
        <radialGradient id={ids.green} gradientUnits="userSpaceOnUse" cx={CX} cy={CY} r={R_OUTER}>
          <stop offset="0.35" stopColor="#dff0b4" />
          <stop offset="0.6" stopColor="#a9d860" />
          <stop offset="0.85" stopColor="#7cc23a" />
          <stop offset="1" stopColor="#5e9f26" />
        </radialGradient>
        <radialGradient id={ids.greenHot} gradientUnits="userSpaceOnUse" cx={CX} cy={CY} r={R_OUTER}>
          <stop offset="0.35" stopColor="#f0fad6" />
          <stop offset="0.6" stopColor="#c3ea7c" />
          <stop offset="0.85" stopColor="#97d94d" />
          <stop offset="1" stopColor="#74bb2f" />
        </radialGradient>
        <radialGradient id={ids.gold} gradientUnits="userSpaceOnUse" cx={CX} cy={CY} r={R_OUTER}>
          <stop offset="0.35" stopColor="#fff5c8" />
          <stop offset="0.6" stopColor="#f9e17a" />
          <stop offset="0.85" stopColor="#f0c53e" />
          <stop offset="1" stopColor="#d9a41d" />
        </radialGradient>
        <radialGradient id={ids.goldHot} gradientUnits="userSpaceOnUse" cx={CX} cy={CY} r={R_OUTER}>
          <stop offset="0.35" stopColor="#fffbe3" />
          <stop offset="0.6" stopColor="#fdeb9c" />
          <stop offset="0.85" stopColor="#f7d45a" />
          <stop offset="1" stopColor="#e8b62c" />
        </radialGradient>
        <radialGradient id={ids.skin} gradientUnits="userSpaceOnUse" cx={CX} cy={CY} r={R_SKIN}>
          <stop offset="0.88" stopColor="#9a7a4c" />
          <stop offset="1" stopColor="#5b422a" />
        </radialGradient>
        <radialGradient id={ids.core} gradientUnits="userSpaceOnUse" cx={CX} cy={CY - 12} r={R_CORE + 10}>
          <stop offset="0" stopColor="#fffef6" />
          <stop offset="1" stopColor="#ece8c6" />
        </radialGradient>
        <clipPath id={ids.clip}>
          <rect x={CX - 38} y={CY - 42} width={76} height={76} rx={14} />
        </clipPath>
        <filter id={ids.shadow} x="-30%" y="-30%" width="160%" height="160%">
          <feGaussianBlur stdDeviation="9" />
        </filter>
      </defs>

      <motion.g
        initial={props.reduceMotion ? { opacity: 0 } : { opacity: 0, scale: 0.55, rotate: -28 }}
        animate={leaving ? { opacity: 0, scale: 0.18, rotate: 0 } : { opacity: 1, scale: 1, rotate: 0 }}
        transition={leaving ? { duration: 0.32, delay: 0.24, ease: [0.5, 0, 0.75, 0] } : spring.bloom}
      >
        {/* Shadow and skin */}
        <circle cx={CX} cy={CY + 8} r={R_SKIN - 4} fill="rgb(0 0 0 / 0.42)" filter={`url(#${ids.shadow})`} />
        <circle cx={CX} cy={CY} r={R_SKIN} fill={`url(#${ids.skin})`} />
        <circle cx={CX} cy={CY} r={R_SKIN - 1.5} fill="none" stroke="#6d5234" strokeWidth={3} strokeDasharray="1.2 2.4" opacity={0.8} />
        <circle cx={CX} cy={CY} r={R_OUTER + 5} fill={props.mode === "tools" ? "#c79420" : "#4f8a1c"} />
        <circle cx={CX} cy={CY} r={R_OUTER + 2} fill="none" stroke={props.mode === "tools" ? "#ffe9a0" : "#d3ee9f"} strokeWidth={1.5} opacity={0.6} />

        {/* Petals */}
        <AnimatePresence mode="popLayout" initial={true}>
          <motion.g
            key={props.mode + n}
            initial={props.reduceMotion ? { opacity: 1 } : { rotate: -34 }}
            animate={{ rotate: 0 }}
            exit={props.reduceMotion ? { opacity: 0 } : { rotate: 30, opacity: 0 }}
            transition={spring.soft}
          >
            {props.options.map((option, i) => (
              <Segment
                key={option.label + i}
                index={i}
                n={n}
                option={option}
                mode={props.mode}
                hot={props.hover === i && !leaving}
                chosen={chosen === i}
                dimmed={leaving && chosen !== i}
                ids={ids}
                interactive={!!props.interactive}
                reduceMotion={!!props.reduceMotion}
                onHover={props.interactive ? props.onHover : undefined}
                onChoose={props.interactive ? props.onChoose : undefined}
              />
            ))}
          </motion.g>
        </AnimatePresence>

        {/* Core */}
        <motion.g
          initial={props.reduceMotion ? false : { scale: 0.2 }}
          animate={{ scale: props.hover === -1 && !leaving ? 1 : 0.96 }}
          transition={spring.snap}
        >
          <circle cx={CX} cy={CY} r={R_CORE} fill={`url(#${ids.core})`} />
          <circle cx={CX} cy={CY} r={R_CORE} fill="none" stroke="rgb(90 70 20 / 0.18)" strokeWidth={1.5} />
          {props.count > 1 && (
            <>
              <rect x={CX - 34} y={CY - 40} width={72} height={72} rx={14} fill="#cfcaa6" transform={`rotate(9 ${CX} ${CY})`} />
              <rect x={CX - 36} y={CY - 41} width={72} height={72} rx={14} fill="#dedab8" transform={`rotate(-6 ${CX} ${CY})`} />
            </>
          )}
          {props.thumb ? (
            <image href={props.thumb} x={CX - 38} y={CY - 42} width={76} height={76} clipPath={`url(#${ids.clip})`} preserveAspectRatio="xMidYMid slice" />
          ) : (
            <rect x={CX - 38} y={CY - 42} width={76} height={76} rx={14} fill="#e4e0bf" />
          )}
          {props.sizeLabel && (
            <g>
              <rect x={CX - 30} y={CY + 22} width={60} height={20} rx={10} fill="rgb(20 24 16 / 0.78)" />
              <text x={CX} y={CY + 36} textAnchor="middle" fontSize={11} fontWeight={700} fill="#fff">
                {props.sizeLabel}
              </text>
            </g>
          )}
          {props.count > 1 && (
            <g>
              <circle cx={CX + 36} cy={CY - 40} r={12} fill="var(--color-kiwi-600)" stroke="#fff" strokeWidth={2} />
              <text x={CX + 36} y={CY - 36} textAnchor="middle" fontSize={11} fontWeight={800} fill="#fff">
                {props.count > 99 ? "99+" : props.count}
              </text>
            </g>
          )}
        </motion.g>
      </motion.g>
    </svg>
  );
}
