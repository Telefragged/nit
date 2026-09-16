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
import { Link, useNavigate, useParams } from "react-router-dom";
import { createDraft, getChangeDrafts, getDiff, getRepo } from "../api/client";
import { tagGraph } from "../api/fold";
import type {
  ChangeDetail,
  Review,
  Revision,
  Subscription,
  Tags,
} from "../api/types";
import { verdictStatus } from "../api/verdict";
import { StatusChip } from "../components/badges";

import TagNav from "../components/TagNav";
import CommentEditor from "../components/CommentEditor";
import CommentThread from "../components/CommentThread";
import DiffFileView from "../components/diff/DiffFileView";
import FileRail from "../components/diff/FileRail";
import ReviewBar from "../components/ReviewBar";
import ReviewSettingsMenu from "../components/ReviewSettings";
import {
  allExpanded,
  collapseAll,
  defaultExpanded,
  expand,
  expandAll,
  toggle,
} from "../lib/collapse";
import {
  anchorAt,
  anchorFile,
  anchorLineText,
  assembleThreads,
  commentCountLabel,
  nodeActivity,
  placementLine,
  pendingUnresolvedCount,
  threadCountByRevision,
  threadInRange,
  threadKey,
  type DiffRange,
  type UiThread,
} from "../lib/comments";
import { confirmDiscard } from "../lib/confirmDiscard";
import { displayPath, fileDomId, treeOrder } from "../lib/diffview";
import { highlight } from "../lib/highlight";
import { repoPath } from "../lib/repo";
import { activeIndexAt } from "../lib/scrollspy";
import { shortcutKey } from "../lib/shortcutKey";
import type { SelectionMiss } from "../lib/selection";
import { selectionAnchorSide, selectionTarget } from "../lib/selection";
import { timeAgo } from "../lib/time";
import { useChangeStream } from "../lib/useChangeStream";
import { useDrafts } from "../lib/useDrafts";
import { useReviewSettings } from "../lib/reviewSettings";
import { useUrlParams } from "../lib/useUrlParams";
import { ErrorPanel } from "./NotFound";
import { targetLine, type DraftTarget, type ReviewCtx } from "./reviewContext";
import { ReviewContext, sameTarget } from "./reviewContext";

const TAG_KEY = "nit.review-tag";

/** The tag the sidebar follows: the reviewer's last key when the change
 * carries it, else `session-id`, else the first key. Null only when the
 * change carries no tag. */
function selectTag(
  tags: Tags,
  preferred: string | null,
): [string, string] | null {
  const entries = Object.entries(tags);
  return (
    entries.find(([key]) => key === preferred) ??
    entries.find(([key]) => key === "session-id") ??
    entries[0] ??
    null
  );
}

/** Why `c` drafted nothing — each names the rule the selection broke and
 * the selection that satisfies it. */
const MISS_TEXT: Record<SelectionMiss["miss"], string> = {
  "mixed-sides":
    "A comment can only span one side of the diff. Select from the old or the new side, not both.",
  "cross-file": "A comment can only span one file. Select within a file.",
  "hunk-gap":
    "A comment can only span consecutive lines. This selection jumps a hunk gap.",
};

/** Where to hang the miss bubble, in the diff column's own coordinates so
 * it scrolls with the lines it is about: under the rejected selection, and
 * flush with the column its code text starts in — measuring the selection's
 * own left edge would hang the bubble off the line-number gutters, which
 * the union rect of a multi-line range reaches. */
