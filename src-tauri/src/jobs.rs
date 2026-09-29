//! Background jobs: every conversion or tool run is a job with live progress, cancellation,
//! and a card in the activity window.

use parking_lot::{Condvar, Mutex};
use serde::{Deserialize, Serialize};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobView {
    pub id: String,
    pub title: String,
    pub detail: String,
    pub status: JobStatus,
    /// 0..1, or negative while the length of the work is unknown.
    pub progress: f32,
    pub stage: Option<String>,
    pub outputs: Vec<String>,
    /// Text result, such as the contents of a QR code.
    pub text: Option<String>,
    pub error: Option<String>,
    pub input: Option<String>,
    pub started_ms: u64,
    pub finished_ms: Option<u64>,
}

#[derive(Default)]
pub struct JobOutcome {
    pub outputs: Vec<PathBuf>,
    pub text: Option<String>,
}

impl JobOutcome {
    pub fn files(outputs: Vec<PathBuf>) -> Self {
        Self { outputs, text: None }
    }
}

/// Returned by `Ctx::check` once the user cancels a job.
#[derive(Debug, thiserror::Error)]
#[error("Cancelled")]
pub struct Cancelled;

struct Job {
    view: Mutex<JobView>,
    cancel: AtomicBool,
}

/// Handed to job bodies for progress reporting and cancellation checks.
pub struct Ctx {
    job: Arc<Job>,
    app: AppHandle,
    last_emit: Mutex<Instant>,
}

impl Ctx {
    pub fn app(&self) -> &AppHandle {
        &self.app
    }

    pub fn cancelled(&self) -> bool {
        self.job.cancel.load(Ordering::Relaxed)
    }

    pub fn check(&self) -> anyhow::Result<()> {
        if self.cancelled() {
            Err(Cancelled.into())
        } else {
            Ok(())
        }
    }

    /// Reports progress in 0..1. Updates are throttled so the UI is not flooded.
    pub fn progress(&self, p: f32) {
        self.job.view.lock().progress = p.clamp(0.0, 1.0);
        self.emit_throttled();
    }

    pub fn stage(&self, stage: impl Into<String>) {
        self.job.view.lock().stage = Some(stage.into());
        self.emit_now();
    }

    fn emit_throttled(&self) {
        let mut last = self.last_emit.lock();
        if last.elapsed() >= Duration::from_millis(70) {
            *last = Instant::now();
            drop(last);
            self.emit_now();
        }
    }

    fn emit_now(&self) {
        let view = self.job.view.lock().clone();
        let _ = self.app.emit("jobs://update", view);
    }
}

impl Ctx {
    /// The whole progress bar, to be divided among steps.
    pub fn span(&self) -> Span<'_> {
        Span {
            ctx: self,
            base: 0.0,
            width: 1.0,
        }
    }
}

/// A slice of a job's progress bar. Steps report 0..1 within their own share.
#[derive(Clone, Copy)]
pub struct Span<'a> {
    ctx: &'a Ctx,
    base: f32,
    width: f32,
}

impl<'a> Span<'a> {
    pub fn progress(&self, p: f32) {
        self.ctx.progress(self.base + p.clamp(0.0, 1.0) * self.width);
    }

    /// Share `i` of `n` equal parts.
    pub fn part(&self, i: usize, n: usize) -> Span<'a> {
        let n = n.max(1) as f32;
        Span {
            ctx: self.ctx,
            base: self.base + self.width * i as f32 / n,
            width: self.width / n,
        }
    }

    /// The sub-range `from..to` (fractions of this span).
    pub fn range(&self, from: f32, to: f32) -> Span<'a> {
        Span {
            ctx: self.ctx,
            base: self.base + self.width * from,
            width: self.width * (to - from),
        }
    }

    pub fn check(&self) -> anyhow::Result<()> {
        self.ctx.check()
    }

    pub fn cancelled(&self) -> bool {
        self.ctx.cancelled()
    }

    pub fn ctx(&self) -> &'a Ctx {
        self.ctx
    }
}

/// A counting semaphore that caps how many jobs of one class run at once.
struct Gate {
    used: Mutex<usize>,
    cv: Condvar,
    max: usize,
}

impl Gate {
    fn new(max: usize) -> Self {
        Self {
            used: Mutex::new(0),
            cv: Condvar::new(),
            max,
        }
    }

    fn acquire(&self) {
        let mut used = self.used.lock();
        while *used >= self.max {
            self.cv.wait(&mut used);
        }
        *used += 1;
    }

    fn release(&self) {
        *self.used.lock() -= 1;
        self.cv.notify_one();
    }
}

pub struct JobManager {
    jobs: Mutex<Vec<Arc<Job>>>,
    heavy: Arc<Gate>,
    light: Arc<Gate>,
    history_path: Mutex<Option<PathBuf>>,
    history: Mutex<Vec<JobView>>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

const HISTORY_LIMIT: usize = 60;

impl JobManager {
    pub fn new() -> Self {
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        Self {
            jobs: Mutex::new(Vec::new()),
            // FFmpeg already uses every core, so two encodes at once is plenty.
            heavy: Arc::new(Gate::new(2)),
            light: Arc::new(Gate::new(cores.clamp(2, 6))),
            history_path: Mutex::new(None),
            history: Mutex::new(Vec::new()),
        }
    }

