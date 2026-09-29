import { AnimatePresence, motion } from "motion/react";
import { CheckSquare, RotateCcw, RotateCw, Split, Trash2 } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { DndContext, DragOverlay, PointerSensor, closestCenter, useSensor, useSensors, type DragEndEvent } from "@dnd-kit/core";
import { SortableContext, arrayMove, rectSortingStrategy, useSortable } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { api } from "../../../lib/ipc";
import { baseName } from "../../../lib/format";
import { spring } from "../../../lib/motion";
import { Button, IconButton, Section, TitleBar } from "../../../components/ui";
import type { ToolProps } from "../ToolApp";

interface Page {
  key: string;
  doc: number;
  index: number;
  /** Clockwise quarter turns added by the user. */
  rotate: number;
  aspect: number;
}

const thumbCache = new Map<string, Promise<string>>();

function Thumb({ path, index, rotate, aspect }: { path: string; index: number; rotate: number; aspect: number }) {
  const [src, setSrc] = useState<string | null>(null);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    let alive = true;
    const io = new IntersectionObserver(([entry]) => {
      if (!entry.isIntersecting) return;
      io.disconnect();
      const key = `${path}#${index}`;
      if (!thumbCache.has(key)) thumbCache.set(key, api.pdfThumb(path, index, 260));
      thumbCache.get(key)!.then((s) => alive && setSrc(s)).catch(() => {});
    });
    io.observe(el);
    return () => {
      alive = false;
      io.disconnect();
    };
  }, [path, index]);
  const sideways = rotate % 2 !== 0;
  return (
    <div ref={ref} className="grid aspect-[3/4] w-full place-items-center">
      <motion.div
        animate={{ rotate: rotate * 90 }}
        transition={spring.soft}
        className="overflow-hidden rounded-md bg-white shadow-md ring-1 ring-black/10"
        style={sideways ? { height: "100%", aspectRatio: `${1 / aspect}` } : { width: aspect > 0.75 ? "100%" : undefined, height: aspect > 0.75 ? undefined : "100%", aspectRatio: `${aspect}` }}
      >
        {src ? <img src={src} alt="" draggable={false} className="size-full object-contain" /> : <div className="size-full animate-pulse bg-sunken" />}
      </motion.div>
    </div>
  );
}

function PageCard({ page, path, number, selected, onClick, sortable }: { page: Page; path: string; number: number; selected: boolean; onClick: (e: React.MouseEvent) => void; sortable: boolean }) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({ id: page.key, disabled: !sortable });
  return (
    <div
      ref={setNodeRef}
      style={{ transform: CSS.Transform.toString(transform), transition, opacity: isDragging ? 0.3 : 1 }}
      {...attributes}
      {...listeners}
      onClick={onClick}
      className={`relative cursor-pointer rounded-2xl p-2.5 transition-colors ${selected ? "bg-kiwi-100 ring-2 ring-accent dark:bg-kiwi-900/40" : "hover:bg-surface"}`}
    >
      <Thumb path={path} index={page.index} rotate={page.rotate} aspect={page.aspect} />
      <div className="mt-1.5 text-center text-xs font-semibold text-ink-3">{number}</div>
    </div>
  );
}

/** Parses "1-3, 5, 8-" into inclusive 1-based ranges. */
export function parseRanges(text: string, pages: number): [number, number][] {
  const out: [number, number][] = [];
  for (const part of text.split(/[,;\s]+/).filter(Boolean)) {
    const m = part.match(/^(\d*)\s*-\s*(\d*)$/);
    if (m) {
      const a = m[1] ? Number(m[1]) : 1;
      const b = m[2] ? Number(m[2]) : pages;
      if (a >= 1 && b >= a) out.push([a, Math.min(b, pages)]);
    } else if (/^\d+$/.test(part)) {
      const n = Number(part);
      if (n >= 1 && n <= pages) out.push([n, n]);
    }
  }
  return out;
}

