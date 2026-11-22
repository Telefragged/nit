import { X } from "lucide-react";
import type { ReactNode } from "react";
import { Button } from "../ui/button";

export default function FloatingPanel({
  label,
  onClose,
  children,
}: {
  label: string;
  onClose: () => void;
  children: ReactNode;
}) {
  return (
    <aside className="floating-panel" aria-label={label}>
      <div className="floating-head">
        <span className="floating-label mono">{label}</span>
        <Button
          variant="ghost"
          size="icon"
          aria-label="Close"
          onClick={onClose}
        >
          <X size={14} aria-hidden />
        </Button>
      </div>
      {children}
    </aside>
  );
}
