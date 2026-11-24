import { skipToken, useQuery } from "@tanstack/react-query";
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
  NavigationType,
  useLocation,
  useNavigate,
  useNavigationType,
  useParams,
} from "react-router-dom";
import { getRepo } from "../api/client";
import { tagGraph } from "../api/fold";
import type {
  Draft,
  Revision,
  Side,
  Subscription,
  Tags,
  ThreadProjection,
  ChangeVersion,
} from "../api/types";
import { StatusChip } from "../components/badges";

import TagNav from "../components/TagNav";
import NewThreadButton from "../components/NewThreadButton";
import NewThreadEditor from "../components/NewThreadEditor";
import ChangeLog from "../components/ChangeLog";
import CommentThread from "../components/CommentThread";
import DiffColumn from "../components/diff/DiffColumn";
import DiffFileView from "../components/diff/DiffFileView";
import FileRail from "../components/diff/FileRail";
import NewRevisionToast from "../components/NewRevisionToast";
import ReviewBar from "../components/ReviewBar";
import ReviewColumns from "../components/ReviewColumns";
import ReviewSettingsMenu from "../components/ReviewSettings";
import Select, { type SelectOption } from "../components/Select";
import Shortcuts from "../components/Shortcuts";
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
  anchorRevision,
  commentPlacement,
  firstLine,
  commentCountLabel,
  nodeActivity,
  placementLine,
  pendingUnresolvedCount,
  shownAt,
  threadCountByRevision,
  threadKey,
  threadsByFile,
  type DiffRange,
} from "../lib/comments";
import {
  baseVersion,
  isBase,
  parseVersion,
  revisionOf,
  revisionVersion,
} from "../lib/version";
import {
  fileLink,
  linkedThread,
  linksThread,
  linkParams,
  parseLink,
  sameLink,
  type DiffLink,
} from "../lib/diffLink";
import { displayPath, fileAt, fileDomId } from "../lib/diffview";
import { repoPath } from "../lib/repo";
import { activeIndexAt, scrollBehavior, whenPresent } from "../lib/scrollspy";
import { shortcutKey } from "../lib/shortcutKey";
import { shortSha } from "../lib/sha";
import { timeAgo } from "../lib/time";
import { useChangeLog } from "../lib/useChangeLog";
import { useChangeStream, usePublishedChange } from "../lib/useChangeStream";
import { useDocumentTitle } from "../lib/useDocumentTitle";
import { useDiff } from "../lib/useDiff";
import { useDraftEditor } from "../lib/useDraftEditor";
import { draftsQuery, useDrafts } from "../lib/useDrafts";
import { useReviewSettings } from "../lib/reviewSettings";
import { useThreads } from "../lib/useThreads";
import { patchParams, useUrlParams } from "../lib/useUrlParams";
import { ErrorPanel } from "./NotFound";
import { REVEAL, ReviewContext, type ReviewCtx } from "../lib/reviewContext";

const TAG_KEY = "nit.review-tag";

const NO_THREADS: ThreadProjection[] = [];
const NO_DRAFTS: Draft[] = [];

/** The location state of a navigation that must not move the view. */
const QUIET = "quiet";

/** The element that shows `line` in the diff column `side` of the file
 * section of the thread `el`: a row of the diff, or a block of a rendered
 * file. Null when the thread does not render under a line, or the section
 * shows no such line. A thread whose line the shown range does not have
 * renders in a group of its own, and its anchor is on a line of another
 * version. */
function anchorTop(el: Element, side: Side, line: number): Element | null {
  const section = el.closest(".meta-row")?.closest(".file-section");
  if (!section) return null;
  const blocks = section.querySelectorAll<HTMLElement>(
    `.doc-block[data-side="${side}"]`,
  );
  return (
    section.querySelector(`.code[data-${side}="${line}"]`) ??
    [...blocks].find(
      (b) => Number(b.dataset.start) <= line && line <= Number(b.dataset.end),
    ) ??
    null
  );
}

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

/** Resolve the ?against param into a diff base for the selected revision.
 * Grammar: "base" or absent → full diff vs parent; "rM" → interdiff
 * rM → rSelected when M < selected (junk falls back to the full diff). */
