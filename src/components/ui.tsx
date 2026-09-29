import clsx from "clsx";
import { motion } from "motion/react";
import { Minus, Square, X } from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useId, type ButtonHTMLAttributes, type ReactNode } from "react";
import { spring } from "../lib/motion";

export function Button({
  variant = "plain",
  size = "md",
  className,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: "primary" | "plain" | "ghost" | "danger"; size?: "sm" | "md" }) {
  return (
    <button
      {...rest}
      className={clsx(
        "inline-flex items-center justify-center gap-1.5 rounded-xl font-semibold transition-[background,transform,opacity] active:scale-[0.97] disabled:pointer-events-none disabled:opacity-45",
        size === "sm" ? "h-8 px-3 text-[12.5px]" : "h-9.5 px-4 text-[13.5px]",
        variant === "primary" && "bg-accent text-accent-ink shadow-sm hover:brightness-105",
        variant === "plain" && "bg-surface text-ink ring-1 ring-line hover:bg-sunken",
        variant === "ghost" && "text-ink-2 hover:bg-sunken hover:text-ink",
        variant === "danger" && "bg-danger text-white hover:brightness-105",
        className,
      )}
    />
  );
}

export function IconButton({ label, className, ...rest }: ButtonHTMLAttributes<HTMLButtonElement> & { label: string }) {
  return (
    <button
      {...rest}
      title={label}
      aria-label={label}
      className={clsx("grid size-8 place-items-center rounded-lg text-ink-3 transition-colors hover:bg-sunken hover:text-ink active:scale-95", className)}
    />
  );
}

export function Toggle({ checked, onChange, label, hint }: { checked: boolean; onChange: (v: boolean) => void; label: ReactNode; hint?: ReactNode }) {
  const id = useId();
  return (
    <label htmlFor={id} className="flex cursor-pointer items-center justify-between gap-4 py-2">
      <span className="min-w-0">
        <span className="block text-[13.5px] font-medium text-ink">{label}</span>
        {hint && <span className="block text-xs text-ink-3">{hint}</span>}
      </span>
      <button
        id={id}
        role="switch"
        aria-checked={checked}
        onClick={() => onChange(!checked)}
        className={clsx("relative h-5.5 w-10 shrink-0 rounded-full transition-colors", checked ? "bg-accent" : "bg-ink-3/35")}
      >
        <motion.span
          className="absolute top-0.75 size-4 rounded-full bg-white shadow"
          animate={{ left: checked ? 21 : 3 }}
          transition={spring.snap}
        />
      </button>
    </label>
  );
}

export function Slider({
  value,
  min,
  max,
  step = 1,
  onChange,
  label,
  format = (v) => String(v),
  center,
}: {
  value: number;
  min: number;
  max: number;
  step?: number;
  onChange: (v: number) => void;
  label?: ReactNode;
  format?: (v: number) => string;
  /** Value where the fill starts, for bidirectional sliders (0 for adjustments). */
  center?: number;
}) {
  const pct = ((value - min) / (max - min)) * 100;
  const origin = center === undefined ? 0 : ((center - min) / (max - min)) * 100;
  const left = Math.min(pct, origin);
  const width = Math.abs(pct - origin);
  return (
    <div className="py-1.5">
      {label && (
        <div className="mb-1 flex items-baseline justify-between text-[12.5px]">
          <span className="font-medium text-ink-2">{label}</span>
          <span className="tabular-nums text-ink-3">{format(value)}</span>
        </div>
      )}
      <div className="relative h-5">
        <div className="absolute inset-x-0 top-1/2 h-1.5 -translate-y-1/2 rounded-full bg-sunken ring-1 ring-line" />
        <div className="absolute top-1/2 h-1.5 -translate-y-1/2 rounded-full bg-accent" style={{ left: `${left}%`, width: `${width}%` }} />
        <input
          type="range"
          min={min}
          max={max}
          step={step}
          value={value}
          onChange={(e) => onChange(Number(e.target.value))}
          onDoubleClick={() => center !== undefined && onChange(center)}
          className="kiwi-range absolute inset-0 w-full cursor-pointer appearance-none bg-transparent"
        />
      </div>
    </div>
  );
}

