import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Link, useNavigate, useParams, useSearchParams } from "react-router-dom";
import { createDraft, getChain, getChange, getDiff } from "../api/client";
import type { ChangeDetail, Comment, Review, Revision } from "../api/types";
import { StatusChip } from "../components/badges";
import CommentEditor, { confirmDiscard } from "../components/CommentEditor";
import type { Thread } from "../components/CommentThread";
import CommentThread from "../components/CommentThread";
import DiffFileView from "../components/diff/DiffFileView";
import FileRail, { fileDomId } from "../components/diff/FileRail";
import ReviewBar from "../components/ReviewBar";
import { displayPath } from "../lib/diffview";
import { highlightLine } from "../lib/highlight";
import { timeAgo } from "../lib/time";
import { ErrorPanel } from "./NotFound";
import type { DraftTarget, ReviewCtx } from "./reviewContext";
import { ReviewContext } from "./reviewContext";

const LAYOUT_KEY = "nit.diff-layout";
type Layout = "unified" | "split";

const VERDICT_BADGE: Record<string, { cls: string; label: string }> = {
  approve: { cls: "badge-green", label: "APPROVED" },
  request_changes: { cls: "badge-red", label: "CHANGES REQUESTED" },
  comment: { cls: "badge-blue", label: "COMMENTED" },
};

function buildThreads(comments: Comment[]): Thread[] {
  const roots = comments
    .filter((c) => c.parent_id === null)
    .sort((a, b) => a.created_at.localeCompare(b.created_at));
  return roots.map((root) => ({
    root,
    replies: comments
      .filter((c) => c.parent_id === root.id)
      .sort((a, b) => a.created_at.localeCompare(b.created_at)),
  }));
}

/** Resolve the ?against param into a diff base for the selected revision.
 * Grammar: "base" → explicit full diff vs parent; "M" → interdiff
 * rM → rSelected when 1 <= M < selected (junk falls back to full diff);
 * absent → implicit interdiff since the reviewer's last review when they
 * are behind on the latest revision (the only `implicit: true` case). */
function deriveDiffBase(
  raw: string | null,
  selected: number,
  lastReviewed: number | null,
  latestNumber: number | undefined,
): { against: number | undefined; implicit: boolean } {
  if (raw !== null) {
    const m = Number(raw);
    const valid = Number.isInteger(m) && m >= 1 && m < selected;
    return { against: valid ? m : undefined, implicit: false };
  }
  if (
    lastReviewed !== null &&
    latestNumber !== undefined &&
    lastReviewed < latestNumber &&
    selected === latestNumber
  ) {
    // Reviewer is behind: default to the interdiff since their review.
    return { against: lastReviewed, implicit: true };
  }
  return { against: undefined, implicit: false };
}

/** Gerrit-style diff range: [Base|rM] → [rN]. Left picks the diff base,
 * right the revision under review. */
function DiffRangeSelect({
  revisions,
  selected,
  against,
  onLeft,
  onRight,
}: {
  revisions: Revision[];
  selected: number;
  against: number | undefined;
  onLeft: (v: string) => void;
  onRight: (n: number) => void;
}) {
  return (
    <>
      <select
        className="rev-select"
        aria-label="Diff base"
        title="Base = parent commit; rM = interdiff against revision M"
        value={against === undefined ? "base" : String(against)}
        onChange={(e) => onLeft(e.target.value)}
      >
        <option value="base">Base</option>
        {revisions.map((r) => (
          <option
            key={r.number}
            value={String(r.number)}
            disabled={r.number >= selected}
          >
            r{r.number}
          </option>
        ))}
      </select>
      <span className="dim mono">→</span>
      <select
        className="rev-select"
        aria-label="Revision"
        title="Revision (patchset) under review"
        value={String(selected)}
        onChange={(e) => onRight(Number(e.target.value))}
      >
        {revisions.map((r) => (
          <option key={r.number} value={String(r.number)}>
            r{r.number}
          </option>
        ))}
      </select>
    </>
  );
}

/** One published review line; long cover messages get a more/less toggle. */
function ReviewItem({ review }: { review: Review }) {
  const badge = VERDICT_BADGE[review.verdict]!;
  const [expanded, setExpanded] = useState(false);
  const [truncated, setTruncated] = useState(false);
  const msgRef = useRef<HTMLSpanElement>(null);
  useLayoutEffect(() => {
    const el = msgRef.current;
    if (el) setTruncated(el.scrollWidth > el.clientWidth);
  }, [review.message]);
  return (
    <div className="review-item">
      <span className={`badge ${badge.cls}`}>{badge.label}</span>
      <span className="mono dim">r{review.revision}</span>
      <span
        ref={msgRef}
        className={`review-message ${expanded ? "expanded" : ""}`}
      >
        {review.message}
      </span>
      {truncated || expanded ? (
        <button
          className="linkish review-more"
          onClick={() => setExpanded((v) => !v)}
        >
          {expanded ? "less" : "more"}
        </button>
      ) : null}
      <span className="dim">{timeAgo(review.created_at)}</span>
    </div>
  );
}