function missAnchor(range: Range, col: DOMRect): { top: number; left: number } {
  const start =
    range.startContainer instanceof Element
      ? range.startContainer
      : range.startContainer.parentElement;
  const text = start?.closest(".code")?.querySelector(".code-text") ?? null;
  return {
    top: range.getBoundingClientRect().bottom - col.top,
    left: (text?.getBoundingClientRect().left ?? col.left) - col.left,
  };
}

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
        className="revision-select"
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
        className="revision-select"
        aria-label="Revision"
        title="Revision under review"
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
  const changeNumber = Number(id);
  const [searchParams, updateParams] = useUrlParams();

  const revisionParam = searchParams.get("revision")
    ? Number(searchParams.get("revision"))
    : undefined;

  // The change page is event-driven: the published projection (revisions,
  // threads, reviews) is folded from the change-event websocket into the
  // ["change", id] cache by `useChangeStream` (below). This query only reads
  // that cache (skipToken — never fetches). The diff, the repo, and the
  // reviewer's drafts/draft decision are not in the log, so they stay REST; `change` composes the published projection with the drafts
  // overlay (`useDrafts`).
  const queryClient = useQueryClient();

  const publishedQ = useQuery<ChangeDetail>({
    queryKey: ["change", changeNumber],
    queryFn: skipToken,
  });
  const published = publishedQ.data;

  // The repo behind this change — the crumb shows its path. Fetched by id once
  // the projection arrives; skipToken holds the hook's place until then.
  const repoQ = useQuery({
    queryKey: ["repo", published?.repo_id],
    queryFn: published ? () => getRepo(published.repo_id) : skipToken,
  });

  const [settings, saveSettings] = useReviewSettings();
  const { mode, layout } = settings;
  const [editingTarget, setEditingTarget] = useState<DraftTarget | null>(null);
  const editorDirty = useRef(false);
  const diffColumnRef = useRef<HTMLDivElement>(null);
  const [activeFile, setActiveFile] = useState<number | null>(null);
  const [changeCommentOpen, setChangeCommentOpen] = useState(false);
  const [replyOpen, setReplyOpen] = useState(false);

  const [selectionMiss, setSelectionMiss] = useState<
    (SelectionMiss & { top: number; left: number; diff: string }) | null
  >(null);

  // --- derive revision/diff mode (before any early return: no hooks below)
  const revisions = published?.revisions ?? [];
  const latest = revisions[revisions.length - 1];

  // The revision the page defaults to (until the URL pins one) is frozen at
  // first sight, not read from the live `latest`: a revision folding in over
  // the websocket must not move it. A revision arriving before the pin effect
  // (below) commits would otherwise slide the default forward and jump the
  // view to the just-pushed revision.
  // Keyed by change (the component is reused across /changes/:id without
  // remounting) via the adjust-during-render idiom used for `shownChange`
  // below.
  const [pinnedRev, setPinnedRev] = useState<{
    changeNumber: number;
    revision: number;
  }>();
  if (latest !== undefined && pinnedRev?.changeNumber !== changeNumber) {
    setPinnedRev({ changeNumber, revision: latest.number });
  }
  const defaultRev =
    pinnedRev?.changeNumber === changeNumber ? pinnedRev.revision : undefined;

  // A revision number is its index.
  const selectedRev = revisions[revisionParam ?? defaultRev ?? -1] ?? latest;
  const selected = selectedRev?.number ?? 1;
  const latestRevision = latest?.number ?? 1;

  // The header graphs every change that carries the same value as this one
  // for the selected tag key. The key the reviewer picked last is kept per
  // browser, so it follows them between changes as long as each carries it.
  const [preferredKey, setPreferredKey] = useState(() =>
    localStorage.getItem(TAG_KEY),
  );
  const chooseTagKey = useCallback((key: string) => {
    setPreferredKey(key);
    localStorage.setItem(TAG_KEY, key);
  }, []);
  const tags = published?.tags ?? {};
  const selectedTag = selectTag(tags, preferredKey);
  const tag =
    selectedTag === null ? undefined : `${selectedTag[0]}=${selectedTag[1]}`;
  // This change alone, by number, until its projection names its tags;
  // then every change that shares the selected one. Each projection + live
  // fold is written into the ["change", id] cache the queries above read.
  const subscription: Subscription = {
    query:
      published === undefined || tag === undefined
        ? { change: changeNumber }
        : { repo: published.repo_id, tag: [tag] },
  };
  const memberProjections = useChangeStream(subscription);
  const graph = useMemo(() => tagGraph(memberProjections), [memberProjections]);
  // The graph's row order, top to bottom: n steps up it, shift+n down.
  const rowIds = useMemo(
    () =>
      graph.nodes.flatMap((n) =>
        n.change_number === null ? [] : [n.change_number],
      ),
    [graph],
  );
  // This change stays in the drafts read while a new subscription refills
  // the picked set, so its overlay never blinks out.
  const draftsMap = useDrafts(
    rowIds.includes(changeNumber) ? rowIds : [changeNumber, ...rowIds],
  );
  // The same read useDrafts makes for this change, for its error: an
  // unknown change number surfaces here, because the websocket says nothing.
  const draftsQ = useQuery({
    queryKey: ["drafts", changeNumber],
    queryFn: () => getChangeDrafts(changeNumber),
  });
  const overlay = draftsMap.get(changeNumber);
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

  // Pin the frozen default into the URL on first load, so the address reflects
  // the viewed revision. It writes `defaultRev` (the first-seen revision), not
  // the live latest — a revision arriving in this effect's post-commit gap
  // can't be captured as the pin, and `selected` already falls back to
  // `defaultRev`, so nothing moves regardless of when the effect flushes.
  useEffect(() => {
    if (revisionParam === undefined && defaultRev !== undefined) {
      updateParams({ revision: String(defaultRev) });
    }
  }, [revisionParam, defaultRev, updateParams]);

  const activity = nodeActivity(memberProjections, draftsMap);
  const drafted = [...draftsMap.values()].filter(
    (d) => d.draft_decision !== null,
  ).length;

  const againstRaw = searchParams.get("against");
  const against = deriveDiffBase(againstRaw, selected);

  // The miss bubble's offset was measured against one diff, and the page
  // outlives every way of replacing it (it is reused across /changes/:id).
  // Adjust-during-render, like `shownChange` below: a bubble hanging over
  // code it never described must not paint even once.
  const diffKey = `${changeNumber}:${selected}:${String(against)}:${layout}:${mode}`;
  if (selectionMiss && selectionMiss.diff !== diffKey) setSelectionMiss(null);

  const diffQ = useQuery({
    queryKey: ["diff", changeNumber, selected, against ?? null, mode],
    queryFn: () => getDiff(changeNumber, selected, against, mode),
    enabled: published !== undefined,
    retry: false,
  });
  // Tree order, not git's: the rail renders the same list as a tree, and
  // the sections below must scroll in the order it reads (`treeOrder`).
  const files = useMemo(() => treeOrder(diffQ.data?.files ?? []), [diffQ.data]);

  // Collapsed-by-default file sections, keyed by file path so the
  // reviewer's open files survive revision/base switches; only another
  // change resets them (lib/collapse.ts). Keyed on the raw route param:
  // a malformed id parses to NaN, which never equals itself and would
  // re-fire this reset every render.
  const [expanded, setExpanded] =
    useState<ReadonlySet<string>>(defaultExpanded);
  const [shownChange, setShownChange] = useState(id);
  if (shownChange !== id) {
    // Adjust-during-render, not an effect: the reset is part of the same
    // render that switches changes, so stale expansion never paints.
    setShownChange(id);
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

  /** Collapsing the section that hosts the open inline CommentEditor
   * unmounts it and destroys its draft — the same discard path the guarded
   * setEditingTarget covers: confirm while dirty. `hidesEditor` says
   * whether the attempted collapse covers the editor's section; returns
   * false when the user keeps their text, and the caller must abort the
   * collapse (no state change). On an accepted discard the target is
   * cleared too — left in place, re-expanding the file would resurrect an
   * empty editor at the stale anchor. */
  const confirmEditorCollapse = useCallback((hidesEditor: boolean): boolean => {
    if (!hidesEditor) return true;
    if (!confirmDiscard(editorDirty.current)) return false;
    editorDirty.current = false;
    setEditingTarget(null);
    return true;
  }, []);

  // An open editor's anchor is its *visual* column, which a range switch
  // would silently re-map to a different (revision, side) at save time
  // (lib/comments draftAnchor). Confirm-and-clear it first, instead of
  // re-anchoring behind the user.
  const switchRange = useCallback(
    (patch: Record<string, string | null>) => {
      if (!confirmEditorCollapse(editingTarget !== null)) return;
      updateParams(patch);
    },
    [editingTarget, confirmEditorCollapse, updateParams],
  );

  // Diff range dropdowns. Left writes ?against ("base" | "1".."N-1").
  // Right writes ?revision; a still-valid numeric base is preserved (the
  // dropdowns are independent coordinates, as in Gerrit), an invalid one
  // resets to Base, an explicit "base" is kept.
  const onLeft = (v: string) => {
    switchRange({ against: v });
  };
  const onRight = useCallback(
    (n: number) => {
      const patch: Record<string, string | null> = { revision: String(n) };
      if (
        againstRaw !== null &&
        againstRaw !== "base" &&
        deriveDiffBase(againstRaw, n) === undefined
      )
        patch.against = null;
      switchRange(patch);
    },
    [againstRaw, switchRange],
  );

  const ctxValue: ReviewCtx = useMemo(
    () => ({
      changeNumber,
      selected,
      against,
      latestRevision,
      showRange: ({ against: from, selected: to }: DiffRange) => {
        switchRange({ against: String(from), revision: String(to) });
      },
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
          targetLine(t) === targetLine(cur);
        if (!sameCell && !confirmDiscard(editorDirty.current)) return false;
        setEditingTarget(t);
        return true;
      },
      setEditorDirty: (dirty: boolean) => {
        editorDirty.current = dirty;
      },
    }),
    [
      changeNumber,
      selected,
      against,
      latestRevision,
      switchRange,
      editingTarget,
    ],
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
  const fileCount = files.length;

  const createChangeComment = useMutation({
    mutationFn: (body: string) =>
      createDraft(changeNumber, { revision: selected, body }),
    onSuccess: () => {
      setChangeCommentOpen(false);
      void queryClient.invalidateQueries({
        queryKey: ["drafts", changeNumber],
      });
    },
  });

  // Keyboard nav: [ / ] previous/next file (revealed like a rail click:
  // expanded, then scrolled), n / shift+n next/previous change, r the latest
  // revision, c comments on the selected diff text, a opens the reply modal.
  // All inert while a modal is open — a showModal() dialog owns the keyboard
  // (Escape arrives as its cancel event), but window listeners still fire, so
  // the open dialog itself is the gate.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (document.querySelector("dialog[open]")) return;
      const key = shortcutKey(e);
      if (key === "[" || key === "]") {
        if (fileCount === 0) return;
        const cur = activeFile ?? (key === "]" ? -1 : fileCount);
        const next = Math.min(
          fileCount - 1,
          Math.max(0, cur + (key === "]" ? 1 : -1)),
        );
        revealFile(next);
      } else if (key === "n" || key === "shift+n") {
        const position = rowIds.indexOf(changeNumber);
        if (position < 0) return;
        const next = rowIds[position + (key === "n" ? -1 : 1)];
        if (next !== undefined) void navigate(`/changes/${next}`);
      } else if (key === "r") {
        // Guarded, because switchRange asks the reviewer to discard an open
        // comment editor — the latest revision is where they already are.
        if (selected !== latestRevision) onRight(latestRevision);
      } else if (key === "c") {
        // Draft a comment on the selected diff text (gerrit's c) — or on
        // the caret's line when the selection is collapsed.
        const sel = document.getSelection();
        if (!sel || sel.rangeCount === 0) return;
        const range = sel.getRangeAt(0);
        const result = selectionTarget(range);
        if (!result) return;
        // preventDefault, or the keystroke lands in the editor's textarea.
        e.preventDefault();
        if ("miss" in result) {
          const col = diffColumnRef.current?.getBoundingClientRect();
          if (col) {
            setSelectionMiss({
              ...result,
              ...missAnchor(range, col),
              diff: diffKey,
            });
          }
          return;
        }
        // The editor renders its own range highlight; the DOM selection
        // would just shout over it. Keep it on a declined discard.
        if (ctxValue.setEditingTarget(result)) sel.removeAllRanges();
      } else if (key === "a") {
        // preventDefault, or the keystroke's own text insertion lands in
        // the cover-message textarea the opening modal focuses.
        e.preventDefault();
        setReplyOpen(true);
      } else if (key === "0") {
        saveSettings({
          ...settings,
          mode: settings.mode === "outline" ? "full" : "outline",
        });
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
    rowIds,
    changeNumber,
    navigate,
    ctxValue,
    diffKey,
    settings,
    saveSettings,
    selected,
    latestRevision,
    onRight,
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

  // A miss answers one selection, so it stands until the reviewer starts
  // another. `selectstart`, not `selectionchange`: the latter also fires
  // for the very selection the miss is about, and it is queued as a task,
  // so it can arrive after the keystroke and blink the answer away as it
  // appears.
  useEffect(() => {
    const onSelectStart = () => {
      setSelectionMiss(null);
    };
    document.addEventListener("selectstart", onSelectStart);
    return () => {
      document.removeEventListener("selectstart", onSelectStart);
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
  // group alike.
  const threadsByFile = useMemo(() => {
    const map = new Map<string, UiThread[]>();
    for (const t of threads) {
      const path = anchorFile(t.anchor);
      if (path === null) continue;
      if (!threadInRange(t, selected, against)) continue;
      const file = files.find((f) => f.path === path || f.old_path === path);
      const key = file ? file.path : path;
      const list = map.get(key) ?? [];
      list.push(t);
      map.set(key, list);
    }
    return map;
  }, [threads, files, selected, against]);

  // The change's published projection arrives over the websocket (no fetch to
  // error on); a bad change number surfaces when its drafts read fails.
  if (draftsQ.isError) {
    return (
      <main className="page">
        <ErrorPanel error={draftsQ.error} />
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

  const repo = repoQ.data;
  const allFilesExpanded = allExpanded(expanded, files);

  const changeLevelThreads = threads.filter(
    (t) => anchorFile(t.anchor) === null,
  );
  const orphanFileThreads = [...threadsByFile.entries()].filter(
    ([path]) => !files.some((f) => f.path === path),
  );

  return (
    <ReviewContext.Provider value={ctxValue}>
      <main className="page-wide review-page">
        <div className="review-header">
          <div className="review-header-main">
            <div className="crumb-line">
              <Link to={`/repos/${change.repo_id}`}>
                {repo ? repoPath(repo.git_dir) : `repo ${change.repo_id}`}
              </Link>
              <span className="sep">/</span>
              <span className="dim">change {change.id}</span>
              <span className="sep">·</span>
              <span className="mono dim" title={change.change_id}>
                {change.change_id.slice(0, 12)}
              </span>
            </div>
            <div className="subject-line">
              <h1>{selectedRev.subject}</h1>
              <StatusChip status={selectedRev.status} />
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
          <TagNav
            tags={tags}
            selectedKey={selectedTag?.[0] ?? null}
            onSelectKey={chooseTagKey}
            graph={graph}
            activity={activity}
            currentId={changeNumber}
          />
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
              title="Keyboard: [ and ] switch files, n and shift+n switch changes, r shows the latest revision, c comments on the selected diff text, a opens the reply dialog"
            >
              <kbd>[</kbd>
              <kbd>]</kbd> files · <kbd>n</kbd>
              <kbd>shift+n</kbd> changes · <kbd>r</kbd> latest revision ·{" "}
              <kbd>c</kbd> comment · <kbd>a</kbd> reply · <kbd>0</kbd> outline
            </span>
            <ReviewSettingsMenu settings={settings} onApply={saveSettings} />
          </div>
        </div>

        <div className="review-layout">
          <aside className="review-sidebar">
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
          {/* The resolved diff base, present only once the query for the
              current range has settled — absent through the skeleton while a
              base switch refetches. A deterministic gate for the screenshot
              harness, which otherwise raced the refetch on a fixed timeout. */}
          <div
            className="diff-column"
            ref={diffColumnRef}
            data-diff-ready={
              diffQ.isSuccess ? String(against ?? "base") : undefined
            }
          >
            {selectionMiss ? (
              <div
                className="selection-miss"
                role="status"
                style={{ top: selectionMiss.top, left: selectionMiss.left }}
              >
                {MISS_TEXT[selectionMiss.miss]}
              </div>
            ) : null}
            {changeLevelThreads.length > 0 || changeCommentOpen ? (
              <section className="change-threads">
                <div className="thread-group-title">Change discussion</div>
                {changeLevelThreads.map((t) => (
                  <CommentThread key={threadKey(t)} thread={t} />
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
                <div className="thread-group-title">
                  Threads on files outside this diff
                </div>
                {orphanFileThreads.map(([path, fileThreads]) => (
                  <div key={path} className="leftover-file">
                    <div className="leftover-path mono">
                      {displayPath(path)}
                    </div>
                    {fileThreads.map((t) => {
                      const lineText = anchorLineText(t.anchor);
                      const at = anchorAt(t.anchor);
                      const line = at && placementLine(at);
                      return (
                        <div className="thread-group-item" key={threadKey(t)}>
                          {lineText ? (
                            <div className="line-excerpt">
                              <span className="excerpt-line">
                                r{t.revision}
                                {line !== null ? `:${line}` : ""}
                              </span>
                              <span
                                dangerouslySetInnerHTML={{
                                  __html: highlight(lineText, null),
                                }}
                              />
                            </div>
                          ) : null}
                          <CommentThread thread={t} />
                        </div>
                      );
                    })}
                  </div>
                ))}
              </section>
            ) : null}
          </div>
        </div>

        <ReviewBar
          change={change}
          tag={tag}
          drafted={drafted}
          selectedRevision={selected}
          unresolved={pendingUnresolvedCount(threads)}
          replyOpen={replyOpen}
          onReplyOpenChange={setReplyOpen}
        />
      </main>
    </ReviewContext.Provider>
  );
}
