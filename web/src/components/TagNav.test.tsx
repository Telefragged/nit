import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ChangeDetail, ChangeStatus } from "../api/types";
import TagNav from "./TagNav";

afterEach(cleanup);

/** A queried node that must exist for the test to make sense. */
function must<T>(value: T | null | undefined, what: string): T {
  if (value == null) throw new Error(`expected ${what}`);
  return value;
}

/** A change whose latest revision carries `status` and `unresolved` open
 * threads: what a row reads. */
function detail(
  changeNumber: number,
  subject: string,
  status: ChangeStatus,
  unresolved: number,
): ChangeDetail {
  return {
    id: changeNumber,
    repo_id: 1,
    change_id: `I${changeNumber}`,
    revisions: [
      {
        number: 0,
        commit_sha: `sha${changeNumber}`,
        parent_sha: "",
        fork_sha: "",
        message: `${subject}\n\nbody`,
        subject,
        created_at: "",
        status,
      },
    ],
    threads: Array.from({ length: unresolved }, (_, i) => ({
      id: i,
      change_number: changeNumber,
      revision: 0,
      anchor: "change" as const,
      resolved: false,
      comments: [],
      created_at: "",
      updated_at: "",
    })),
    drafts: [],
    reviews: [],
    draft_decision: null,
  };
}

const members = [
  detail(10, "first change", "approved", 0),
  detail(11, "second change", "changes_requested", 2),
  detail(12, "third change", "pending", 0),
];

const renderNav = (currentId: number, onSelectKey = vi.fn()) =>
  render(
    <MemoryRouter>
      <TagNav
        tags={{ branch: "feat", "session-id": "s1" }}
        selectedKey="session-id"
        onSelectKey={onSelectKey}
        members={members}
        currentId={currentId}
      />
    </MemoryRouter>,
  );

const list = () => document.querySelector(".tag-nav-list");

describe("TagNav", () => {
  it("offers the change's tag keys and reports the chosen one", () => {
    const onSelectKey = vi.fn();
    renderNav(11, onSelectKey);
    const select = screen.getByLabelText<HTMLSelectElement>("Tag");
    expect(select.disabled).toBe(false);
    expect([...select.options].map((o) => o.value)).toEqual([
      "branch",
      "session-id",
    ]);
    expect(select.value).toBe("session-id");

    fireEvent.change(select, { target: { value: "branch" } });
    expect(onSelectKey).toHaveBeenCalledWith("branch");
  });

  it("disables the selector for a change with no tags", () => {
    render(
      <MemoryRouter>
        <TagNav
          tags={{}}
          selectedKey={null}
          onSelectKey={vi.fn()}
          members={[]}
          currentId={11}
        />
      </MemoryRouter>,
    );
    expect(screen.getByLabelText<HTMLSelectElement>("Tag").disabled).toBe(true);
    expect(document.querySelectorAll(".tag-nav-row")).toHaveLength(0);
  });

  it("lists every member, links the siblings, and marks the current one", () => {
    renderNav(11);
    expect(document.querySelector(".tag-nav-pos")?.textContent).toBe("2/3");

    expect(document.querySelectorAll(".tag-nav-row")).toHaveLength(3);

    const links = screen.getAllByRole("link");
    expect(links.map((a) => a.getAttribute("href"))).toEqual([
      "/changes/10",
      "/changes/12",
    ]);

    // A div, not a link, so the current page never self-links; aria-current
    // flags it for assistive tech.
    const current = must(
      document.querySelector<HTMLElement>(".tag-nav-row.current"),
      ".tag-nav-row.current",
    );
    expect(current.tagName).toBe("DIV");
    expect(current.getAttribute("aria-current")).toBe("page");
    expect(within(current).getByText("second change")).toBeTruthy();
    expect(within(current).getByText("2 open")).toBeTruthy();
    expect(document.querySelectorAll(".unresolved-count")).toHaveLength(1);
  });

  it("collapses and expands the list from the disclosure", () => {
    renderNav(11);
    const toggle = screen.getByRole("button");

    // Defaults open: the sidebar has room, so the list is visible up front.
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    expect(list()).not.toBeNull();

    fireEvent.click(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    expect(list()).toBeNull();

    fireEvent.click(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    expect(list()).not.toBeNull();
  });
});
