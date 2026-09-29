import { motion } from "motion/react";
import { LayoutGrid } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { DndContext, PointerSensor, closestCenter, useSensor, useSensors, type DragEndEvent } from "@dnd-kit/core";
import { SortableContext, arrayMove, horizontalListSortingStrategy, useSortable } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { api, fileUrl } from "../../../lib/ipc";
import { browserImage } from "../../../lib/kinds";
import { spring } from "../../../lib/motion";
import { Button, Section, Segmented, Slider, TitleBar } from "../../../components/ui";
import type { ToolProps } from "../ToolApp";

interface Item {
  path: string;
  bitmap: ImageBitmap;
}

type Layout = "grid" | "row" | "column";

const BACKGROUNDS = [
  { value: "#ffffff", label: "White" },
  { value: "#f4f1e3", label: "Cream" },
  { value: "#16270a", label: "Kiwi" },
  { value: "#000000", label: "Black" },
];

interface Cell {
  item: Item;
  x: number;
  y: number;
  w: number;
  h: number;
}

/** Positions every image for the chosen layout at output size. */
function arrange(items: Item[], layout: Layout, columns: number, width: number, gap: number, fit: "cover" | "contain"): { cells: Cell[]; width: number; height: number } {
  const n = items.length;
  if (!n) return { cells: [], width, height: 1 };
  const ratio = (i: Item) => i.bitmap.width / i.bitmap.height;
  if (layout === "row") {
    // One row of equal height; the width decides that height.
    const sum = items.reduce((s, i) => s + ratio(i), 0);
    const h = (width - gap * (n + 1)) / sum;
    let x = gap;
    const cells = items.map((item) => {
      const w = h * ratio(item);
      const c = { item, x, y: gap, w, h };
      x += w + gap;
      return c;
    });
    return { cells, width, height: Math.round(h + gap * 2) };
  }
  if (layout === "column") {
    const w = width - gap * 2;
    let y = gap;
    const cells = items.map((item) => {
      const h = w / ratio(item);
      const c = { item, x: gap, y, w, h };
      y += h + gap;
      return c;
    });
    return { cells, width, height: Math.round(y) };
  }
  // Grid: cells share the median aspect ratio so the collage stays tidy.
  const cols = Math.max(1, Math.min(columns, n));
  const rows = Math.ceil(n / cols);
  const ratios = items.map(ratio).sort((a, b) => a - b);
  const cellRatio = fit === "cover" ? Math.min(2, Math.max(0.5, ratios[Math.floor(ratios.length / 2)])) : 1;
  const cw = (width - gap * (cols + 1)) / cols;
  const ch = cw / cellRatio;
  const cells = items.map((item, i) => ({ item, x: gap + (i % cols) * (cw + gap), y: gap + Math.floor(i / cols) * (ch + gap), w: cw, h: ch }));
  return { cells, width, height: Math.round(gap + rows * (ch + gap)) };
}

function draw(ctx: CanvasRenderingContext2D, cells: Cell[], k: number, radius: number, fit: "cover" | "contain", background: string, w: number, h: number) {
  ctx.fillStyle = background;
  ctx.fillRect(0, 0, w * k, h * k);
  for (const c of cells) {
    const { bitmap } = c.item;
    ctx.save();
    ctx.beginPath();
    ctx.roundRect(c.x * k, c.y * k, c.w * k, c.h * k, radius * k);
    ctx.clip();
    const scale = fit === "cover" ? Math.max(c.w / bitmap.width, c.h / bitmap.height) : Math.min(c.w / bitmap.width, c.h / bitmap.height);
    const dw = bitmap.width * scale;
    const dh = bitmap.height * scale;
    ctx.drawImage(bitmap, (c.x + (c.w - dw) / 2) * k, (c.y + (c.h - dh) / 2) * k, dw * k, dh * k);
    ctx.restore();
  }
}

function StripItem({ item, index }: { item: Item; index: number }) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({ id: item.path });
  const src = useMemo(() => {
    const c = document.createElement("canvas");
    const s = 96 / Math.max(item.bitmap.width, item.bitmap.height);
    c.width = Math.round(item.bitmap.width * s);
    c.height = Math.round(item.bitmap.height * s);
    c.getContext("2d")!.drawImage(item.bitmap, 0, 0, c.width, c.height);
    return c.toDataURL();
  }, [item]);
  return (
    <div
      ref={setNodeRef}
      {...attributes}
      {...listeners}
      style={{ transform: CSS.Transform.toString(transform), transition, zIndex: isDragging ? 10 : undefined }}
      className={`relative size-16 shrink-0 cursor-grab overflow-hidden rounded-xl ring-1 ring-line ${isDragging ? "shadow-xl ring-2 ring-accent" : ""}`}
    >
      <img src={src} alt="" draggable={false} className="size-full object-cover" />
      <span className="absolute bottom-0.5 right-1 rounded bg-black/60 px-1 text-[10px] font-bold text-white">{index + 1}</span>
    </div>
  );
}

