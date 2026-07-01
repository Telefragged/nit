import {
  skipToken,
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { flushSync } from "react-dom";
import {
  Link,
  useNavigate,
  useParams,
  useSearchParams,
} from "react-router-dom";
import { createDraft, getChain, getDiff, getRepo } from "../api/client";
import type { ChangeDetail, Decision, Review, Revision } from "../api/types";
import { verdictStatus } from "../api/verdict";
import { StatusChip } from "../components/badges";
import ChainNav from "../components/ChainNav";
import CommentEditor from "../components/CommentEditor";
import CommentThread from "../components/CommentThread";
import DiffFileView from "../components/diff/DiffFileView";
import FileRail from "../components/diff/FileRail";
import ReviewBar from "../components/ReviewBar";
import {
  allExpanded,
  collapseAll,
  defaultExpanded,
  expand,
  expandAll,
  toggle,
} from "../lib/collapse";
import {
  assembleThreads,
  commentCountLabel,
  commentPlacement,
  pendingUnresolvedCount,
  threadCountByRevision,
  threadKey,
  type UiThread,
} from "../lib/comments";
import { confirmDiscard } from "../lib/confirmDiscard";
import { displayPath, fileDomId } from "../lib/diffview";
import { highlightLine } from "../lib/highlight";
import { repoPath } from "../lib/repo";
import { activeIndexAt } from "../lib/scrollspy";
import { isShortcutKey } from "../lib/shortcutKey";
import type { SelectionMiss } from "../lib/selection";
import { selectionAnchorSide, selectionTarget } from "../lib/selection";
import { timeAgo } from "../lib/time";
import { useChangeDetails } from "../lib/useChangeDetails";
import { useChangeStream } from "../lib/useChangeStream";
import { useDrafts } from "../lib/useDrafts";
import { ErrorPanel } from "./NotFound";
import type { DraftTarget, ReviewCtx } from "./reviewContext";
import { ReviewContext, sameTarget } from "./reviewContext";

const LAYOUT_KEY = "nit.diff-layout";
type Layout = "unified" | "split";

/** Why `c` did nothing — several misses are policy, not user error, so
 * they deserve words (docs/frontend.md). */
const MISS_TEXT: Record<SelectionMiss["miss"], string> = {
  "mixed-sides": "selection doesn't lie on one side of the diff",
  "cross-file": "selection crosses file sections",
  "hunk-gap": "selection spans a hunk gap",
};

/** Resolve the ?against param into a diff base for the selected revision.
 * Grammar: "base" or absent → full diff vs parent; "M" → interdiff
 * rM → rSelected when 0 <= M < selected (junk falls back to the full diff). */
function deriveDiffBase(
  raw: string | null,
  selected: number,
): number | undefined {
  if (raw === null) return undefined;
  const m = Number(raw);
  const valid = Number.isInteger(m) && m >= 0 && m < selected;
  return valid ? m : undefined;
}

/** Gerrit-style diff range: [Base|rM] → [rN]. Left picks the diff base,
 * right the revision under review. Each rN option is tagged with its own
 * comment-thread count (`counts`) so the reviewer sees where discussion
 * sits before switching — native <option> takes plain text only, so it
 * reads "r2 · 3 comments", not the styled label the file headers use. */
function DiffRangeSelect({
  revisions,
  selected,
  against,
  counts,
  onLeft,
  onRight,
}: {
  revisions: Revision[];
  selected: number;
  against: number | undefined;
  counts: Map<number, number>;
  onLeft: (v: string) => void;
  onRight: (n: number) => void;
}) {
  const label = (r: Revision) => {
    const n = counts.get(r.number) ?? 0;
    return n > 0 ? `r${r.number} · ${commentCountLabel(n)}` : `r${r.number}`;
  };
  return (
    <>
      <select
        className="rev-select"
        aria-label="Diff base"
        title="Base = parent commit; rM = interdiff against revision M"
        value={against === undefined ? "base" : String(against)}
        onChange={(e) => {
          onLeft(e.target.value);
        }}
      >
        <option value="base">Base</option>
        {revisions.map((r) => (
          <option
            key={r.number}
            value={String(r.number)}
            disabled={r.number >= selected}
          >
            {label(r)}
          </option>
        ))}
      </select>
      <span className="dim mono">→</span>
      <select
        className="rev-select"
        aria-label="Revision"
        title="Revision (patchset) under review"
        value={String(selected)}
        onChange={(e) => {
          onRight(Number(e.target.value));
        }}
      >
        {revisions.map((r) => (
          <option key={r.number} value={String(r.number)}>
            {label(r)}
          </option>
        ))}
      </select>
    </>
  );
}

/** One published review line; long cover messages get a more/less toggle. */
function ReviewItem({ review }: { review: Review }) {
  const [expanded, setExpanded] = useState(false);
  const [truncated, setTruncated] = useState(false);
  const msgRef = useRef<HTMLSpanElement>(null);
  useLayoutEffect(() => {
    const el = msgRef.current;
    if (el) setTruncated(el.scrollWidth > el.clientWidth);
  }, [review.message]);
  return (
    <div className="review-item">
      <StatusChip status={verdictStatus[review.verdict]} />
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
          onClick={() => {
            setExpanded((v) => !v);
          }}
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

  // The change page is event-driven: the published projection (revisions,
  // threads, reviews) is folded from the change-event websocket into the
  // ["change", id] cache by `useChangeStream` (below). This query only reads
  // that cache (skipToken — never fetches). The chain structure, the diff, the
  // repo, and the reviewer's drafts/staged decision are not in the log, so they
  // stay REST; `change` composes the published projection with the drafts
  // overlay (`useDrafts`).
  const queryClient = useQueryClient();

  const publishedQ = useQuery<ChangeDetail>({
    queryKey: ["change", changeId],
    queryFn: skipToken,
  });
  const published = publishedQ.data;

  // The repo behind this change — the crumb shows its path. Fetched by id once
  // the projection arrives; skipToken holds the hook's place until then.
  const repoQ = useQuery({
    queryKey: ["repo", published?.repo_id],
    queryFn: published ? () => getRepo(published.repo_id) : skipToken,
  });

  const [layout, setLayout] = useState<Layout>(() =>
    localStorage.getItem(LAYOUT_KEY) === "split" ? "split" : "unified",
  );
  const [editingTarget, setEditingTarget] = useState<DraftTarget | null>(null);
  const editorDirty = useRef(false);
  const diffColumnRef = useRef<HTMLDivElement>(null);
  const [activeFile, setActiveFile] = useState<number | null>(null);
  const [changeCommentOpen, setChangeCommentOpen] = useState(false);
  const [replyOpen, setReplyOpen] = useState(false);

  // Transient "why c did nothing" notice; a fresh object per press keeps
  // the timeout effect retriggering on repeated identical misses.
  const [selectionMiss, setSelectionMiss] = useState<SelectionMiss | null>(
    null,
  );
  useEffect(() => {
    if (selectionMiss === null) return undefined;
    const timer = setTimeout(() => {
      setSelectionMiss(null);
    }, 4000);
    return () => {
      clearTimeout(timer);
    };
  }, [selectionMiss]);

  // --- derive revision/diff mode (before any early return: no hooks below)
  const revisions = published?.revisions ?? [];
  const latest = revisions[revisions.length - 1];
  const selectedRev =
    revisions.find((r) => r.number === (revisionParam ?? latest?.number)) ??
    latest;
  const selected = selectedRev?.number ?? 1;

  // The chain context is the derived chain through this change rooted at the
  // viewed revision — the path that pins `selected`. Fetched with the revision
  // so switching patchsets re-roots onto that revision's chain.
  const chainQ = useQuery({
    queryKey: ["chain", changeId, selected],
    queryFn: published ? () => getChain(changeId, selected) : skipToken,
  });
  const memberIds = (chainQ.data?.path ?? []).map((m) => m.change_id);

  // Subscribe to the change and every chain member: each snapshot + live fold is
  // written into the ["change", id] cache the queries above read. The drafts
  // overlay rides a separate ["drafts", id] read; `change` composes the two.
  useChangeStream([changeId, ...memberIds]);
  const draftsMap = useDrafts([changeId, ...memberIds]);
  const overlay = draftsMap.get(changeId);
  const change = useMemo(
    () =>
      published
        ? {
            ...published,
            drafts: overlay?.drafts ?? [],
            draft_decision: overlay?.draft_decision ?? null,
          }
        : undefined,
    [published, overlay],
  );

  // Pin the viewed revision into the URL on first load. A new revision arriving
  // over the websocket then becomes selectable (it joins `revisions`) without
  // the view jumping to it — `selected` follows the pinned ?revision, not the
  // moving latest. The pin is to the revision already shown, so nothing moves.
  const latestNumber = latest?.number;
  useEffect(() => {
    if (revisionParam === undefined && latestNumber !== undefined) {
      setSearchParams(
        (prev) => {
          const next = new URLSearchParams(prev);
          next.set("revision", String(latestNumber));
          return next;
        },
        { replace: true },
      );
    }
  }, [revisionParam, latestNumber, setSearchParams]);

  // Each chain member's published projection comes from the ["change", id]
  // cache the stream keeps live (ChainNav reads each member's
  // unresolved/latest-revision); its staged decision comes from the member's
  // drafts overlay, for the review bar's chain-wide "Submit (k)" count + nav.
  const memberDetails = useChangeDetails(memberIds, true);
  const memberDecisions = new Map<number, Decision | null>();
  draftsMap.forEach((d, id) => {
    memberDecisions.set(id, d.draft_decision?.decision ?? null);
  });

  const againstRaw = searchParams.get("against");
  const against = deriveDiffBase(againstRaw, selected);

  const diffQ = useQuery({
    queryKey: ["diff", changeId, selected, against ?? null],
    queryFn: () => getDiff(changeId, selected, against),
    enabled: published !== undefined,
    retry: false,
  });
  const files = useMemo(() => diffQ.data?.files ?? [], [diffQ.data]);

  // Collapsed-by-default file sections. Expansion is keyed by file path
  // and reset whenever a different diff is shown (other change, revision
  // or base); only the commit message starts expanded (lib/collapse.ts).
  const [expanded, setExpanded] =
    useState<ReadonlySet<string>>(defaultExpanded);
  const diffIdentity = `${changeId}:${selected}:${against ?? "base"}`;
  const [shownDiff, setShownDiff] = useState(diffIdentity);
  if (shownDiff !== diffIdentity) {
    // Adjust-during-render, not an effect: the reset is part of the same
    // render that switches diffs, so stale expansion never paints.
    setShownDiff(diffIdentity);
    setExpanded(defaultExpanded());
  }

  /** Reveal a file: activate + expand it, then scroll to it. The expansion
   * is committed with flushSync first, because scrollIntoView positions
   * against the layout at call time and expanding a section reflows
   * everything below it — scrolling before the commit would target the
   * pre-expansion position and land wrong. Shared by rail clicks and the
   * [ / ] keys (both event handlers, where flushSync is safe). */
  const revealFile = useCallback(
    (index: number) => {
      const path = files[index]?.path;
      flushSync(() => {
        setActiveFile(index);
        if (path !== undefined) setExpanded((cur) => expand(cur, path));
      });
      document
        .getElementById(fileDomId(index))
        ?.scrollIntoView({ behavior: "smooth", block: "start" });
    },
    [files],
  );

  const ctxValue: ReviewCtx = useMemo(
    () => ({
      changeId,
      selected,
      against,
      editingTarget,
      // Moving or clearing the target unmounts the inline CommentEditor and
      // destroys its draft, so this is a discard path: confirm while dirty.
      // Same-anchor calls are no-ops; a move within the same file/side/line
      // (a re-selected range) keeps the editor mounted — same React key —
      // so nothing is discarded and no confirmation is owed.
      setEditingTarget: (t) => {
        const cur = editingTarget;
        if (t && cur && sameTarget(t, cur)) return true;
        const sameCell =
          t !== null &&
          cur !== null &&
          t.file === cur.file &&
          t.side === cur.side &&
          t.line === cur.line;
        if (!sameCell && !confirmDiscard(editorDirty.current)) return false;
        setEditingTarget(t);
        return true;
      },
      setEditorDirty: (dirty: boolean) => {
        editorDirty.current = dirty;
      },
    }),
    [changeId, selected, against, editingTarget],
  );

  // The reviewer's view of every thread: published threads merged with their
  // pending drafts, plus draft-only new threads (lib/comments). Assembled
  // once and reused for the diff grouping and the per-revision counts.
  const threads = useMemo(
    () => assembleThreads(change?.threads ?? [], change?.drafts ?? []),
    [change?.threads, change?.drafts],
  );

  // Per-revision thread totals for the diff-range dropdowns (not filtered
  // by the shown range — each revision's own count).
  const revisionCommentCounts = useMemo(
    () => threadCountByRevision(threads),
    [threads],
  );

  const navigate = useNavigate();
  const chainPath = chainQ.data?.path;
  const fileCount = files.length;

  const createChangeComment = useMutation({
    mutationFn: (body: string) =>
      createDraft(changeId, { revision: selected, body }),
    onSuccess: () => {
      setChangeCommentOpen(false);
      void queryClient.invalidateQueries({ queryKey: ["drafts", changeId] });
    },
  });

  // Keyboard nav: [ / ] previous/next file (revealed like a rail click:
  // expanded, then scrolled), n / p next/previous change, c comments on
  // the selected diff text, a opens the reply modal. All inert while the
  // modal is open — it is a showModal() dialog, so it owns the keyboard
  // (Escape arrives as its cancel event) and the page behind it is inert.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (replyOpen || !isShortcutKey(e)) return;
      if (e.key === "[" || e.key === "]") {
        if (fileCount === 0) return;
        const cur = activeFile ?? (e.key === "]" ? -1 : fileCount);
        const next = Math.min(
          fileCount - 1,
          Math.max(0, cur + (e.key === "]" ? 1 : -1)),
        );
        revealFile(next);
      } else if (e.key === "n" || e.key === "p") {
        if (!chainPath) return;
        const idx = chainPath.findIndex((c) => c.change_id === changeId);
        if (idx < 0) return;
        const next = chainPath[idx + (e.key === "n" ? 1 : -1)];
        if (next) void navigate(`/changes/${next.change_id}`);
      } else if (e.key === "c") {
        // Draft a comment on the selected diff text (gerrit's c) — or on
        // the caret's line when the selection is collapsed.
        const sel = document.getSelection();
        if (!sel || sel.rangeCount === 0) return;
        const result = selectionTarget(sel.getRangeAt(0));
        if (!result) return;
        // preventDefault, or the keystroke lands in the editor's textarea.
        e.preventDefault();
        if ("miss" in result) {
          setSelectionMiss(result);
          return;
        }
        // The editor renders its own range highlight; the DOM selection
        // would just shout over it. Keep it on a declined discard.
        if (ctxValue.setEditingTarget(result)) sel.removeAllRanges();
      } else if (e.key === "a") {
        // preventDefault, or the keystroke's own text insertion lands in
        // the cover-message textarea the opening modal focuses.
        e.preventDefault();
        setReplyOpen(true);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, [
    fileCount,
    activeFile,
    revealFile,
    chainPath,
    changeId,
    navigate,
    replyOpen,
    ctxValue,
  ]);

  // Side-by-side selection paint: tag the diff column with the side the
  // current selection's anchor sits in, so styles/diff.css can blank the other
  // column's ::selection. The interleaved subgrid makes a one-column drag's
  // DOM range sweep the other column's cells too — without this they light
  // up even though they are not part of the selected text. Cleared when the
  // selection collapses or leaves the diff (unified diffs have no
  // data-side, so the attribute never gets set there).
  useEffect(() => {
    const onSelectionChange = () => {
      const col = diffColumnRef.current;
      if (!col) return;
      const sel = document.getSelection();
      const side =
        sel && !sel.isCollapsed ? selectionAnchorSide(sel.anchorNode) : null;
      // selectionchange fires continuously through a drag; only touch the
      // attribute (and the style recalc it triggers across the diff) when
      // the side actually flips.
      if (side === col.getAttribute("data-sel-side")) return;
      if (side) col.setAttribute("data-sel-side", side);
      else col.removeAttribute("data-sel-side");
    };
    document.addEventListener("selectionchange", onSelectionChange);
    return () => {
      document.removeEventListener("selectionchange", onSelectionChange);
    };
  }, []);

  // Scroll spy: keep activeFile — the rail highlight and the [ / ] cursor —
  // on the file section currently under the sticky chrome. The threshold is
  // the sections' scroll-margin-top, read from computed style so the sticky
  // offsets live only in styles/review.css; it is the exact line scrollIntoView
  // targets, so a rail click / keystroke and the spy agree on the
  // destination file instead of fighting (+1 absorbs fractional scrolls).
  // During smooth programmatic scrolls the highlight follows live rather
  // than being suppressed: the spy's fixed point is the scroll target, so
  // the sweep self-corrects on arrival with no settle bookkeeping.
  useEffect(() => {
    if (fileCount === 0) return;
    let raf = 0;
    const onScroll = () => {
      if (raf) return;
      raf = requestAnimationFrame(() => {
        raf = 0;
        const sections = Array.from({ length: fileCount }, (_, i) =>
          document.getElementById(fileDomId(i)),
        ).filter((el) => el !== null);
        const first = sections[0];
        if (!first) return;
        const threshold =
          parseFloat(getComputedStyle(first).scrollMarginTop) + 1;
        setActiveFile(
          activeIndexAt(
            sections.map((el) => el.getBoundingClientRect().top),
            threshold,
          ),
        );
      });
    };
    window.addEventListener("scroll", onScroll, { passive: true });
    onScroll(); // initialize for restored scroll positions
    return () => {
      window.removeEventListener("scroll", onScroll);
      cancelAnimationFrame(raf);
    };
  }, [fileCount]);

  // File threads shown in the current diff range: a line comment whose
  // (revision, side) is one of the displayed columns, or a file-level
  // comment (no column to filter by). Everything pinned to another
  // revision drops out — of the diff, the rail counts, and the leftover
  // group alike (docs/api.md "Comment placement").
  const threadsByFile = useMemo(() => {
    const map = new Map<string, UiThread[]>();
    for (const t of threads) {
      if (t.file === null) continue;
      if (t.line !== null && commentPlacement(t, selected, against) === null)
        continue;
      const file = files.find(
        (f) => f.path === t.file || f.old_path === t.file,
      );
      const key = file ? file.path : t.file;
      const list = map.get(key) ?? [];
      list.push(t);
      map.set(key, list);
    }
    return map;
  }, [threads, files, selected, against]);

  // The change's published projection arrives over the websocket (no fetch to
  // error on); a bad change id surfaces when its chain REST read fails.
  if (chainQ.isError) {
    return (
      <main className="page">
        <ErrorPanel error={chainQ.error} />
      </main>
    );
  }
  if (!change || !latest || !selectedRev) {
    return (
      <main className="page">
        <div className="skeleton" style={{ width: 320, height: 18 }} />
        <div className="skeleton" style={{ width: 200, marginTop: 10 }} />
        <div className="skeleton" style={{ marginTop: 24, height: 260 }} />
      </main>
    );
  }

  const chain = chainQ.data;
  const repo = repoQ.data;
  // Position and displayed status for this change are on the path entry, not
  // on ChangeDetail.
  const here = chain?.path.find((c) => c.change_id === change.id);
  const allFilesExpanded = allExpanded(expanded, files);

  /** Collapsing the section that hosts the open inline CommentEditor
   * unmounts it and destroys its draft — the same discard path the guarded
   * setEditingTarget covers: confirm while dirty. `hidesEditor` says
   * whether the attempted collapse covers the editor's section; returns
   * false when the user keeps their text, and the caller must abort the
   * collapse (no state change). On an accepted discard the target is
   * cleared too — left in place, re-expanding the file would resurrect an
   * empty editor at the stale anchor. */
  const confirmEditorCollapse = (hidesEditor: boolean): boolean => {
    if (!hidesEditor) return true;
    if (!confirmDiscard(editorDirty.current)) return false;
    editorDirty.current = false;
    setEditingTarget(null);
    return true;
  };
  const changeLevelThreads = threads.filter((t) => t.file === null);
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
  // An open editor's anchor is its *visual* column, which a range switch
  // would silently re-map to a different (revision, side) at save time
  // (lib/comments draftAnchor). Confirm-and-clear it first — the same
  // discard guard collapse uses — instead of re-anchoring behind the user.
  const switchRange = (patch: Record<string, string | null>) => {
    if (editingTarget && !confirmEditorCollapse(true)) return;
    updateParams(patch);
  };
  const onLeft = (v: string) => {
    switchRange({ against: v });
  };
  const onRight = (n: number) => {
    const patch: Record<string, string | null> = { revision: String(n) };
    if (
      againstRaw !== null &&
      againstRaw !== "base" &&
      deriveDiffBase(againstRaw, n) === undefined
    )
      patch.against = null;
    switchRange(patch);
  };

  const setLayoutPersist = (l: Layout) => {
    setLayout(l);
    localStorage.setItem(LAYOUT_KEY, l);
  };

  const subjectLine = selectedRev.message.split("\n")[0] ?? "";

  return (
    <ReviewContext.Provider value={ctxValue}>
      <main className="page-wide review-page">
        <div className="review-header">
          <div className="crumb-line">
            <Link to={`/repos/${change.repo_id}`}>
              {repo ? repoPath(repo.git_dir) : `repo ${change.repo_id}`}
            </Link>
            <span className="sep">/</span>
            <span className="dim">
              change {here ? here.position + 1 : "—"}
              {chain ? ` of ${chain.path.length}` : ""}
            </span>
            <span className="sep">·</span>
            <span className="mono dim" title={change.change_key}>
              {change.change_key.slice(0, 12)}
            </span>
          </div>
          <div className="subject-line">
            <h1>{subjectLine}</h1>
            {here ? <StatusChip status={here.status} /> : null}
          </div>
          <div className="meta-line">
            <span className="dim">
              commit{" "}
              <span className="mono">
                {selectedRev.commit_sha.slice(0, 12)}
              </span>
            </span>
            <span className="dim">
              parent{" "}
              <span className="mono">
                {selectedRev.parent_sha.slice(0, 12)}
              </span>
            </span>
            <span className="dim">{timeAgo(selectedRev.created_at)}</span>
          </div>
          <ReviewsStrip change={change} />
        </div>

        <div className="diffbar">
          <div className="diffbar-mode">
            <DiffRangeSelect
              revisions={revisions}
              selected={selected}
              against={against}
              counts={revisionCommentCounts}
              onLeft={onLeft}
              onRight={onRight}
            />
          </div>
          <div className="diffbar-toggles">
            {selectionMiss ? (
              <span className="selection-miss">
                {MISS_TEXT[selectionMiss.miss]}
              </span>
            ) : null}
            <button
              className="linkish change-comment-btn"
              onClick={() => {
                setChangeCommentOpen(true);
              }}
            >
              + change comment
            </button>
            <span
              className="kbd-hint"
              title="Keyboard: [ and ] switch files, n and p switch changes, c comments on the selected diff text, a opens the reply dialog"
            >
              <kbd>[</kbd>
              <kbd>]</kbd> files · <kbd>n</kbd>
              <kbd>p</kbd> changes · <kbd>c</kbd> comment · <kbd>a</kbd> reply
            </span>
            <span className="seg">
              <button
                className={layout === "unified" ? "active" : ""}
                onClick={() => {
                  setLayoutPersist("unified");
                }}
              >
                Unified
              </button>
              <button
                className={layout === "split" ? "active" : ""}
                onClick={() => {
                  setLayoutPersist("split");
                }}
              >
                Side-by-side
              </button>
            </span>
          </div>
        </div>

        <div className="review-layout">
          <aside className="review-sidebar">
            <ChainNav
              chain={chain}
              currentId={changeId}
              memberDetails={memberDetails}
            />
            <FileRail
              files={files}
              threadsByFile={threadsByFile}
              activeIndex={activeFile}
              onSelect={revealFile}
              allExpanded={allFilesExpanded}
              onToggleAll={() => {
                if (
                  !confirmEditorCollapse(
                    allFilesExpanded && editingTarget !== null,
                  )
                )
                  return;
                setExpanded(
                  allFilesExpanded ? collapseAll() : expandAll(files),
                );
              }}
            />
          </aside>
          <div className="diff-column" ref={diffColumnRef}>
            {changeLevelThreads.length > 0 || changeCommentOpen ? (
              <section className="change-threads">
                <div className="outdated-title">Change discussion</div>
                {changeLevelThreads.map((t) => (
                  <CommentThread
                    key={threadKey(t)}
                    thread={t}
                    changeId={changeId}
                  />
                ))}
                {changeCommentOpen ? (
                  <CommentEditor
                    placeholder="Comment on the whole change…"
                    saving={createChangeComment.isPending}
                    onSave={(body) => {
                      createChangeComment.mutate(body);
                    }}
                    onCancel={() => {
                      setChangeCommentOpen(false);
                    }}
                  />
                ) : null}
              </section>
            ) : null}

            {diffQ.isError ? (
              <ErrorPanel error={diffQ.error} />
            ) : diffQ.isPending ? (
              <div>
                <div className="skeleton" style={{ height: 14 }} />
                <div
                  className="skeleton"
                  style={{ height: 14, marginTop: 8, width: "80%" }}
                />
                <div
                  className="skeleton"
                  style={{ height: 14, marginTop: 8, width: "90%" }}
                />
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
                  collapsed={!expanded.has(file.path)}
                  onToggle={() => {
                    if (
                      !confirmEditorCollapse(
                        expanded.has(file.path) &&
                          editingTarget?.file === file.path,
                      )
                    )
                      return;
                    setExpanded((cur) => toggle(cur, file.path));
                  }}
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
                      <div className="outdated-item" key={threadKey(t)}>
                        {t.line_text ? (
                          <div className="line-excerpt">
                            <span className="excerpt-line">
                              r{t.revision}
                              {t.line !== null ? `:${t.line}` : ""}
                            </span>
                            <span
                              dangerouslySetInnerHTML={{
                                __html: highlightLine(t.line_text, null),
                              }}
                            />
                          </div>
                        ) : null}
                        <CommentThread thread={t} changeId={changeId} />
                      </div>
                    ))}
                  </div>
                ))}
              </section>
            ) : null}
          </div>
        </div>

        <ReviewBar
          change={change}
          chain={chain}
          memberDecisions={memberDecisions}
          selectedRevision={selected}
          unresolved={pendingUnresolvedCount(threads)}
          replyOpen={replyOpen}
          onReplyOpenChange={setReplyOpen}
        />
      </main>
    </ReviewContext.Provider>
  );
}