function ReviewsStrip({ change }: { change: ChangeDetail }) {
  if (change.reviews.length === 0) return null;
  return (
    <div className="reviews-strip">
      {change.reviews.map((review) => (
        <ReviewItem review={review} key={review.id} />
      ))}
    </div>
  );
}

export default function ReviewPage() {
  const { id } = useParams();
  const changeId = Number(id);
  const [searchParams, setSearchParams] = useSearchParams();

  const revisionParam = searchParams.get("revision")
    ? Number(searchParams.get("revision"))
    : undefined;

  const changeQ = useQuery({
    queryKey: ["change", changeId, revisionParam ?? "latest"],
    queryFn: () => getChange(changeId, revisionParam),
  });
  const change = changeQ.data;

  const chainQ = useQuery({
    queryKey: ["chain", change?.chain_id],
    queryFn: () => getChain(change!.chain_id),
    enabled: change !== undefined,
  });

  const [layout, setLayout] = useState<Layout>(() =>
    localStorage.getItem(LAYOUT_KEY) === "split" ? "split" : "unified",
  );
  const [editingTarget, setEditingTarget] = useState<DraftTarget | null>(null);
  const editorDirty = useRef(false);
  const [activeFile, setActiveFile] = useState<number | null>(null);
  const [changeCommentOpen, setChangeCommentOpen] = useState(false);
  const queryClient = useQueryClient();

  // --- derive revision/diff mode (before any early return: no hooks below)
  const revisions = change?.revisions ?? [];
  const latest = revisions[revisions.length - 1];
  const selectedRev =
    revisions.find((r) => r.number === (revisionParam ?? latest?.number)) ??
    latest;
  const selected = selectedRev?.number ?? 1;

  const againstRaw = searchParams.get("against");
  const lastReviewed = change?.last_reviewed_revision ?? null;
  const { against, implicit } = deriveDiffBase(
    againstRaw,
    selected,
    lastReviewed,
    latest?.number,
  );

  const diffQ = useQuery({
    queryKey: ["diff", changeId, selected, against ?? null],
    queryFn: () => getDiff(changeId, selected, against),
    enabled: change !== undefined && !(selectedRev?.needs_rebase ?? false),
    retry: false,
  });

  const ctxValue: ReviewCtx = useMemo(
    () => ({
      changeId,
      draftRevision: selected,
      interdiff: against !== undefined,
      editingTarget,
      // Moving or clearing the target unmounts the inline CommentEditor and
      // destroys its draft, so this is a discard path: confirm while dirty.
      // Same-anchor clicks keep the editor mounted and are no-ops.
      setEditingTarget: (t) => {
        const cur = editingTarget;
        if (
          t &&
          cur &&
          t.file === cur.file &&
          t.side === cur.side &&
          t.line === cur.line
        ) {
          return;
        }
        if (!confirmDiscard(editorDirty.current)) return;
        setEditingTarget(t);
      },
      editorDirty,
    }),
    [changeId, selected, against, editingTarget],
  );

  const threads = useMemo(
    () => buildThreads(change?.comments ?? []),
    [change?.comments],
  );

  const navigate = useNavigate();
  const chainChanges = chainQ.data?.changes;
  const fileCount = diffQ.data?.files.length ?? 0;

  // Change-level draft (no file/line anchor).
  const createChangeComment = useMutation({
    mutationFn: (body: string) =>
      createDraft(changeId, { revision: selected, body }),
    onSuccess: () => {
      setChangeCommentOpen(false);
      void queryClient.invalidateQueries({ queryKey: ["change", changeId] });
    },
  });

  // Keyboard nav: [ / ] previous/next file, n / p next/previous change.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      const el = e.target as HTMLElement | null;
      if (el && /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName)) return;
      if (e.key === "[" || e.key === "]") {
        if (fileCount === 0) return;
        setActiveFile((prev) => {
          const cur = prev ?? (e.key === "]" ? -1 : fileCount);
          const next = Math.min(
            fileCount - 1,
            Math.max(0, cur + (e.key === "]" ? 1 : -1)),
          );
          document
            .getElementById(fileDomId(next))
            ?.scrollIntoView({ behavior: "smooth", block: "start" });
          return next;
        });
      } else if (e.key === "n" || e.key === "p") {
        if (!chainChanges || !change || change.position === null) return;
        const live = chainChanges
          .filter((c) => c.position !== null)
          .sort((a, b) => a.position! - b.position!);
        const idx = live.findIndex((c) => c.id === change.id);
        if (idx < 0) return;
        const next = live[idx + (e.key === "n" ? 1 : -1)];
        if (next) navigate(`/changes/${next.id}`);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [fileCount, chainChanges, change, navigate]);

  const files = diffQ.data?.files ?? [];
  const threadsByFile = useMemo(() => {
    const map = new Map<string, Thread[]>();
    for (const t of threads) {
      if (t.root.file === null) continue;
      const file = files.find(
        (f) => f.path === t.root.file || f.old_path === t.root.file,
      );
      const key = file ? file.path : t.root.file;
      const list = map.get(key) ?? [];
      list.push(t);
      map.set(key, list);
    }
    return map;
  }, [threads, files]);

  // --- early returns ---------------------------------------------------
  if (changeQ.isError) {
    return (
      <main className="page">
        <ErrorPanel error={changeQ.error} />
      </main>
    );
  }
  if (changeQ.isPending || !change || !latest || !selectedRev) {
    return (
      <main className="page">
        <div className="skeleton" style={{ width: 320, height: 18 }} />
        <div className="skeleton" style={{ width: 200, marginTop: 10 }} />
        <div className="skeleton" style={{ marginTop: 24, height: 260 }} />
      </main>
    );
  }

  const chain = chainQ.data;
  const changeLevelThreads = threads.filter((t) => t.root.file === null);
  const orphanFileThreads = [...threadsByFile.entries()].filter(
    ([path]) => !files.some((f) => f.path === path),
  );

  const updateParams = (patch: Record<string, string | null>) => {
    const next = new URLSearchParams(searchParams);
    for (const [k, v] of Object.entries(patch)) {
      if (v === null) next.delete(k);
      else next.set(k, v);
    }
    setSearchParams(next, { replace: true });
  };

  // Diff range dropdowns. Left writes ?against ("base" | "1".."N-1").
  // Right writes ?revision; a still-valid numeric base is preserved (the
  // dropdowns are independent coordinates, as in Gerrit), an invalid one
  // resets to Base, an explicit "base" is kept.
  const onLeft = (v: string) => updateParams({ against: v });
  const onRight = (n: number) => {
    const patch: Record<string, string | null> = { revision: String(n) };
    if (
      againstRaw !== null &&
      againstRaw !== "base" &&
      deriveDiffBase(againstRaw, n, lastReviewed, latest.number).against ===
        undefined
    )
      patch.against = null; // numeric base not valid for the viewed rev
    updateParams(patch);
  };

  const setLayoutPersist = (l: Layout) => {
    setLayout(l);
    localStorage.setItem(LAYOUT_KEY, l);
  };

  const subjectLine = selectedRev.message.split("\n")[0] ?? change.subject;

  return (
    <ReviewContext.Provider value={ctxValue}>
      <main className="page-wide review-page">
        <div className="review-header">
          <div className="crumb-line">
            <Link to={`/chains/${change.chain_id}`}>
              {chain?.branch ?? `chain ${change.chain_id}`}
            </Link>
            <span className="sep">/</span>
            <span className="dim">
              change {change.position !== null ? change.position + 1 : "—"}
              {chain ? ` of ${chain.changes.length}` : ""}
            </span>
            <span className="sep">·</span>
            <span className="mono dim" title={change.change_key}>
              {change.change_key.slice(0, 12)}
            </span>
          </div>
          <div className="subject-line">
            <h1>{subjectLine}</h1>
            <StatusChip status={change.status} />
          </div>
          <div className="meta-line">
            <span className="dim">
              commit <span className="mono">{selectedRev.short_sha}</span>
            </span>
            <span className="dim">
              parent <span className="mono">{selectedRev.parent_sha.slice(0, 12)}</span>
            </span>
            <span className="dim">{timeAgo(selectedRev.created_at)}</span>
          </div>
          {selectedRev.fixups.length > 0 ? (
            <div className="fixup-list">
              {selectedRev.fixups.map((fixup) => (
                <div className="fixup-item" key={fixup.sha} title={fixup.message}>
                  <span className="badge badge-blue">FIXUP</span>
                  <span className="mono dim">{fixup.short_sha}</span>
                  <span className="fixup-subject">
                    {fixup.message.split("\n")[0]}
                  </span>
                </div>
              ))}
            </div>
          ) : null}
          <ReviewsStrip change={change} />
          {selectedRev.needs_rebase ? (
            <div className="banner banner-error">
              <strong>needs rebase</strong>
              <span className="banner-body">
                Fixup folding conflicted on this revision — the agent must
                restructure before it can be diffed or reviewed.
              </span>
            </div>
          ) : null}
        </div>

        <div className="diffbar">
          <div className="diffbar-mode">
            <DiffRangeSelect
              revisions={revisions}
              selected={selected}
              against={against}
              onLeft={onLeft}
              onRight={onRight}
            />
            {implicit ? (
              <span className="dim">— changes since your review</span>
            ) : null}
          </div>
          <div className="diffbar-toggles">
            <button
              className="linkish change-comment-btn"
              onClick={() => setChangeCommentOpen(true)}
            >
              + change comment
            </button>
            <span
              className="kbd-hint"
              title="Keyboard: [ and ] switch files, n and p switch changes"
            >
              <kbd>[</kbd>
              <kbd>]</kbd> files · <kbd>n</kbd>
              <kbd>p</kbd> changes
            </span>
            <span className="seg">
              <button
                className={layout === "unified" ? "active" : ""}
                onClick={() => setLayoutPersist("unified")}
              >
                Unified
              </button>
              <button
                className={layout === "split" ? "active" : ""}
                onClick={() => setLayoutPersist("split")}
              >
                Side-by-side
              </button>
            </span>
          </div>
        </div>

        <div className="review-layout">
          <FileRail
            files={files}
            threadsByFile={threadsByFile}
            activeIndex={activeFile}
            onSelect={(i) => {
              setActiveFile(i);
              document
                .getElementById(fileDomId(i))
                ?.scrollIntoView({ behavior: "smooth", block: "start" });
            }}
          />
          <div className="diff-column">
            {changeLevelThreads.length > 0 || changeCommentOpen ? (
              <section className="change-threads">
                <div className="outdated-title">Change discussion</div>
                {changeLevelThreads.map((t) => (
                  <CommentThread
                    key={t.root.id}
                    thread={t}
                    changeId={changeId}
                    draftRevision={selected}
                  />
                ))}
                {changeCommentOpen ? (
                  <CommentEditor
                    placeholder="Comment on the whole change…"
                    saving={createChangeComment.isPending}
                    onSave={(body) => createChangeComment.mutate(body)}
                    onCancel={() => setChangeCommentOpen(false)}
                  />
                ) : null}
              </section>
            ) : null}

            {selectedRev.needs_rebase ? (
              <div className="empty-state">
                Diff unavailable while the revision needs a rebase.
              </div>
            ) : diffQ.isError ? (
              <ErrorPanel error={diffQ.error} />
            ) : diffQ.isPending ? (
              <div>
                <div className="skeleton" style={{ height: 14 }} />
                <div className="skeleton" style={{ height: 14, marginTop: 8, width: "80%" }} />
                <div className="skeleton" style={{ height: 14, marginTop: 8, width: "90%" }} />
              </div>
            ) : files.length === 0 ? (
              <div className="empty-state">Empty diff — no file changes.</div>
            ) : (
              files.map((file, i) => (
                <DiffFileView
                  key={file.path}
                  file={file}
                  layout={layout}
                  threads={threadsByFile.get(file.path) ?? []}
                  domId={fileDomId(i)}
                />
              ))
            )}

            {orphanFileThreads.length > 0 ? (
              <section className="leftover-threads">
                <div className="outdated-title">
                  Threads on files outside this diff
                </div>
                {orphanFileThreads.map(([path, fileThreads]) => (
                  <div key={path} className="leftover-file">
                    <div className="leftover-path mono">
                      {displayPath(path)}
                    </div>
                    {fileThreads.map((t) => (
                      <div className="outdated-item" key={t.root.id}>
                        {t.root.line_text ? (
                          <div className="line-excerpt">
                            <span className="excerpt-line">
                              r{t.root.revision}
                              {t.root.line !== null ? `:${t.root.line}` : ""}
                            </span>
                            <span
                              dangerouslySetInnerHTML={{
                                __html: highlightLine(t.root.line_text, null),
                              }}
                            />
                          </div>
                        ) : null}
                        <CommentThread
                          thread={t}
                          changeId={changeId}
                          draftRevision={selected}
                        />
                      </div>
                    ))}
                  </div>
                ))}
              </section>
            ) : null}
          </div>
        </div>

        <ReviewBar change={change} chain={chain} selectedRevision={selected} />
      </main>
    </ReviewContext.Provider>
  );
}