function deriveDiffBase(raw: string | null, selected: number): ChangeVersion {
  const version = parseVersion(raw);
  return version !== null && !isBase(version) && revisionOf(version) < selected
    ? version
    : baseVersion(selected);
}

/** The ?against param of `version`: none for a base. */
const againstParam = (version: ChangeVersion) =>
  isBase(version) ? null : version;

/** Gerrit-style diff range: [Base|rM] → [rN]. Left picks the diff base,
 * right the revision under review. Each rN row carries its own
 * comment-thread count (`counts`) so the reviewer sees where discussion
 * sits before switching. */
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
  against: ChangeVersion;
  counts: Map<number, number>;
  onLeft: (v: string) => void;
  onRight: (n: number) => void;
}) {
  const option = (r: Revision): SelectOption => {
    const n = counts.get(r.number) ?? 0;
    return {
      value: String(r.number),
      label: `r${r.number}`,
      detail: n > 0 ? commentCountLabel(n) : undefined,
    };
  };
  return (
    <>
      <Select
        // Wide enough for a revision and its comment count, so choosing one
        // never reflows the toolbar around it.
        className="min-w-[18ch]"
        label="Diff base"
        title="Base = parent commit; rM = interdiff against revision M"
        placeholder="no base"
        value={againstParam(against) ?? "base"}
        onChange={onLeft}
        options={[
          { value: "base", label: "Base" },
          ...revisions.map((r) => ({
            ...option(r),
            value: revisionVersion(r.number),
            disabled: r.number >= selected,
          })),
        ]}
      />
      <span className="dim mono">→</span>
      <Select
        className="min-w-[18ch]"
        label="Revision"
        title="Revision under review"
        placeholder="no revisions"
        value={String(selected)}
        onChange={(v) => {
          onRight(Number(v));
        }}
        options={revisions.map(option)}
      />
    </>
  );
}

