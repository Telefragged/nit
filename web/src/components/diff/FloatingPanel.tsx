import type { ReactNode } from "react";
import { CloseButton } from "../ui/button";

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
        <CloseButton onClick={onClose} />
      </div>
      {children}
    </aside>
  );
}