export function Segmented<T extends string>({
  value,
  options,
  onChange,
  className,
}: {
  value: T;
  options: { value: T; label: ReactNode }[];
  onChange: (v: T) => void;
  className?: string;
}) {
  const id = useId();
  return (
    <div className={clsx("flex rounded-xl bg-sunken p-0.5 ring-1 ring-line", className)}>
      {options.map((o) => (
        <button
          key={o.value}
          onClick={() => onChange(o.value)}
          className={clsx(
            "relative flex-1 rounded-[10px] px-2.5 py-1.5 text-[12.5px] font-semibold transition-colors",
            value === o.value ? "text-ink" : "text-ink-3 hover:text-ink-2",
          )}
        >
          {value === o.value && (
            <motion.span layoutId={`seg-${id}`} className="absolute inset-0 rounded-[10px] bg-surface shadow-sm ring-1 ring-line" transition={spring.snap} />
          )}
          <span className="relative inline-flex items-center gap-1.5">{o.label}</span>
        </button>
      ))}
    </div>
  );
}

export function Section({ title, children, className }: { title?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <section className={clsx("rounded-2xl bg-surface p-3.5 ring-1 ring-line", className)}>
      {title && <h3 className="mb-1 text-[11px] font-bold uppercase tracking-[0.08em] text-ink-3">{title}</h3>}
      {children}
    </section>
  );
}

/** Title bar for frameless tool windows. */
export function TitleBar({ title, subtitle, children }: { title: ReactNode; subtitle?: ReactNode; children?: ReactNode }) {
  const win = getCurrentWindow();
  return (
    <header data-tauri-drag-region className="flex h-12 shrink-0 items-center gap-3 border-b border-line bg-surface pl-4 pr-1.5">
      <KiwiMark className="pointer-events-none size-6" />
      <div data-tauri-drag-region className="min-w-0 flex-1">
        <div data-tauri-drag-region className="truncate text-[13.5px] font-semibold text-ink">
          {title}
        </div>
        {subtitle && (
          <div data-tauri-drag-region className="truncate text-[11.5px] text-ink-3">
            {subtitle}
          </div>
        )}
      </div>
      {children}
      <div className="flex items-center">
        <IconButton label="Minimize" onClick={() => win.minimize()}>
          <Minus className="size-4" />
        </IconButton>
        <IconButton label="Maximize" onClick={() => win.toggleMaximize()}>
          <Square className="size-3.5" />
        </IconButton>
        <IconButton label="Close" className="hover:bg-danger hover:text-white" onClick={() => win.close()}>
          <X className="size-4" />
        </IconButton>
      </div>
    </header>
  );
}

/** The kiwi slice mark used in headers. */
export function KiwiMark({ className, spin = false }: { className?: string; spin?: boolean }) {
  const seeds = Array.from({ length: 10 }, (_, i) => i * 36);
  return (
    <motion.svg
      viewBox="0 0 64 64"
      className={className}
      animate={spin ? { rotate: 360 } : undefined}
      transition={spin ? { duration: 18, repeat: Infinity, ease: "linear" } : undefined}
    >
      <circle cx="32" cy="32" r="31" fill="#6b4f31" />
      <circle cx="32" cy="32" r="27.5" fill="#7cc23a" />
      {seeds.map((a) => (
        <line key={`l${a}`} x1="32" y1="32" x2={32 + 27 * Math.sin((a * Math.PI) / 180)} y2={32 - 27 * Math.cos((a * Math.PI) / 180)} stroke="#b9e27a" strokeWidth="1.4" />
      ))}
      {seeds.map((a) => {
        const r = ((a + 18) * Math.PI) / 180;
        const x = 32 + 16 * Math.sin(r);
        const y = 32 - 16 * Math.cos(r);
        return <ellipse key={a} cx={x} cy={y} rx="1.6" ry="3.2" fill="#1d1812" transform={`rotate(${a + 18} ${x} ${y})`} />;
      })}
      <circle cx="32" cy="32" r="9.5" fill="#f4f1d6" />
    </motion.svg>
  );
}