export default function ReviewPage() {
  const { id } = useParams();
  const changeNumber = Number(id);
  const [searchParams, updateParams] = useUrlParams();
  const link = useMemo(() => parseLink(searchParams), [searchParams]);
  const linkedId = linkedThread(link);
  const location = useLocation();
  const navigationType = useNavigationType();

  const revisionParam = searchParams.get("revision")
    ? Number(searchParams.get("revision"))
    : undefined;

  // `useChangeStream` (below) folds the published projection (revisions,
  // threads, tags) from the change-event websocket into the ["change", id]
  // cache that this query returns. The diff, the repo, and the reviewer's
  // drafts and draft decision are not in the log, so they come from REST.
  // `change` composes the published projection with the drafts overlay
  // (`useDrafts`).

  const published = usePublishedChange(changeNumber);
  const log = useChangeLog(changeNumber, published);

  const repoQ = useQuery({
    queryKey: ["repo", published?.repo_id],
    queryFn: published ? () => getRepo(published.repo_id) : skipToken,
  });

  const [settings, saveSettings] = useReviewSettings();
  const { layout, whitespace } = settings;
  const editor = useDraftEditor();
  const { editingTarget, setEditingTarget } = editor;
  const [activeFile, setActiveFile] = useState<number | null>(null);
  const [changeCommentOpen, setChangeCommentOpen] = useState(false);
  const [replyOpen, setReplyOpen] = useState(false);
  const [shortcutsOpen, setShortcutsOpen] = useState(false);

  const revisions = published?.revisions ?? [];
  const latest = revisions[revisions.length - 1];

  // The revision the page defaults to (until the URL pins one) is frozen at
  // first sight: a revision folding in over the websocket must not move it.
  // A revision arriving before the pin effect (below) commits would
  // otherwise slide the default forward and jump the view to the
  // just-pushed revision. Keyed by change, because the component is reused
  // across /changes/:id without remounting.
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
  useDocumentTitle(selectedRev?.subject ?? null);

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
  // The skeleton unmounts TagNav while the next change loads, so the "Show
  // all" choice is state of this page, and it stays the same from one
  // change to the next.
  const [tagNavExpanded, setTagNavExpanded] = useState(false);
  const tags = published?.tags ?? {};
  const selectedTag = selectTag(tags, preferredKey);
  const tag =
    selectedTag === null ? undefined : `${selectedTag[0]}=${selectedTag[1]}`;
  const subscription: Subscription = {
    query:
      published === undefined || tag === undefined
        ? { change: [changeNumber] }
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
  const { overlays: draftsMap, loading: overlaysLoading } = useDrafts(
    rowIds.includes(changeNumber) ? rowIds : [changeNumber, ...rowIds],
  );
  // The same read useDrafts makes for this change, for its error: an
  // unknown change number surfaces here, because the websocket says nothing.
  const draftsQ = useQuery(draftsQuery(changeNumber));
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
  // the viewed revision.
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

  const {
    diff: diffQ,
    files,
    attachKey,
  } = useDiff(published, changeNumber, selected, against, whitespace);

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

  // The link of the last navigation that the page handled, and the
  // location key of that navigation. A link moves the view when it first
  // appears, and again when a click on a link pushes it anew. Any other
  // rewrite of the URL, for example a revision switch, leaves the view
  // where the reviewer put it. A quiet navigation (`selectLink`) also
  // leaves the view in place.
  const [revealed, setRevealed] = useState<{
    link: DiffLink | null;
    key: string;
  }>({ link: null, key: "" });
  // The link that the page still has to scroll to. A file section scrolls
  // to a file link and then clears it. The page scrolls to a thread link,
  // and `scrolledThread` records that scroll. Each reveal stores a fresh
  // object, so a repeat of one link scrolls again.
  const [pending, setPending] = useState<DiffLink | null>(null);
  const onRevealed = useCallback(() => {
    setPending(null);
  }, []);
  const scrolledThread = useRef<DiffLink | null>(null);
  const pendingFile = fileLink(pending);
  const pendingThread = linkedThread(pending);

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
      document.getElementById(fileDomId(index))?.scrollIntoView({
        behavior: scrollBehavior(),
        block: "start",
      });
    },
    [files],
  );

  const confirmEditorCollapse = useCallback(
    (hidesEditor: boolean) => !hidesEditor || setEditingTarget(null),
    [setEditingTarget],
  );

  // An open editor's anchor is its *visual* column, which a range switch
  // would silently re-map to a different version at save time
  // (lib/comments columnVersion). Confirm-and-clear it first.
  const switchRange = useCallback(
    (patch: Record<string, string | null>) => {
      if (!confirmEditorCollapse(editingTarget !== null)) return;
      updateParams(patch);
    },
    [editingTarget, confirmEditorCollapse, updateParams],
  );

  // Diff range dropdowns. Left writes ?against ("base", or "rM" below the
  // selected revision).
  // Right writes ?revision; a still-valid revision base is preserved (the
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
        againstParam(deriveDiffBase(againstRaw, n)) === null
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
      whitespace,
      attachKey,
      showRange: ({ against: from, selected: to }: DiffRange) => {
        switchRange({
          against: againstParam(from),
          revision: String(to),
        });
      },
      link,
      linkHref: (to: DiffLink) =>
        `?${patchParams(searchParams, linkParams(to)).toString()}`,
      selectLink: (to: DiffLink) => {
        updateParams(linkParams(to), QUIET);
      },
      editingTarget: editor.editingTarget,
      setEditingTarget: editor.setEditingTarget,
      setEditorDirty: editor.setEditorDirty,
    }),
    [
      changeNumber,
      selected,
      against,
      latestRevision,
      whitespace,
      attachKey,
      switchRange,
      link,
      searchParams,
      updateParams,
      editor.editingTarget,
      editor.setEditingTarget,
      editor.setEditorDirty,
    ],
  );

  const {
    threads,
    shown: shownThreads,
    ported: portedQ,
  } = useThreads({
    changeNumber,
    published: change?.threads ?? NO_THREADS,
    drafts: change?.drafts ?? NO_DRAFTS,
    selected,
    against,
    includeResolved: settings.ported === "all",
    linked: linkedId,
    enabled: published !== undefined,
  });

  const revisionCommentCounts = useMemo(
    () => threadCountByRevision(threads),
    [threads],
  );

  const navigate = useNavigate();
  const fileCount = files.length;

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
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
      } else if (key === "a") {
        // preventDefault, or the keystroke's own text insertion lands in
        // the cover-message textarea the opening modal focuses.
        e.preventDefault();
        setReplyOpen(true);
      } else if (key?.endsWith("?")) {
        // `?` needs shift on some keyboard layouts and not on others.
        setShortcutsOpen(true);
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
    selected,
    latestRevision,
    onRight,
  ]);

  // Scroll spy: keep activeFile — the rail highlight and the [ / ] cursor —
  // on the file section currently under the sticky chrome. The threshold is
  // the sections' scroll-margin-top, read from computed style so the sticky
  // offsets live only in styles/review.css; it is the exact line scrollIntoView
  // targets, so a rail click / keystroke and the spy agree on the
  // destination file (+1 absorbs fractional scrolls).
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

  const threadsOfFile = useMemo(
    () => threadsByFile(shownThreads, files, selected, against, linkedId),
    [shownThreads, files, selected, against, linkedId],
  );

  // Adjust-during-render, like `shownChange` above: the file a link names
  // expands in the same render that reads the link, so it never paints
  // collapsed. A file link waits for the diff to list its file. A thread
  // link also waits for the ported places, because they decide where the
  // thread renders.
  const moves =
    link !== null &&
    location.state !== QUIET &&
    (navigationType === NavigationType.Push ||
      location.state === REVEAL ||
      !sameLink(link, revealed.link));
  if (location.key !== revealed.key) {
    if (!moves) {
      setRevealed({ link, key: location.key });
    } else {
      const linked = fileLink(link);
      const thread =
        linked === null && diffQ.isSuccess && !portedQ.isFetching
          ? shownThreads.find((t) => linksThread(link, t.id))
          : undefined;
      const path =
        linked?.file ?? (thread ? anchorFile(shownAt(thread)) : null);
      const file = path === null ? undefined : fileAt(files, path);
      if (linked !== null ? file !== undefined : thread !== undefined) {
        setRevealed({ link, key: location.key });
        if (file) setExpanded((cur) => expand(cur, file.path));
        setPending({ ...link });
      }
    }
  }

  // A layout effect, so the scroll lands before the expanded file paints
  // at its old offset.
  useLayoutEffect(() => {
    if (pendingThread === null || scrolledThread.current === pending) return;
    const thread = shownThreads.find((t) => t.id === pendingThread);
    const anchor = thread && shownAt(thread);
    const scroll = (el: Element) => {
      // The thread renders under the last line of its anchor. The scroll
      // goes to the first line, so the page shows the code that the
      // thread is about. A thread with no line on the page is the target
      // itself.
      const at = anchor && anchorAt(anchor);
      const place = anchor && commentPlacement(anchor, selected, against);
      const top = at && place && anchorTop(el, place.side, firstLine(at));
      (top ?? el).scrollIntoView({
        behavior: scrollBehavior(),
        block: "start",
      });
      scrolledThread.current = pending;
    };
    const find = () =>
      document.querySelector(`[data-thread="${pendingThread}"]`);
    return whenPresent(document.body, find, scroll);
  }, [pending, pendingThread, expanded, shownThreads, selected, against]);

  // Busy until the stream has delivered the change and a member of its tag
  // graph, and every read has its first answer. A read that this render enables
  // already reports isLoading, so the page never looks idle between two
  // reads that depend on each other.
  const busy =
    published === undefined ||
    memberProjections.length === 0 ||
    overlaysLoading ||
    [repoQ, draftsQ, diffQ, portedQ].some((q) => q.isLoading);

  if (draftsQ.isError) {
    return (
      <main className="page" aria-busy={false}>
        <ErrorPanel error={draftsQ.error} />
      </main>
    );
  }
  if (!change || !latest || !selectedRev) {
    return (
      <main className="page" aria-busy={busy}>
        <div className="skeleton" style={{ width: 320, height: 18 }} />
        <div className="skeleton" style={{ width: 200, marginTop: 10 }} />
        <div className="skeleton" style={{ marginTop: 24, height: 260 }} />
      </main>
    );
  }

  const repo = repoQ.data;
  const allFilesExpanded = allExpanded(expanded, files);

  const changeLevelThreads = shownThreads.filter(
    (t) => anchorFile(shownAt(t)) === null,
  );
  const orphanFileThreads = [...threadsOfFile.entries()].filter(
    ([path]) => !files.some((f) => f.path === path),
  );

  return (
    <ReviewContext.Provider value={ctxValue}>
      <main className="page-wide review-page" aria-busy={busy}>
        {/* The key drops the toast of the previous change. */}
        <NewRevisionToast
          key={changeNumber}
          opened={defaultRev ?? latest.number}
          latest={latest.number}
          selected={selected}
          onOpen={onRight}
        />
        <div className="review-header">
          <div className="review-header-main">
            <div className="crumb-line">
              <Link to={`/repos/${change.repo_id}`}>
                {repo ? repoPath(repo.git_dir) : `repo ${change.repo_id}`}
              </Link>
              <span className="sep">/</span>
              <span>change {change.id}</span>
            </div>
            <div className="subject-line">
              <h1>{selectedRev.subject}</h1>
              <StatusChip status={selectedRev.status} />
            </div>
            <div className="meta-line">
              <span>
                commit{" "}
                <span className="mono">{shortSha(selectedRev.commit_sha)}</span>
              </span>
              <span>
                parent{" "}
                <span className="mono">{shortSha(selectedRev.parent_sha)}</span>
              </span>
              <span title={change.change_id}>
                Change-Id{" "}
                <span className="mono">{change.change_id.slice(0, 12)}</span>
              </span>
              <span>{timeAgo(selectedRev.created_at)}</span>
            </div>
          </div>
          <TagNav
            tags={tags}
            selectedKey={selectedTag?.[0] ?? null}
            onSelectKey={chooseTagKey}
            graph={graph}
            activity={activity}
            currentId={changeNumber}
            expanded={tagNavExpanded}
            onExpandedChange={setTagNavExpanded}
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
            <NewThreadButton
              anchor="change"
              onClick={() => {
                setChangeCommentOpen(true);
              }}
            />
            <Shortcuts open={shortcutsOpen} onOpenChange={setShortcutsOpen} />
            <ReviewSettingsMenu settings={settings} onChange={saveSettings} />
          </div>
        </div>

        <ReviewColumns
          rail={
            <FileRail
              files={files}
              threadsByFile={threadsOfFile}
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
          }
        >
          <DiffColumn
            layout={layout}
            ready={diffQ.isSuccess ? against : undefined}
          >
            {changeLevelThreads.length > 0 || changeCommentOpen ? (
              <section className="change-threads">
                <div className="thread-group-title">Change discussion</div>
                {changeLevelThreads.map((t) => (
                  <CommentThread key={threadKey(t)} thread={t} />
                ))}
                {changeCommentOpen ? (
                  <NewThreadEditor
                    onClose={() => {
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
                  threads={threadsOfFile.get(file.path) ?? []}
                  domId={fileDomId(i)}
                  collapsed={!expanded.has(file.path)}
                  reveal={pendingFile?.file === file.path ? pendingFile : null}
                  onRevealed={onRevealed}
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
              <section>
                <div className="thread-group-title">
                  Threads on files outside this diff
                </div>
                {orphanFileThreads.map(([path, fileThreads]) => (
                  <div key={path} className="leftover-file">
                    <div className="leftover-path mono">
                      {displayPath(path)}
                    </div>
                    {fileThreads.map((t) => {
                      const at = anchorAt(t.anchor);
                      return (
                        <div className="thread-group-item" key={threadKey(t)}>
                          {at ? (
                            <div className="line-excerpt">
                              <span className="excerpt-line">
                                r{anchorRevision(t.anchor)}:{placementLine(at)}
                              </span>
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
          </DiffColumn>
        </ReviewColumns>

        <ChangeLog feed={log} threads={change.threads} />

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
