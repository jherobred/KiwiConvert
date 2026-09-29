import { AnimatePresence, motion } from "motion/react";
import { FolderOpen, History, Trash2 } from "lucide-react";
import { useEffect, useRef, useState, type RefObject } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, on } from "../../lib/ipc";
import { baseName } from "../../lib/format";
import { spring } from "../../lib/motion";
import type { JobView } from "../../lib/types";
import { Button, IconButton, KiwiMark } from "../../components/ui";

function ago(ms: number): string {
  const s = Math.max(0, (Date.now() - ms) / 1000);
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
  return new Date(ms).toLocaleDateString();
}

export function Home({ dialogOpen }: { dialogOpen: RefObject<boolean> }) {
  const [dragging, setDragging] = useState(false);
  const [history, setHistory] = useState<JobView[]>([]);
  const zone = useRef<HTMLDivElement>(null);

  useEffect(() => {
    api.jobsHistory().then(setHistory);
    const subs = [on<boolean>("hub://drag", setDragging), on<void>("history://changed", () => api.jobsHistory().then(setHistory))];
    return () => subs.forEach((s) => s.then((f) => f()));
  }, []);

  const choose = async () => {
    dialogOpen.current = true;
    let picked: string[] | string | null = null;
    try {
      picked = await open({ multiple: true, title: "Choose files to convert" });
    } finally {
      dialogOpen.current = false;
    }
    const paths = Array.isArray(picked) ? picked : picked ? [picked] : [];
    if (!paths.length || !zone.current) return;
    // Let focus return from the dialog before the wheel takes it.
    await new Promise((r) => setTimeout(r, 120));
    const win = getCurrentWindow();
    const [pos, scale] = await Promise.all([win.outerPosition(), win.scaleFactor()]);
    const r = zone.current.getBoundingClientRect();
    await api.openWheel(paths, Math.round(pos.x + (r.left + r.width / 2) * scale), Math.round(pos.y + (r.top + r.height / 2) * scale));
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3 px-3 pb-3">
      <motion.div
        ref={zone}
        animate={{ scale: dragging ? 1.02 : 1 }}
        transition={spring.snap}
        className={`relative flex flex-col items-center gap-2 overflow-hidden rounded-2xl border-2 border-dashed px-5 py-6 text-center transition-colors ${
          dragging ? "border-accent bg-kiwi-100/60 dark:bg-kiwi-900/30" : "border-line bg-surface"
        }`}
      >
        <motion.div animate={{ scale: dragging ? 1.15 : 1, rotate: dragging ? 20 : 0 }} transition={spring.bloom}>
          <KiwiMark className="size-16 drop-shadow-md" spin />
        </motion.div>
        <div className="mt-1 font-display text-[17px] font-bold text-ink">{dragging ? "Let go to pick a format" : "Drop files here"}</div>
        <p className="max-w-64 text-[12.5px] leading-relaxed text-ink-3">
          Or hold <kbd className="rounded-md bg-sunken px-1.5 py-0.5 font-sans text-[11px] font-bold text-ink-2 ring-1 ring-line">Shift</kbd> while
          dragging a file in File Explorer. Add{" "}
          <kbd className="rounded-md bg-sunken px-1.5 py-0.5 font-sans text-[11px] font-bold text-ink-2 ring-1 ring-line">Ctrl</kbd> for tools.
        </p>
        <Button variant="primary" className="mt-1.5" onClick={choose}>
          <FolderOpen className="size-4" /> Choose files
        </Button>
      </motion.div>

      <div className="flex min-h-0 flex-1 flex-col">
        <div className="flex items-center justify-between px-1 pb-1.5">
          <h3 className="flex items-center gap-1.5 text-[11px] font-bold uppercase tracking-[0.08em] text-ink-3">
            <History className="size-3.5" /> Recent
          </h3>
          {history.length > 0 && (
            <IconButton label="Clear history" className="size-7" onClick={() => api.historyClear()}>
              <Trash2 className="size-3.5" />
            </IconButton>
          )}
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto pr-0.5">
          {history.length === 0 ? (
            <p className="px-1 pt-6 text-center text-[12.5px] text-ink-3">Converted files will show up here.</p>
          ) : (
            <ul className="flex flex-col gap-1">
              <AnimatePresence initial={false}>
                {history.slice(0, 30).map((job) => (
                  <RecentRow key={job.id} job={job} />
                ))}
              </AnimatePresence>
            </ul>
          )}
        </div>
      </div>
    </div>
  );
}

function RecentRow({ job }: { job: JobView }) {
  const [thumb, setThumb] = useState<string | null>(null);
  const target = job.outputs[0] ?? job.input;
  useEffect(() => {
    if (!target) return;
    let alive = true;
    api.thumbnail(target, 64).then((t) => alive && setThumb(t)).catch(() => {});
    return () => {
      alive = false;
    };
  }, [target]);
  const name = job.outputs.length > 1 ? `${job.outputs.length} files` : job.outputs[0] ? baseName(job.outputs[0]) : job.title;
  return (
    <motion.li layout initial={{ opacity: 0, y: -6 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0 }} transition={spring.soft}>
      <button
        onClick={() => job.outputs.length && api.reveal(job.outputs)}
        title={job.outputs.join("\n")}
        className="flex w-full items-center gap-2.5 rounded-xl px-2 py-1.5 text-left transition-colors hover:bg-surface"
      >
        <div className="size-9 shrink-0 overflow-hidden rounded-lg bg-sunken ring-1 ring-line">
          {thumb && <img src={thumb} alt="" className="size-full object-cover" draggable={false} />}
        </div>
        <div className="min-w-0 flex-1">
          <div className="truncate text-[13px] font-medium text-ink">{name}</div>
          <div className="truncate text-[11.5px] text-ink-3">
            {job.detail} · {ago(job.finishedMs ?? job.startedMs)}
          </div>
        </div>
      </button>
    </motion.li>
  );
}