    pub fn load_history(&self, app: &AppHandle) {
        let Ok(dir) = app.path().app_data_dir() else { return };
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("history.json");
        if let Some(list) = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<Vec<JobView>>(&s).ok())
        {
            *self.history.lock() = list;
        }
        *self.history_path.lock() = Some(path);
    }

    pub fn history(&self) -> Vec<JobView> {
        self.history.lock().clone()
    }

    pub fn clear_history(&self) {
        self.history.lock().clear();
        self.save_history();
    }

    fn save_history(&self) {
        if let Some(path) = self.history_path.lock().clone() {
            let list = self.history.lock().clone();
            if let Ok(json) = serde_json::to_string(&list) {
                let _ = std::fs::write(path, json);
            }
        }
    }

    pub fn active(&self) -> Vec<JobView> {
        self.jobs.lock().iter().map(|j| j.view.lock().clone()).collect()
    }

    pub fn cancel(&self, id: &str) {
        if let Some(job) = self.jobs.lock().iter().find(|j| j.view.lock().id == id) {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }

    pub fn dismiss(&self, id: &str) {
        self.jobs.lock().retain(|j| {
            let v = j.view.lock();
            v.id != id || matches!(v.status, JobStatus::Queued | JobStatus::Running)
        });
    }

    /// Starts `body` on a background thread and returns the job id.
    pub fn spawn<F>(
        &self,
        app: &AppHandle,
        title: String,
        detail: String,
        input: Option<PathBuf>,
        heavy: bool,
        body: F,
    ) -> String
    where
        F: FnOnce(&Ctx) -> anyhow::Result<JobOutcome> + Send + 'static,
    {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let job = Arc::new(Job {
            view: Mutex::new(JobView {
                id: id.clone(),
                title,
                detail,
                status: JobStatus::Queued,
                progress: -1.0,
                stage: None,
                outputs: vec![],
                text: None,
                error: None,
                input: input.map(|p| p.to_string_lossy().into_owned()),
                started_ms: now_ms(),
                finished_ms: None,
            }),
            cancel: AtomicBool::new(false),
        });
        self.jobs.lock().push(job.clone());
        let _ = app.emit("jobs://update", job.view.lock().clone());
        crate::ui::show_activity(app);

        let gate = if heavy { self.heavy.clone() } else { self.light.clone() };
        let app = app.clone();
        std::thread::Builder::new()
            .name(format!("job-{}", &id[..8]))
            .spawn(move || {
                gate.acquire();
                let ctx = Ctx {
                    job: job.clone(),
                    app: app.clone(),
                    last_emit: Mutex::new(Instant::now() - Duration::from_secs(1)),
                };
                {
                    let mut v = job.view.lock();
                    v.status = JobStatus::Running;
                    v.started_ms = now_ms();
                }
                ctx.emit_now();

                let result = if ctx.cancelled() {
                    Err(Cancelled.into())
                } else {
                    catch_unwind(AssertUnwindSafe(|| body(&ctx))).unwrap_or_else(|panic| {
                        let msg = panic
                            .downcast_ref::<&str>()
                            .map(|s| s.to_string())
                            .or_else(|| panic.downcast_ref::<String>().cloned())
                            .unwrap_or_else(|| "unexpected internal error".into());
                        Err(anyhow::anyhow!("KiwiConvert hit an internal error: {msg}"))
                    })
                };
                gate.release();

                let view = {
                    let mut v = job.view.lock();
                    v.finished_ms = Some(now_ms());
                    match result {
                        Ok(outcome) => {
                            v.status = JobStatus::Done;
                            v.progress = 1.0;
                            v.outputs = outcome
                                .outputs
                                .iter()
                                .map(|p| p.to_string_lossy().into_owned())
                                .collect();
                            v.text = outcome.text;
                        }
                        Err(e) if e.is::<Cancelled>() || ctx.cancelled() => {
                            v.status = JobStatus::Cancelled;
                        }
                        Err(e) => {
                            v.status = JobStatus::Failed;
                            v.error = Some(format!("{e:#}"));
                            log::error!("job failed: {e:?}");
                        }
                    }
                    v.clone()
                };
                let _ = app.emit("jobs://update", view.clone());
                crate::jobs::on_finished(&app, &view);
            })
            .expect("failed to start a job thread");
        id
    }

    fn record(&self, view: &JobView) {
        let mut h = self.history.lock();
        h.retain(|v| v.id != view.id);
        h.insert(0, view.clone());
        h.truncate(HISTORY_LIMIT);
        drop(h);
        self.save_history();
    }
}

impl Default for JobManager {
    fn default() -> Self {
        Self::new()
    }
}

fn on_finished(app: &AppHandle, view: &JobView) {
    let manager = app.state::<JobManager>();
    if view.status == JobStatus::Done {
        manager.record(view);
        let _ = app.emit("history://changed", ());
        let settings = app.state::<crate::settings::SettingsStore>().get();
        if settings.reveal_outputs && !view.outputs.is_empty() {
            let paths: Vec<PathBuf> = view.outputs.iter().map(PathBuf::from).collect();
            crate::platform::reveal(&paths);
        }
    }
}