export default function CollageBuilder({ session, close }: ToolProps) {
  const [items, setItems] = useState<Item[]>([]);
  const [layout, setLayout] = useState<Layout>("grid");
  const [columns, setColumns] = useState(2);
  const [gap, setGap] = useState(24);
  const [radius, setRadius] = useState(18);
  const [background, setBackground] = useState("#ffffff");
  const [fit, setFit] = useState<"cover" | "contain">("cover");
  const [outWidth, setOutWidth] = useState("2400");
  const [busy, setBusy] = useState(false);
  const [box, setBox] = useState({ w: 700, h: 500 });
  const stage = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 5 } }));

  useEffect(() => {
    let alive = true;
    Promise.all(
      session.paths.map(async (path) => {
        const url = browserImage(path) ? fileUrl(path) : fileUrl((await api.imagePreview(path)).path);
        const blob = await (await fetch(url)).blob();
        return { path, bitmap: await createImageBitmap(blob, { imageOrientation: "from-image" }) };
      }),
    ).then((list) => {
      if (!alive) return;
      setItems(list);
      setColumns(Math.ceil(Math.sqrt(list.length)));
    });
    return () => {
      alive = false;
    };
  }, [session.paths]);

  useEffect(() => {
    const el = stage.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setBox({ w: el.clientWidth, h: el.clientHeight }));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const width = Number(outWidth);
  const plan = useMemo(() => arrange(items, layout, columns, width, gap * (width / 1600), fit), [items, layout, columns, width, gap, fit]);
  const k = Math.min((box.w - 48) / plan.width, (box.h - 48) / plan.height);

  useEffect(() => {
    const c = canvas.current;
    if (!c || !items.length) return;
    const dpr = window.devicePixelRatio || 1;
    c.width = Math.round(plan.width * k * dpr);
    c.height = Math.round(plan.height * k * dpr);
    c.style.width = `${plan.width * k}px`;
    c.style.height = `${plan.height * k}px`;
    draw(c.getContext("2d")!, plan.cells, k * dpr, radius * (width / 1600), fit, background, plan.width, plan.height);
  }, [plan, k, radius, fit, background, items.length, width]);

  const onDragEnd = (e: DragEndEvent) => {
    if (!e.over || e.active.id === e.over.id) return;
    const from = items.findIndex((i) => i.path === e.active.id);
    const to = items.findIndex((i) => i.path === e.over!.id);
    setItems(arrayMove(items, from, to));
  };

  const save = async () => {
    setBusy(true);
    await new Promise((r) => requestAnimationFrame(r));
    const out = document.createElement("canvas");
    out.width = plan.width;
    out.height = plan.height;
    const ctx = out.getContext("2d", { willReadFrequently: true })!;
    draw(ctx, plan.cells, 1, radius * (width / 1600), fit, background, plan.width, plan.height);
    const data = ctx.getImageData(0, 0, out.width, out.height).data;
    await api.saveRenderedImage(new Uint8Array(data.buffer), {
      source: session.paths[0],
      name: "Collage",
      width: out.width,
      height: out.height,
      format: "jpg",
      title: `Collage of ${items.length} photos`,
    });
    close();
  };

  return (
    <>
      <TitleBar title="Collage" subtitle={`${session.paths.length} photos · ${plan.width} × ${plan.height}`} />
      <div className="flex min-h-0 flex-1">
        <div className="flex min-w-0 flex-1 flex-col">
          <div ref={stage} className="relative grid min-h-0 flex-1 place-items-center bg-sunken">
            {items.length ? (
              <motion.canvas ref={canvas} layout transition={spring.soft} className="rounded-sm shadow-2xl" />
            ) : (
              <p className="text-sm text-ink-3">Loading photos…</p>
            )}
          </div>
          <div className="border-t border-line bg-canvas p-3">
            <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={onDragEnd}>
              <SortableContext items={items.map((i) => i.path)} strategy={horizontalListSortingStrategy}>
                <div className="flex gap-2 overflow-x-auto pb-1">
                  {items.map((item, i) => (
                    <StripItem key={item.path} item={item} index={i} />
                  ))}
                </div>
              </SortableContext>
            </DndContext>
          </div>
        </div>
        <aside className="flex w-[290px] shrink-0 flex-col border-l border-line bg-canvas">
          <div className="flex min-h-0 flex-1 flex-col gap-2.5 overflow-y-auto p-3">
            <Section title="Layout">
              <Segmented
                className="mt-1"
                value={layout}
                onChange={setLayout}
                options={[
                  { value: "grid", label: "Grid" },
                  { value: "row", label: "Row" },
                  { value: "column", label: "Column" },
                ]}
              />
              {layout === "grid" && <Slider label="Columns" min={1} max={Math.max(1, items.length)} value={columns} onChange={setColumns} />}
              <Segmented className="mt-2" value={fit} onChange={setFit} options={[{ value: "cover", label: "Fill cells" }, { value: "contain", label: "Show whole photo" }]} />
            </Section>
            <Section title="Style">
              <Slider label="Spacing" min={0} max={80} value={gap} onChange={setGap} format={(v) => `${v}px`} />
              <Slider label="Corner radius" min={0} max={80} value={radius} onChange={setRadius} format={(v) => `${v}px`} />
              <div className="flex gap-2 pt-1.5">
                {BACKGROUNDS.map((b) => (
                  <button
                    key={b.value}
                    title={b.label}
                    onClick={() => setBackground(b.value)}
                    className={`size-8 rounded-full ring-2 ring-offset-2 ring-offset-surface ${background === b.value ? "ring-accent" : "ring-transparent"}`}
                    style={{ background: b.value, boxShadow: "inset 0 0 0 1px rgb(0 0 0 / 0.15)" }}
                  />
                ))}
              </div>
            </Section>
            <Section title="Size">
              <Segmented
                className="mt-1"
                value={outWidth}
                onChange={setOutWidth}
                options={[
                  { value: "1600", label: "1600" },
                  { value: "2400", label: "2400" },
                  { value: "3600", label: "3600" },
                ]}
              />
              <p className="pt-1.5 text-xs text-ink-3">Width in pixels. Saved as a JPG beside the first photo.</p>
            </Section>
          </div>
          <div className="border-t border-line bg-surface p-3">
            <Button variant="primary" className="w-full" disabled={busy || items.length < 2} onClick={save}>
              <LayoutGrid className="size-4" /> Save collage
            </Button>
          </div>
        </aside>
      </div>
    </>
  );
}
