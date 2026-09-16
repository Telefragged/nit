import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ChangeGraph, ChangeStatus, GraphNode } from "../api/types";
import type { NodeActivity } from "../lib/comments";
import TagNav from "./TagNav";

afterEach(cleanup);

/** A queried node that must exist for the test to make sense. */
function must<T>(value: T | null | undefined, what: string): T {
  if (value == null) throw new Error(`expected ${what}`);
  return value;
}

function node(
  changeNumber: number,
  subject: string,
  status: ChangeStatus,
  parent: string,
): GraphNode {
  return {
    commit_sha: `sha${changeNumber}`,
    section: "open",
    subject,
    status,
    parents: [parent],
    change_number: changeNumber,
    change_id: `I${changeNumber}`,
    revision: 0,
    fork_sha: "m",
    group: null,
  };
}

/** `count` unresolved threads at revision 0. */
function activity(count: number): NodeActivity {
  return {
    threads: Array.from({ length: count }, (_, i) => ({
      id: i,
      change_number: 0,
      revision: 0,
      anchor: "change" as const,
      resolved: false,
      comments: [],
      created_at: "",
      updated_at: "",
    })),
    drafts: [],
    unresolved: count,
    decision: null,
  };
}

/** A chain of `n` changes numbered from 10, tip first as the graph lists
 * them: change 10 + n - 1 at the top, change 10 at the bottom on "m". */
function chain(n: number): ChangeGraph {
  const nodes: GraphNode[] = [];
  for (let i = n - 1; i >= 0; i--) {
    const id = 10 + i;
    nodes.push(
      node(id, `change ${id}`, "pending", i === 0 ? "m" : `sha${id - 1}`),
    );
  }
  return { history_truncated: false, nodes };
}

const three: ChangeGraph = {
  history_truncated: false,
  nodes: [
    node(12, "third change", "pending", "sha11"),
    node(11, "second change", "changes_requested", "sha10"),
    node(10, "first change", "approved", "m"),
  ],
};

const renderNav = (
  graph: ChangeGraph,
  currentId: number,
  onSelectKey = vi.fn(),
) =>
  render(
    <MemoryRouter>
      <TagNav
        tags={{ branch: "feat", "session-id": "s1" }}
        selectedKey="session-id"
        onSelectKey={onSelectKey}
        graph={graph}
        activity={new Map([[11, activity(2)]])}
        currentId={currentId}
      />
    </MemoryRouter>,
  );

const rows = () => [...document.querySelectorAll(".tag-nav-row")];
const subjects = () =>
  rows().map((r) => r.querySelector(".subj")?.textContent ?? "");

describe("TagNav", () => {
  it("shows the chosen key with its value, and offers the others", () => {
    const onSelectKey = vi.fn();
    renderNav(three, 11, onSelectKey);
    const head = screen.getByLabelText<HTMLButtonElement>("Tag");
    expect(head.disabled).toBe(false);
    expect(head.querySelector(".select-label")?.textContent).toBe("session-id");
    expect(head.querySelector(".select-detail")?.textContent).toBe("s1");

    fireEvent.click(head);
    const options = screen.getAllByRole("option");
    expect(
      options.map((o) => [
        o.querySelector(".select-label")?.textContent,
        o.querySelector(".select-detail")?.textContent,
        o.getAttribute("aria-selected"),
      ]),
    ).toEqual([
      ["branch", "feat", "false"],
      ["session-id", "s1", "true"],
    ]);

    fireEvent.click(must(options[0], "the branch option"));
    expect(onSelectKey).toHaveBeenCalledWith("branch");
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("closes the list on a second press of the head", () => {
    renderNav(three, 11);
    const head = screen.getByLabelText<HTMLButtonElement>("Tag");
    fireEvent.click(head);
    expect(screen.getByRole("listbox")).not.toBeNull();
    fireEvent.click(head);
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("disables the selector for a change with no tags", () => {
    render(
      <MemoryRouter>
        <TagNav
          tags={{}}
          selectedKey={null}
          onSelectKey={vi.fn()}
          graph={{ history_truncated: false, nodes: [] }}
          activity={new Map()}
          currentId={11}
        />
      </MemoryRouter>,
    );
    expect(screen.getByLabelText<HTMLButtonElement>("Tag").disabled).toBe(true);
    expect(rows()).toHaveLength(0);
  });

  it("draws every row in graph order, links the others, and marks the current one", () => {
    renderNav(three, 11);
    expect(subjects()).toEqual([
      "third change",
      "second change",
      "first change",
    ]);
    expect(document.querySelector(".graph-rail")).not.toBeNull();

    const links = screen.getAllByRole("link");
    expect(links.map((a) => a.getAttribute("href"))).toEqual([
      "/changes/12",
      "/changes/10",
    ]);

    // A div, not a link, so the current page never self-links; aria-current
    // flags it for assistive tech.
    const current = must(
      document.querySelector<HTMLElement>(".tag-nav-row.current"),
      ".tag-nav-row.current",
    );
    expect(current.tagName).toBe("DIV");
    expect(current.getAttribute("aria-current")).toBe("page");
    expect(current.querySelector(".subj")?.textContent).toBe("second change");
    expect(current.querySelector(".unresolved-count")?.textContent).toBe(
      "2 open",
    );
    expect(document.querySelectorAll(".unresolved-count")).toHaveLength(1);
    // Nothing to expand: the whole graph fits the window.
    expect(document.querySelector(".tag-nav-all")).toBeNull();
  });

  it("windows seven rows around the current change", () => {
    // Twelve changes, 21 at the top. Change 16 sits at row 5, so the
    // window holds three rows either side of it.
    renderNav(chain(12), 16);
    expect(subjects()).toEqual(
      [19, 18, 17, 16, 15, 14, 13].map((id) => `change ${id}`),
    );
  });

  it("moves the window to the end when the current change is near it", () => {
    renderNav(chain(12), 20);
    expect(subjects()).toEqual(
      [21, 20, 19, 18, 17, 16, 15].map((id) => `change ${id}`),
    );
    cleanup();
    renderNav(chain(12), 11);
    expect(subjects()).toEqual(
      [16, 15, 14, 13, 12, 11, 10].map((id) => `change ${id}`),
    );
  });

  it("shows the whole graph on request, and the window again", () => {
    renderNav(chain(12), 16);
    fireEvent.click(screen.getByRole("button", { name: "show all" }));
    expect(rows()).toHaveLength(12);
    fireEvent.click(screen.getByRole("button", { name: "show less" }));
    expect(rows()).toHaveLength(7);
  });
});
