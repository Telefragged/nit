import { useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import { Link, useParams } from "react-router-dom";
import { getChanges, getHistory, getRepo, getTags } from "../api/client";
import { repoGraph } from "../api/fold";
import type { ChangeStatus } from "../api/types";
import ChangeGraph, { type NodeActivity } from "../components/ChangeGraph";
import { repoPath } from "../lib/repo";
import { useDrafts } from "../lib/useDrafts";
import { useUrlParams } from "../lib/useUrlParams";
import { ErrorPanel } from "./NotFound";

/** Every status but `merged`: the open region derives from active tips, but a
 * walk may pass through an abandoned member (abandonment is
 * membership-inert), so the fetch must resolve those too. */
const GRAPH_STATUSES: ChangeStatus[] = [
  "pending",
  "approved",
  "changes_requested",
  "commented",
  "abandoned",
];

/** `list`, plus `choice` when the list lacks it. */
const pinned = (list: string[], choice: string | null): string[] =>
  choice === null || list.includes(choice) ? list : [...list, choice];

/** A repo's review dashboard: one change graph centered on the canonical
 * ref — open changes ascending above the HEAD anchor, merged
 * history descending below it — assembled in the browser (api/fold) from the
 * repo's unmerged change folds and its canonical history. `?group=<key>`
 * groups the open changes by that tag key. `?value=<value>` then keeps
 * only the changes that carry that value. */
export default function Dashboard() {
  const { repoId } = useParams();
  const id = Number(repoId);
  const [params, updateParams] = useUrlParams();
  const groupBy = params.get("group");
  const value = groupBy === null ? null : params.get("value");
  const tag =
    groupBy !== null && value !== null ? `${groupBy}=${value}` : undefined;

  // The repo's path (its name) is fixed for the page's lifetime, so fetch it
  // once by id — only the changes/history reads refetch as things land.
  const repoQuery = useQuery({
    queryKey: ["repo", id],
    queryFn: () => getRepo(id),
  });
  const changesQuery = useQuery({
    queryKey: ["repo-changes", id, tag ?? null],
    queryFn: () => getChanges(id, GRAPH_STATUSES, tag),
  });
  const historyQuery = useQuery({
    queryKey: ["history", id],
    queryFn: () => getHistory(id),
  });
  const graph = useMemo(
    () =>
      changesQuery.data && historyQuery.data
        ? repoGraph(changesQuery.data.changes, historyQuery.data, groupBy)
        : undefined,
    [changesQuery.data, historyQuery.data, groupBy],
  );
  // The selectors offer what an unmerged change carries now, plus the
  // URL's own choice. The selectors then still show the choice of a stale
  // link.
  const tagsQuery = useQuery({
    queryKey: ["repo-tags", id],
    queryFn: () => getTags(id, GRAPH_STATUSES),
  });
  const tags = tagsQuery.data?.tags ?? {};
  const groupKeys = pinned(Object.keys(tags), groupBy);
  const values = groupBy === null ? [] : pinned(tags[groupBy] ?? [], value);

  // Each open node's activity badges read straight off the fold the bulk
  // read already delivered. Only the reviewer's drafts and draft decision
  // are outside the log, so that overlay is the one per-change read
  // (GET /changes/{id}/drafts).
  const activityIds = useMemo(
    () =>
      (graph?.nodes ?? []).flatMap((n) =>
        n.section === "open" && n.change_number !== null
          ? [n.change_number]
          : [],
      ),
    [graph],
  );
  const overlays = useDrafts(activityIds);
  const projs = changesQuery.data?.changes;
  const activity = useMemo(() => {
    const projById = new Map((projs ?? []).map((p) => [p.id, p]));
    return new Map<number, NodeActivity>(
      activityIds.map((id) => [
        id,
        {
          threads: projById.get(id)?.threads ?? [],
          drafts: overlays.get(id)?.drafts ?? [],
          decision: overlays.get(id)?.draft_decision?.decision ?? null,
        },
      ]),
    );
  }, [activityIds, projs, overlays]);

  const repo = repoQuery.data;
  const error = changesQuery.error ?? historyQuery.error;

  return (
    <main className="page">
      <h1 className="mono">{repo ? repoPath(repo.git_dir) : "Repository"}</h1>
      <p className="subtitle">
        <Link to="/" className="mono">
          ← Repositories
        </Link>{" "}
        · change graph
        {repo ? (
          <>
            {" "}
            over <span className="mono">{repo.canonical_ref}</span>
          </>
        ) : null}
        .
      </p>
      <div className="graph-toolbar">
        <label>
          Group by
          <select
            className="revision-select"
            value={groupBy ?? ""}
            onChange={(e) => {
              // A value belongs to its key.
              updateParams({ group: e.target.value || null, value: null });
            }}
          >
            <option value="">none</option>
            {groupKeys.map((key) => (
              <option key={key} value={key}>
                {key}
              </option>
            ))}
          </select>
        </label>
        <label>
          Only
          <select
            className="revision-select"
            value={value ?? ""}
            disabled={groupBy === null}
            onChange={(e) => {
              updateParams({ value: e.target.value || null });
            }}
          >
            <option value="">
              {groupBy === null ? "" : `every ${groupBy}`}
            </option>
            {values.map((v) => (
              <option key={v} value={v}>
                {v}
              </option>
            ))}
          </select>
        </label>
      </div>
      {error ? (
        <ErrorPanel error={error} />
      ) : graph === undefined ? (
        <div className="skeleton" style={{ height: 320 }} />
      ) : graph.nodes.length === 0 ? (
        <div className="empty-state">
          Nothing here yet. Run <code>nit push</code> from this repo to register
          a change for review.
        </div>
      ) : (
        <ChangeGraph graph={graph} activity={activity} />
      )}
    </main>
  );
}
