import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { FolderOpen } from "lucide-react";
import { api } from "../../lib/ipc";
import type { AppInfo, Settings } from "../../lib/types";
import { Button, Section, Segmented, Slider, Toggle } from "../../components/ui";

export function SettingsView({ settings }: { settings: Settings }) {
  const [autostart, setAutostart] = useState(false);
  const [info, setInfo] = useState<AppInfo | null>(null);

  useEffect(() => {
    isEnabled().then(setAutostart).catch(() => {});
    api.appInfo().then(setInfo);
  }, []);

  const set = (patch: Partial<Settings>) => api.setSettings({ ...settings, ...patch });

  const chooseFolder = async () => {
    const dir = await open({ directory: true, title: "Save converted files to" });
    if (typeof dir === "string") set({ outputFolder: dir });
  };

  const toggleAutostart = async (on: boolean) => {
    try {
      if (on) await enable();
      else await disable();
      setAutostart(await isEnabled());
    } catch {
      setAutostart(!on);
    }
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-2.5 overflow-y-auto px-3 pb-3">
      <Section title="Gesture">
        <Toggle
          label="Hold Shift while dragging"
          hint="Shows the wheel. Add Ctrl for tools."
          checked={settings.gestureEnabled}
          onChange={(v) => set({ gestureEnabled: v })}
        />
        <Toggle
          label="Work in every app"
          hint="Not just File Explorer and the desktop"
          checked={settings.anyApp}
          onChange={(v) => set({ anyApp: v })}
        />
        <Toggle label="Start with Windows" checked={autostart} onChange={toggleAutostart} />
      </Section>

      <Section title="Saving">
        <Segmented
          className="my-1.5"
          value={settings.outputFolder ? "folder" : "beside"}
          onChange={(v) => (v === "beside" ? set({ outputFolder: null }) : chooseFolder())}
          options={[
            { value: "beside", label: "Beside the original" },
            { value: "folder", label: "In a folder" },
          ]}
        />
        {settings.outputFolder && (
          <button onClick={chooseFolder} className="mb-1 flex w-full items-center gap-2 rounded-lg px-1 py-1 text-left text-xs text-ink-2 hover:text-ink">
            <FolderOpen className="size-3.5 shrink-0" />
            <span className="truncate">{settings.outputFolder}</span>
          </button>
        )}
        <Toggle label="Show finished files in File Explorer" checked={settings.revealOutputs} onChange={(v) => set({ revealOutputs: v })} />
        <Toggle label="Keep photo metadata" hint="Camera, date, and location travel with the copy" checked={settings.keepMetadata} onChange={(v) => set({ keepMetadata: v })} />
      </Section>

      <Section title="Quality">
        <Slider label="JPEG" min={40} max={100} value={settings.jpegQuality} onChange={(v) => set({ jpegQuality: v })} />
        <Slider label="WebP" min={40} max={100} value={settings.webpQuality} onChange={(v) => set({ webpQuality: v })} />
        <Slider label="HEIC" min={40} max={100} value={settings.heicQuality} onChange={(v) => set({ heicQuality: v })} />
        <Slider label="AVIF" min={30} max={100} value={settings.avifQuality} onChange={(v) => set({ avifQuality: v })} />
        <div className="mt-2 mb-1 text-[12.5px] font-medium text-ink-2">Video</div>
        <Segmented
          value={settings.videoPreset}
          onChange={(v) => set({ videoPreset: v })}
          options={[
            { value: "fast", label: "Faster" },
            { value: "balanced", label: "Balanced" },
            { value: "quality", label: "Best" },
          ]}
        />
      </Section>

      <Section title="Documents">
        <div className="mt-1 mb-1 text-[12.5px] font-medium text-ink-2">Page size</div>
        <Segmented
          value={settings.pageSize}
          onChange={(v) => set({ pageSize: v })}
          options={[
            { value: "a4", label: "A4" },
            { value: "letter", label: "Letter" },
            { value: "fit", label: "Fit image" },
          ]}
        />
        <Slider label="PDF to image resolution" min={72} max={300} step={6} value={settings.pdfDpi} format={(v) => `${v} DPI`} onChange={(v) => set({ pdfDpi: v })} />
      </Section>

      <Section title="Appearance">
        <Segmented
          className="my-1.5"
          value={settings.theme}
          onChange={(v) => set({ theme: v })}
          options={[
            { value: "system", label: "System" },
            { value: "light", label: "Light" },
            { value: "dark", label: "Dark" },
          ]}
        />
        <Toggle label="Reduce motion" checked={settings.reduceMotion} onChange={(v) => set({ reduceMotion: v })} />
      </Section>

      <Section title="About">
        <div className="flex items-center justify-between py-1 text-[12.5px] text-ink-2">
          <span>KiwiConvert {info?.version}</span>
          <span className={info?.ffmpeg && info?.pdf ? "text-accent" : "text-danger"}>
            {info ? (info.ffmpeg && info.pdf ? "All engines ready" : "Some engines are missing") : ""}
          </span>
        </div>
        <p className="py-1 text-xs leading-relaxed text-ink-3">
          Open source under the MIT license. Uses FFmpeg (GPL), PDFium, and other open source components.
        </p>
        <div className="flex gap-2 pt-1">
          <Button size="sm" onClick={() => api.openLink("https://github.com/jherobred/KiwiConvert")}>
            Source code
          </Button>
          <Button size="sm" variant="ghost" onClick={() => api.quit()}>
            Quit KiwiConvert
          </Button>
        </div>
      </Section>
    </div>
  );
}
