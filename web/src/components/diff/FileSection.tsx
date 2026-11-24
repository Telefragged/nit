import { ChevronDown, ChevronRight } from "lucide-react";
import type { ReactNode, Ref } from "react";

/** One file section: its header, then `children`, the body. A collapsed
 * section has no body.
 *
 * The header shows `mark` in the status badge of `letter`, then
 * `title`, then `controls`. A click on the header, outside a link or a
 * button of it, calls `onToggle`. `path` is the path of a file of a
 * diff. */
export default function FileSection({
  ref,
  domId,
  path,
  collapsed,
  onToggle,
  letter,
  mark,
  title,
  controls,
  children,
}: {
  ref?: Ref<HTMLElement>;
  domId?: string;
  path?: string;
  collapsed: boolean;
  onToggle: () => void;
  letter?: string;
  mark: ReactNode;
  title: ReactNode;
  controls?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section
      ref={ref}
      className={`file-section ${collapsed ? "collapsed" : ""}`}
      id={domId}
      data-diff-path={path}
    >
      <header
        className="file-header"
        role="button"
        tabIndex={0}
        onClick={(e) => {
          if (!(e.target as Element).closest("a, button")) onToggle();
        }}
        onKeyDown={(e) => {
          if (e.target !== e.currentTarget) return;
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            onToggle();
          }
        }}
        aria-expanded={!collapsed}
        title={collapsed ? "Expand file" : "Collapse file"}
      >
        {collapsed ? (
          <ChevronRight className="fchevron" size={14} aria-hidden />
        ) : (
          <ChevronDown className="fchevron" size={14} aria-hidden />
        )}
        <span className={letter ? `fstat fstat-${letter}` : "fstat"}>
          {mark}
        </span>
        <span className="fpath">{title}</span>
        {controls}
      </header>
      {children}
    </section>
  );
}
