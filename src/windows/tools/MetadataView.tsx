import { AnimatePresence, motion } from "motion/react";
import { ChevronDown, Eraser, MapPinOff, Save } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { api } from "../../lib/ipc";
import { spring } from "../../lib/motion";
import type { MetaReport } from "../../lib/types";
import { Button, Section } from "../../components/ui";
import type { ToolProps } from "./ToolApp";
import { FileHeader, Sheet } from "./Sheet";

export default function MetadataView({ session, run }: ToolProps) {
  const paths = session.paths;
  const [report, setReport] = useState<MetaReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [edits, setEdits] = useState<Record<string, string>>({});
  const [open, setOpen] = useState<Record<string, boolean>>({ File: true });
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api
      .metadataRead(paths[0])
      .then((r) => {
        setReport(r);
        setEdits(Object.fromEntries(r.fields.map((f) => [f.key, f.value])));
      })
      .catch((e) => setError(String(e)));
  }, [paths]);

  const changed = useMemo(() => {
    if (!report) return {};
    return Object.fromEntries(report.fields.filter((f) => (edits[f.key] ?? "") !== f.value).map((f) => [f.key, edits[f.key] ?? ""]));
  }, [edits, report]);
  const dirty = Object.keys(changed).length > 0;

  const go = async (mode: "edit" | "strip" | "location") => {
    setBusy(true);
    await run({ mode, edits: mode === "edit" ? changed : {} });
  };

  return (
    <Sheet
      title="Metadata"
      subtitle={paths.length > 1 ? `Changes apply to all ${paths.length} files` : "Saved as a copy; the original stays as it is"}
      footer={
        <>
          {report?.hasLocation && (
            <Button disabled={busy} onClick={() => go("location")}>
              <MapPinOff className="size-4" /> Remove location
            </Button>
          )}
          <Button disabled={busy || !report} onClick={() => go("strip")}>
            <Eraser className="size-4" /> Remove all
          </Button>
          {report?.editable && (
            <Button variant="primary" disabled={busy || !dirty} onClick={() => go("edit")}>
              <Save className="size-4" /> Save changes
            </Button>
          )}
        </>
      }
    >
      <FileHeader paths={paths} />
      {error && <p className="rounded-xl bg-danger/10 px-3 py-2 text-sm text-danger">{error}</p>}
      {!report && !error && <p className="px-1 text-sm text-ink-3">Reading metadata…</p>}

      {report && report.fields.length > 0 && (
        <Section title="Edit">
          <div className="grid grid-cols-1 gap-2 pt-1 sm:grid-cols-2">
            {report.fields.map((f) => (
              <label key={f.key} className="flex flex-col gap-1">
                <span className="text-xs font-medium text-ink-3">{f.label}</span>
                <input
                  value={edits[f.key] ?? ""}
                  onChange={(e) => setEdits({ ...edits, [f.key]: e.target.value })}
                  placeholder="Empty"
                  className="h-9 rounded-xl bg-sunken px-3 text-[13px] text-ink ring-1 ring-line outline-none placeholder:text-ink-3/60 focus:ring-2 focus:ring-accent"
                />
              </label>
            ))}
          </div>
        </Section>
      )}

      {report?.groups.map((g) => (
        <div key={g.name} className="overflow-hidden rounded-2xl bg-surface ring-1 ring-line">
          <button onClick={() => setOpen({ ...open, [g.name]: !open[g.name] })} className="flex w-full items-center justify-between px-3.5 py-2.5 text-left">
            <span className="text-[12.5px] font-bold text-ink">
              {g.name} <span className="font-medium text-ink-3">· {g.entries.length}</span>
            </span>
            <motion.span animate={{ rotate: open[g.name] ? 180 : 0 }} transition={spring.snap}>
              <ChevronDown className="size-4 text-ink-3" />
            </motion.span>
          </button>
          <AnimatePresence initial={false}>
            {open[g.name] && (
              <motion.div initial={{ height: 0 }} animate={{ height: "auto" }} exit={{ height: 0 }} transition={spring.soft} className="overflow-hidden">
                <dl className="grid grid-cols-[minmax(0,2fr)_minmax(0,3fr)] gap-x-3 gap-y-1 border-t border-line px-3.5 py-2.5 text-[12.5px]">
                  {g.entries.map((e, i) => (
                    <div key={i} className="contents">
                      <dt className="truncate text-ink-3" title={e.key}>
                        {e.key}
                      </dt>
                      <dd className="truncate text-ink select-text" title={e.value}>
                        {e.value}
                      </dd>
                    </div>
                  ))}
                </dl>
              </motion.div>
            )}
          </AnimatePresence>
        </div>
      ))}
    </Sheet>
  );
}
