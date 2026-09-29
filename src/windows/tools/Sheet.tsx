import { motion } from "motion/react";
import { useEffect, useState, type ReactNode } from "react";
import { api } from "../../lib/ipc";
import { baseName, bytes } from "../../lib/format";
import { spring } from "../../lib/motion";
import { TitleBar } from "../../components/ui";

/** Layout for the small tool windows: title bar, scrolling body, action footer. */
export function Sheet({ title, subtitle, children, footer }: { title: ReactNode; subtitle?: ReactNode; children: ReactNode; footer: ReactNode }) {
  return (
    <>
      <TitleBar title={title} subtitle={subtitle} />
      <motion.div
        className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto p-4"
        initial={{ opacity: 0, y: 8 }}
        animate={{ opacity: 1, y: 0 }}
        transition={spring.soft}
      >
        {children}
      </motion.div>
      <footer className="flex shrink-0 items-center justify-end gap-2 border-t border-line bg-surface px-4 py-3">{footer}</footer>
    </>
  );
}

function Thumb({ path }: { path: string }) {
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    api.thumbnail(path, 96).then((t) => alive && setSrc(t)).catch(() => {});
    return () => {
      alive = false;
    };
  }, [path]);
  return (
    <div className="size-11 shrink-0 overflow-hidden rounded-xl bg-sunken ring-1 ring-line">
      {src && <img src={src} alt="" className="size-full object-cover" draggable={false} />}
    </div>
  );
}

/** The files a tool will work on, with their combined size. */
export function FileHeader({ paths }: { paths: string[] }) {
  const [total, setTotal] = useState<number | null>(null);
  useEffect(() => {
    Promise.all(paths.map((p) => api.fileSize(p))).then((s) => setTotal(s.reduce((a, b) => a + b, 0)));
  }, [paths]);
  return (
    <div className="flex items-center gap-3 rounded-2xl bg-surface p-3 ring-1 ring-line">
      <div className="flex -space-x-5">
        {paths.slice(0, 3).map((p, i) => (
          <div key={p} style={{ zIndex: 3 - i, transform: `rotate(${(i - 1) * 5}deg)` }}>
            <Thumb path={p} />
          </div>
        ))}
      </div>
      <div className="min-w-0 flex-1">
        <div className="truncate text-[13.5px] font-semibold text-ink">{paths.length === 1 ? baseName(paths[0]) : `${paths.length} files`}</div>
        <div className="text-xs text-ink-3">{total === null ? "…" : bytes(total)}</div>
      </div>
    </div>
  );
}
