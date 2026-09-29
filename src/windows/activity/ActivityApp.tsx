import { AnimatePresence, motion } from "motion/react";
import { useEffect, useRef, useState } from "react";
import { api, on } from "../../lib/ipc";
import { useSettings } from "../../lib/theme";
import type { JobView } from "../../lib/types";
import { JobCard } from "./JobCard";

const MAX_VISIBLE = 4;
/** Room around the stack for card shadows. */
const PAD = 16;

export default function ActivityApp() {
  useSettings();
  const [jobs, setJobs] = useState<JobView[]>([]);
  const stack = useRef<HTMLDivElement>(null);

  useEffect(() => {
    api.jobsActive().then((list) => setJobs(list.filter((j) => j.status === "queued" || j.status === "running")));
    const un = on<JobView>("jobs://update", (job) => {
      setJobs((list) => {
        const i = list.findIndex((j) => j.id === job.id);
        if (i === -1) return [...list, job];
        const next = list.slice();
        next[i] = job;
        return next;
      });
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  const dismiss = (id: string) => {
    setJobs((list) => list.filter((j) => j.id !== id));
    api.jobDismiss(id);
  };

  // The window is sized to the cards so it never covers anything else. It grows at once
  // and shrinks after a short delay, so cards sliding down aren't clipped.
  useEffect(() => {
    const el = stack.current;
    if (!el) return;
    let shrink: number | undefined;
    let current = 0;
    const observer = new ResizeObserver(() => {
      const height = Math.ceil(el.getBoundingClientRect().height) + PAD * 2;
      window.clearTimeout(shrink);
      if (height >= current) {
        current = height;
        api.activityResize(height);
      } else {
        shrink = window.setTimeout(() => {
          current = height;
          api.activityResize(height);
        }, 380);
      }
    });
    observer.observe(el);
    return () => {
      observer.disconnect();
      window.clearTimeout(shrink);
    };
  }, []);

  const visible = jobs.slice(-MAX_VISIBLE);
  const hidden = jobs.length - visible.length;

  return (
    <div className="flex h-full w-full flex-col justify-end overflow-hidden" style={{ padding: PAD }}>
      <div ref={stack} className="flex flex-col gap-2.5">
        <AnimatePresence onExitComplete={() => !jobs.length && window.setTimeout(() => api.activityHide(), 400)}>
          {hidden > 0 && (
            <motion.div
              key="more"
              layout
              initial={{ opacity: 0, y: 10 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0 }}
              className="self-end rounded-full bg-surface/95 px-3 py-1 text-xs font-semibold text-ink-2 shadow ring-1 ring-line"
            >
              +{hidden} more
            </motion.div>
          )}
          {visible.map((job) => (
            <JobCard key={job.id} job={job} onDismiss={() => dismiss(job.id)} />
          ))}
        </AnimatePresence>
      </div>
    </div>
  );
}
