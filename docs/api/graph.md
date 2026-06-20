## Graph

The repo's **change graph** is one spine-centered DAG over the canonical
branch — the source for the web dashboard, which replaces the per-chain
tables. Where `/api/chains` returns independent tip-rooted _paths_ that
duplicate a change shared by two chains, the graph is a single
commit-sha-keyed node set: a shared change appears **once**, and fan-out and
merge commits are first-class. Like a chain, nothing about it is stored — it
is assembled at read time from the same in-memory folds + sha index, plus a
git walk of the canonical branch for the merged history.

- `GET /api/repos/{id}/graph` → RepoGraph. 404 if the repo is unknown. The
  history region is a fixed window of merged commits below the canonical HEAD
  (5); there is no client knob — paging deeper is a future paginated endpoint,
  not a refetch of the whole graph.

The graph has three regions around the **canonical HEAD** anchor — resolved
live from `base_branch`, never assumed equal to any one chain's recorded
`base_sha` (each push computed its own merge-base):

- **open** — every active change ascending above HEAD, derived exactly like
  `/api/chains` (each active tip walked back to its fork) then unioned and
  **deduplicated by commit-sha**. Only a reachable revision is a node: an
  amended tip's superseded revision is unreachable and never appears. The rare
  B-in-two-chains case (one change live at two revisions under two tips) is
  two nodes — they are different commits with different parents, so collapsing
  them would break a descendant's lineage.
  An open change may fork **behind** HEAD — its base predates the current HEAD
  (the canonical branch advanced without a rebase). It keeps its real base
  `parents`; the client draws that as a distinct edge (to the base node when it
  is within the window, else down into the truncation marker below).
- **head** — the canonical HEAD commit, the anchor (one node).
- **history** — up to a fixed window (5) of merged commits descending below HEAD, a
  git walk of the canonical branch. A commit mapping to a known change (by its
  `Change-Id` trailer) is enriched with that `change_id`/`change_key`; a merge
  or pre-nit commit is a bare node (subject from the commit message, no
  change). `history_truncated` is true when the branch has more merged commits
  below the window — the client shows an "earlier history hidden" marker that
  the spine descends into and behind-forks older than the window dangle to.

Nodes are returned in **row order** (top → bottom): a topological order in
which every node precedes its parents (children ascend, parents descend), so
the array index _is_ the row. Each node lists its `parents` by commit-sha; the
client inverts these for fan-out, packs lanes (the canonical branch is the
pinned center column), and renders. An edge is drawn to whichever parents are
in the node set; `parents.len() > 1` is a merge.

```json
RepoGraph = {
  "repo_id": 1,
  "base_branch": "main",
  "anchor": "9f12c0a…",        // the head node's commit_sha
  "history_truncated": false,  // more merged commits exist below the window
  "nodes": [GraphNode]         // row order: open (top) → head → history (bottom)
}
GraphNode = {
  "commit_sha": "a1f7c0d…",    // 40-hex; the node's stable id; client truncates
  "section": "open",           // open | head | history
  "subject": "feat(api): rename the --base flag",
  "status": "pending",         // ChangeStatus at the node's revision; the client
                               //   styles by section (head/history render merged)
  "parents": ["7b0c784…"],     // parent commit-shas; edges to those present; len>1 is a merge
  "change_id": 10,             // null for a bare git commit (merge / pre-nit)
  "change_key": "I3f2…",       // null with change_id
  "revision": 2,               // the pinned patchset (open nodes); null off the open region
  "counts": {"threads": 3, "drafts": 1, "unresolved": 2}, // activity; zeros off the open region
  "draft_decision": "approve"  // the change's staged decision (Decision), or null
}
```

`status`, `counts`, and `draft_decision` are read at the node's pinned
revision, exactly as a `PathEntry`. `change_id`/`change_key`/`revision` are
null on a bare git commit, and `revision` is null on the head node.