export default function PdfOrganizer({ session, run }: ToolProps) {
  const paths = session.paths;
  const splitMode = session.tool === "split-pdf";
  const [pages, setPages] = useState<Page[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [anchor, setAnchor] = useState<number | null>(null);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [how, setHow] = useState<"each" | "every" | "ranges" | "selected">("each");
  const [every, setEvery] = useState(2);
  const [rangeText, setRangeText] = useState("1-3, 4-");
  const [busy, setBusy] = useState(false);
  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 6 } }));

  useEffect(() => {
    api
      .pdfPages(paths)
      .then((list) => setPages(list.map((p, i) => ({ key: `${p.doc}-${p.index}-${i}`, doc: p.doc, index: p.index, rotate: 0, aspect: p.width / Math.max(1, p.height) }))))
      .catch((e) => setError(String(e)));
  }, [paths]);

  const click = (i: number, e: React.MouseEvent) => {
    const key = pages[i].key;
    const next = new Set(e.ctrlKey || e.metaKey ? selected : []);
    if (e.shiftKey && anchor !== null) {
      const [a, b] = [Math.min(anchor, i), Math.max(anchor, i)];
      for (let j = a; j <= b; j++) next.add(pages[j].key);
    } else if (next.has(key) && (e.ctrlKey || e.metaKey)) next.delete(key);
    else next.add(key);
    setSelected(next);
    setAnchor(i);
  };

  const targets = (p: Page) => selected.size === 0 || selected.has(p.key);
  const rotate = (q: number) => setPages((list) => list.map((p) => (targets(p) ? { ...p, rotate: (p.rotate + q + 4) % 4 } : p)));
  const remove = () => {
    setPages((list) => list.filter((p) => !selected.has(p.key)));
    setSelected(new Set());
  };

  const onDragEnd = (e: DragEndEvent) => {
    setActiveId(null);
    if (!e.over || e.active.id === e.over.id) return;
    const from = pages.findIndex((p) => p.key === e.active.id);
    const to = pages.findIndex((p) => p.key === e.over!.id);
    setPages(arrayMove(pages, from, to));
  };

  const splitRanges = useMemo((): [number, number][] => {
    const n = pages.length;
    if (how === "each") return [];
    if (how === "every") {
      const out: [number, number][] = [];
      for (let a = 1; a <= n; a += Math.max(1, every)) out.push([a, Math.min(n, a + Math.max(1, every) - 1)]);
      return out;
    }
    if (how === "ranges") return parseRanges(rangeText, n);
    // Split after each selected page.
    const cuts = pages.map((p, i) => (selected.has(p.key) ? i + 1 : 0)).filter((i) => i > 0 && i < n);
    const out: [number, number][] = [];
    let start = 1;
    for (const c of cuts) {
      out.push([start, c]);
      start = c + 1;
    }
    out.push([start, n]);
    return out;
  }, [how, every, rangeText, pages, selected]);

  const save = async () => {
    setBusy(true);
    if (splitMode) await run({ ranges: splitRanges });
    else await run({ pages: pages.map((p) => ({ doc: p.doc, index: p.index, rotate: p.rotate })) });
  };

  const active = pages.find((p) => p.key === activeId);
  const title = paths.length > 1 ? `${paths.length} PDFs` : baseName(paths[0]);

  return (
    <>
      <TitleBar title={splitMode ? "Split PDF" : "Organize pages"} subtitle={`${title} · ${pages.length} page${pages.length === 1 ? "" : "s"}`}>
        {!splitMode && (
          <div className="mr-2 flex items-center gap-0.5">
            <IconButton label="Rotate left" onClick={() => rotate(-1)}>
              <RotateCcw className="size-4" />
            </IconButton>
            <IconButton label="Rotate right" onClick={() => rotate(1)}>
              <RotateCw className="size-4" />
            </IconButton>
            <IconButton label="Delete selected" onClick={remove} disabled={selected.size === 0}>
              <Trash2 className="size-4" />
            </IconButton>
            <IconButton label="Select all" onClick={() => setSelected(new Set(selected.size === pages.length ? [] : pages.map((p) => p.key)))}>
              <CheckSquare className="size-4" />
            </IconButton>
          </div>
        )}
      </TitleBar>
      <div className="flex min-h-0 flex-1">
        <div className="min-w-0 flex-1 overflow-y-auto p-4" onClick={(e) => e.target === e.currentTarget && setSelected(new Set())}>
          {error && <p className="text-sm text-danger">{error}</p>}
          <DndContext sensors={sensors} collisionDetection={closestCenter} onDragStart={(e) => setActiveId(String(e.active.id))} onDragEnd={onDragEnd} onDragCancel={() => setActiveId(null)}>
            <SortableContext items={pages.map((p) => p.key)} strategy={rectSortingStrategy}>
              <div className="grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-2">
                <AnimatePresence>
                  {pages.map((p, i) => (
                    <motion.div key={p.key} initial={{ opacity: 0, scale: 0.9 }} animate={{ opacity: 1, scale: 1 }} exit={{ opacity: 0, scale: 0.8 }} transition={spring.soft}>
                      <PageCard page={p} path={paths[p.doc]} number={i + 1} selected={selected.has(p.key)} onClick={(e) => click(i, e)} sortable={!splitMode} />
                    </motion.div>
                  ))}
                </AnimatePresence>
              </div>
            </SortableContext>
            <DragOverlay>
              {active && (
                <div className="rotate-3 rounded-2xl bg-surface p-2.5 shadow-2xl ring-2 ring-accent">
                  <Thumb path={paths[active.doc]} index={active.index} rotate={active.rotate} aspect={active.aspect} />
                </div>
              )}
            </DragOverlay>
          </DndContext>
        </div>

        <aside className="flex w-[280px] shrink-0 flex-col border-l border-line bg-canvas">
          <div className="min-h-0 flex-1 overflow-y-auto p-3">
            {splitMode ? (
              <Section title="Split into">
                <div className="flex flex-col gap-1 pt-1">
                  {(
                    [
                      ["each", "One file per page"],
                      ["every", "Groups of pages"],
                      ["ranges", "Custom ranges"],
                      ["selected", "After selected pages"],
                    ] as const
                  ).map(([value, label]) => (
                    <button key={value} onClick={() => setHow(value)} className="relative rounded-xl px-3 py-2 text-left text-[13px] font-semibold text-ink">
                      {how === value && <motion.span layoutId="split-how" className="absolute inset-0 rounded-xl bg-kiwi-100 ring-1 ring-accent/40 dark:bg-kiwi-900/40" transition={spring.snap} />}
                      <span className="relative">{label}</span>
                    </button>
                  ))}
                </div>
                {how === "every" && (
                  <label className="mt-2 flex items-center gap-2 text-sm text-ink-2">
                    Every
                    <input type="number" min={1} value={every} onChange={(e) => setEvery(Math.max(1, Number(e.target.value) || 1))} className="h-8 w-16 rounded-lg bg-sunken px-2 text-ink ring-1 ring-line" />
                    pages
                  </label>
                )}
                {how === "ranges" && (
                  <input
                    value={rangeText}
                    onChange={(e) => setRangeText(e.target.value)}
                    placeholder="1-3, 5, 8-"
                    className="mt-2 h-9 w-full rounded-xl bg-sunken px-3 text-sm text-ink ring-1 ring-line outline-none focus:ring-2 focus:ring-accent"
                  />
                )}
                {how === "selected" && <p className="mt-2 text-xs text-ink-3">Click pages where a new file should end. Hold Ctrl to pick several.</p>}
                <p className="mt-3 text-xs font-semibold text-ink-2">
                  {how === "each" ? `${pages.length} files` : `${splitRanges.length} file${splitRanges.length === 1 ? "" : "s"}`}
                </p>
              </Section>
            ) : (
              <Section title="Tips">
                <ul className="flex list-disc flex-col gap-1.5 pl-4 pt-1 text-xs leading-relaxed text-ink-3">
                  <li>Drag pages to reorder them.</li>
                  <li>Click to select; Ctrl or Shift to select more.</li>
                  <li>Rotate or delete from the title bar.</li>
                  {paths.length > 1 && <li>Pages from all {paths.length} PDFs are combined into one file.</li>}
                </ul>
              </Section>
            )}
          </div>
          <div className="border-t border-line bg-surface p-3">
            <Button variant="primary" className="w-full" disabled={busy || pages.length === 0 || (splitMode && how !== "each" && splitRanges.length === 0)} onClick={save}>
              {splitMode ? (
                <>
                  <Split className="size-4" /> Split PDF
                </>
              ) : (
                "Save as new PDF"
              )}
            </Button>
          </div>
        </aside>
      </div>
    </>
  );
}
