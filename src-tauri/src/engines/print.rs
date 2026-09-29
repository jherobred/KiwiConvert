//! HTML to PDF through WebView2's print engine, the same one Edge uses for "Save as PDF".
//! Used for DOCX, TXT and Markdown to PDF. It lays out any script and font Windows has.

use anyhow::{Context, Result, anyhow, bail};
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::time::Duration;
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, WebviewUrl, WebviewWindowBuilder};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_PRINT_ORIENTATION_LANDSCAPE, COREWEBVIEW2_PRINT_ORIENTATION_PORTRAIT, ICoreWebView2_2, ICoreWebView2_7,
    ICoreWebView2Environment6,
};
use webview2_com::PrintToPdfCompletedHandler;
use windows::core::{HSTRING, Interface};

static COUNTER: AtomicU32 = AtomicU32::new(0);

#[derive(Debug, Clone, Copy)]
pub struct Page {
    /// Inches.
    pub width: f64,
    pub height: f64,
    pub margin: f64,
    pub landscape: bool,
}

impl Page {
    pub fn from_setting(page_size: &str) -> Self {
        let (width, height) = if page_size == "letter" { (8.5, 11.0) } else { (8.27, 11.69) };
        Page { width, height, margin: 0.8, landscape: false }
    }
}

/// Wraps content in a document with a locked-down CSP: no scripts, no network.
pub fn document(title: &str, body: &str, extra_css: &str) -> String {
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src data:; style-src 'unsafe-inline'; font-src data:">
<title>{title}</title>
<style>
  html {{ -webkit-print-color-adjust: exact; print-color-adjust: exact; }}
  body {{ font-family: "Segoe UI", "Segoe UI Variable Text", Calibri, sans-serif; font-size: 11pt; line-height: 1.45; color: #111; margin: 0; }}
  p {{ margin: 0 0 8pt; white-space: pre-wrap; }}
  h1, h2, h3, h4, h5, h6 {{ margin: 14pt 0 6pt; line-height: 1.2; page-break-after: avoid; }}
  h1 {{ font-size: 20pt; }} h2 {{ font-size: 16pt; }} h3 {{ font-size: 13pt; }}
  table {{ border-collapse: collapse; margin: 6pt 0 10pt; width: 100%; }}
  td, th {{ border: 1px solid #999; padding: 4pt 6pt; vertical-align: top; }}
  td p, th p {{ margin: 0; }}
  img {{ max-width: 100%; height: auto; }}
  ul, ol {{ margin: 0 0 8pt; padding-left: 22pt; }}
  li {{ margin: 0 0 2pt; }}
  pre {{ font-family: Consolas, "Cascadia Mono", monospace; font-size: 9.5pt; white-space: pre-wrap; word-wrap: break-word; margin: 0; }}
  .page-break {{ break-after: page; }}
  a {{ color: #0b57d0; }}
  {extra_css}
</style></head><body>{body}</body></html>"#
    )
}

pub fn escape(text: &str) -> String {
    let mut s = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => s.push_str("&amp;"),
            '<' => s.push_str("&lt;"),
            '>' => s.push_str("&gt;"),
            '"' => s.push_str("&quot;"),
            _ => s.push(c),
        }
    }
    s
}

/// Renders `html` to a PDF file at `out`.
pub fn html_to_pdf(app: &AppHandle, html: &str, out: &Path, page: Page) -> Result<()> {
    let dir = std::env::temp_dir().join("KiwiConvert").join("print");
    std::fs::create_dir_all(&dir)?;
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let html_path = dir.join(format!("doc-{}-{n}.html", uuid::Uuid::new_v4().simple()));
    std::fs::write(&html_path, html)?;
    let url = tauri::Url::from_file_path(&html_path).map_err(|_| anyhow!("bad temporary path"))?;

    let (loaded_tx, loaded_rx) = mpsc::channel::<()>();
    let label = format!("print-{n}");
    let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(url))
        .visible(false)
        .skip_taskbar(true)
        .inner_size(900.0, 1200.0)
        .disable_drag_drop_handler()
        .on_page_load(move |_, payload| {
            if payload.event() == PageLoadEvent::Finished {
                let _ = loaded_tx.send(());
            }
        })
        .build()
        .context("could not start the PDF printer")?;

    let result = (|| -> Result<()> {
        loaded_rx
            .recv_timeout(Duration::from_secs(30))
            .map_err(|_| anyhow!("the document took too long to lay out"))?;
        // Give images a moment to decode before printing.
        std::thread::sleep(Duration::from_millis(150));

        let (done_tx, done_rx) = mpsc::channel::<Result<()>>();
        let target = HSTRING::from(out.as_os_str());
        window.with_webview(move |pw| {
            let started = (|| -> windows::core::Result<()> {
                unsafe {
                    let core = pw.controller().CoreWebView2()?;
                    let env = core.cast::<ICoreWebView2_2>()?.Environment()?.cast::<ICoreWebView2Environment6>()?;
                    let settings = env.CreatePrintSettings()?;
                    settings.SetShouldPrintBackgrounds(true.into())?;
                    settings.SetShouldPrintHeaderAndFooter(false.into())?;
                    settings.SetPageWidth(page.width)?;
                    settings.SetPageHeight(page.height)?;
                    settings.SetMarginTop(page.margin)?;
                    settings.SetMarginBottom(page.margin)?;
                    settings.SetMarginLeft(page.margin)?;
                    settings.SetMarginRight(page.margin)?;
                    settings.SetOrientation(if page.landscape {
                        COREWEBVIEW2_PRINT_ORIENTATION_LANDSCAPE
                    } else {
                        COREWEBVIEW2_PRINT_ORIENTATION_PORTRAIT
                    })?;
                    let tx = done_tx.clone();
                    let handler = PrintToPdfCompletedHandler::create(Box::new(move |hr, ok| {
                        let r = match (hr, ok) {
                            (Ok(()), true) => Ok(()),
                            (Err(e), _) => Err(anyhow!("printing failed: {e}")),
                            _ => Err(anyhow!("printing failed")),
                        };
                        let _ = tx.send(r);
                        Ok(())
                    }));
                    core.cast::<ICoreWebView2_7>()?.PrintToPdf(&target, &settings, &handler)?;
                }
                Ok(())
            })();
            if let Err(e) = started {
                let _ = done_tx.send(Err(anyhow!("printing is not available: {e}")));
            }
        })?;
        done_rx
            .recv_timeout(Duration::from_secs(120))
            .map_err(|_| anyhow!("printing took too long"))??;
        if std::fs::metadata(out).map(|m| m.len()).unwrap_or(0) == 0 {
            bail!("the printer produced an empty file");
        }
        Ok(())
    })();

    let _ = window.destroy();
    let _ = std::fs::remove_file(&html_path);
    result
}
