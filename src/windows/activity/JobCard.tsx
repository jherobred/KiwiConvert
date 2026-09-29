import { AnimatePresence, motion } from "motion/react";
import { AlertTriangle, Copy, ExternalLink, FolderOpen, Play, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { api } from "../../lib/ipc";
import { baseName, duration } from "../../lib/format";
import { spring } from "../../lib/motion";
import type { JobView } from "../../lib/types";

const AUTO_DISMISS_MS = 6500;

function useThumb(path: string | null) {
  const [thumb, setThumb] = useState<string | null>(null);
  useEffect(() => {
    if (!path) return;
    let alive = true;
    api.thumbnail(path, 96).then((t) => alive && setThumb(t)).catch(() => {});
    return () => {
      alive = false;
    };
  }, [path]);
  return thumb;
}

/** Seconds left, from how fast progress has moved so far. */
function eta(job: JobView, now: number): string | null {
  if (job.status !== "running" || job.progress < 0.04 || job.progress >= 1) return null;
  const elapsed = (now - job.startedMs) / 1000;
  if (elapsed < 2) return null;
  const left = (elapsed * (1 - job.progress)) / job.progress;
  if (left < 1) return "almost done";
  return `${duration(left)} left`;
}

function isLink(text: string) {
  return /^https?:\/\/\S+$/i.test(text.trim());
}

export function JobCard({ job, onDismiss }: { job: JobView; onDismiss: () => void }) {
  const thumb = useThumb(job.outputs.length === 1 ? job.outputs[0] : job.input);
  const [hovered, setHovered] = useState(false);
  const [copied, setCopied] = useState(false);
  const [now, setNow] = useState(Date.now());
  const timer = useRef<number | undefined>(undefined);
  const running = job.status === "running" || job.status === "queued";
  const done = job.status === "done";
  const failed = job.status === "failed";

  useEffect(() => {
    if (!running) return;
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [running]);

  // Finished cards leave on their own unless the pointer is on them or there is text to read.
  useEffect(() => {
    window.clearTimeout(timer.current);
    if (hovered || running || failed || job.text) return;
    timer.current = window.setTimeout(onDismiss, job.status === "cancelled" ? 1800 : AUTO_DISMISS_MS);
    return () => window.clearTimeout(timer.current);
  }, [hovered, running, failed, job.status, job.text, onDismiss]);

  const progress = Math.max(0, Math.min(1, job.progress));
  const indeterminate = running && job.progress < 0;
  const subtitle = failed
    ? "Couldn't finish"
    : job.status === "cancelled"
      ? "Cancelled"
      : done
        ? job.outputs.length > 1
          ? `Saved ${job.outputs.length} files`
          : job.outputs.length === 1
            ? `Saved ${baseName(job.outputs[0])}`
            : "Done"
        : [job.detail, job.stage, eta(job, now)].filter(Boolean).join(" · ");

  const copy = (text: string) => {
    navigator.clipboard.writeText(text).then(() => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1400);
    });
  };

  return (
    <motion.div
      data-card
      layout
      initial={{ opacity: 0, x: 60, scale: 0.96 }}
      animate={{ opacity: 1, x: 0, scale: 1 }}
      exit={{ opacity: 0, x: 80, transition: { duration: 0.22, ease: [0.4, 0, 1, 1] } }}
      transition={spring.soft}
      onHoverStart={() => setHovered(true)}
      onHoverEnd={() => setHovered(false)}
      className="panel relative w-full overflow-hidden rounded-2xl"
    >
      <div className="flex items-center gap-3 p-3 pr-2.5">
        <div className="relative size-12 shrink-0 overflow-hidden rounded-xl bg-sunken ring-1 ring-line">
          {thumb && <img src={thumb} alt="" className="size-full object-cover" draggable={false} />}
          <AnimatePresence>
            {done && (
              <motion.div
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                className="absolute inset-0 grid place-items-center bg-kiwi-600/70"
              >
                <svg viewBox="0 0 24 24" className="size-7 text-white" fill="none" stroke="currentColor" strokeWidth={3} strokeLinecap="round" strokeLinejoin="round">
                  <motion.path d="M5 12.5l4.5 4.5L19 7.5" initial={{ pathLength: 0 }} animate={{ pathLength: 1 }} transition={{ duration: 0.35, delay: 0.05 }} />
                </svg>
              </motion.div>
            )}
            {failed && (
              <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} className="absolute inset-0 grid place-items-center bg-danger/80">
                <AlertTriangle className="size-6 text-white" />
              </motion.div>
            )}
          </AnimatePresence>
        </div>

        <div className="min-w-0 flex-1">
          <div className="truncate text-[13.5px] font-semibold text-ink">{job.title}</div>
          <div className={`truncate text-xs ${failed ? "text-danger" : "text-ink-3"}`}>{subtitle}</div>
          {running && (
            <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-sunken">
              {indeterminate ? (
                <motion.div
                  className="h-full w-1/3 rounded-full bg-accent"
                  animate={{ x: ["-110%", "320%"] }}
                  transition={{ duration: 1.1, repeat: Infinity, ease: "easeInOut" }}
                />
              ) : (
                <motion.div
                  className="relative h-full rounded-full bg-accent"
                  initial={false}
                  animate={{ width: `${Math.max(3, progress * 100)}%` }}
                  transition={spring.progress}
                >
                  <div className="absolute inset-0 animate-pulse bg-white/20" />
                </motion.div>
              )}
            </div>
          )}
        </div>

        <div className="flex shrink-0 items-center gap-0.5">
          {running && (
            <CardButton label="Cancel" onClick={() => api.jobCancel(job.id)}>
              <X className="size-4" />
            </CardButton>
          )}
          {done && job.outputs.length === 1 && (
            <CardButton label="Open" onClick={() => api.openFile(job.outputs[0])}>
              <Play className="size-4" />
            </CardButton>
          )}
          {done && job.outputs.length > 0 && (
            <CardButton label="Show in folder" onClick={() => api.reveal(job.outputs)}>
              <FolderOpen className="size-4" />
            </CardButton>
          )}
          {!running && (
            <CardButton label="Dismiss" onClick={onDismiss}>
              <X className="size-4" />
            </CardButton>
          )}
        </div>
      </div>

      {(failed || job.text) && (
        <div className="border-t border-line bg-sunken/60 px-3 py-2">
          <p className="max-h-24 overflow-auto whitespace-pre-wrap break-words text-xs text-ink-2 select-text">{failed ? job.error : job.text}</p>
          <div className="mt-1.5 flex gap-1.5">
            <button
              className="flex items-center gap-1 rounded-lg bg-surface px-2 py-1 text-xs font-medium text-ink-2 ring-1 ring-line hover:text-ink"
              onClick={() => copy((failed ? job.error : job.text) ?? "")}
            >
              <Copy className="size-3.5" /> {copied ? "Copied" : "Copy"}
            </button>
            {!failed && job.text && isLink(job.text) && (
              <button
                className="flex items-center gap-1 rounded-lg bg-accent px-2 py-1 text-xs font-semibold text-accent-ink"
                onClick={() => api.openLink(job.text!.trim())}
              >
                <ExternalLink className="size-3.5" /> Open link
              </button>
            )}
          </div>
        </div>
      )}

      {done && !job.text && (
        <motion.div
          className="absolute bottom-0 left-0 h-0.5 bg-accent/60"
          initial={{ width: "100%" }}
          animate={{ width: hovered ? "100%" : "0%" }}
          transition={{ duration: hovered ? 0.2 : AUTO_DISMISS_MS / 1000, ease: "linear" }}
        />
      )}
    </motion.div>
  );
}

function CardButton({ label, onClick, children }: { label: string; onClick: () => void; children: React.ReactNode }) {
  return (
    <button
      title={label}
      aria-label={label}
      onClick={onClick}
      className="grid size-8 place-items-center rounded-lg text-ink-3 transition-colors hover:bg-sunken hover:text-ink active:scale-95"
    >
      {children}
    </button>
  );
}
