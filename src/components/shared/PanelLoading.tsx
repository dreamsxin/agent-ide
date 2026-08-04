import { LoaderCircle } from "lucide-react";

export default function PanelLoading({ label }: { label: string }) {
  return (
    <div
      className="flex h-full min-h-24 items-center justify-center gap-2 text-xs text-surface-muted"
      role="status"
      aria-live="polite"
    >
      <LoaderCircle aria-hidden="true" className="h-4 w-4 animate-spin text-accent-blue" />
      <span>{label}</span>
    </div>
  );
}
