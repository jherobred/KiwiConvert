import { Button } from "../../../components/ui";
import type { ToolProps } from "../ToolApp";
import { Sheet } from "../Sheet";

export default function Placeholder({ close }: ToolProps) {
  return (
    <Sheet title="PDF pages" footer={<Button onClick={close}>Close</Button>}>
      <p className="text-sm text-ink-3">Loading…</p>
    </Sheet>
  );
}
