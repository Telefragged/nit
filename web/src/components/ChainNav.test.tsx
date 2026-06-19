import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it } from "vitest";
import type { Chain, ChangeStatus, PathEntry } from "../api/types";
import ChainNav from "./ChainNav";

afterEach(cleanup);

/** A queried node that must exist for the test to make sense. */
function must<T>(value: T | null | undefined, what: string): T {
  if (value == null) throw new Error(`expected ${what}`);
  return value;
}

function member(
  changeId: number,
  position: number,
  subject: string,
  status: ChangeStatus,
  unresolved = 0,
  extra: Partial<PathEntry> = {},
): PathEntry {
  return {
    change_id: changeId,
    position,
    change_key: `I${changeId}`,
    subject,
    status,
    revision: 0,
    latest_revision: 0,
    newer_elsewhere: false,
    merged_elsewhere: false,
    commit_sha: `sha${changeId}`,
    short_sha: `sha${changeId}`,
    counts: { threads: 0, drafts: 0, unresolved },
    ...extra,
  };
}

const chain: Chain = {
  tip_change_id: 12,
  repo_id: 1,
  name: "feat/x",
  base_branch: "main",
  state: "waiting_for_review",
  partial: false,
  web_url: "http://x/repos/1#chain-12",
  path: [
    member(10, 0, "first change", "approved"),
    member(11, 1, "second change", "changes_requested", 2, {
      newer_elsewhere: true,
      revision: 0,
      latest_revision: 2,
    }),
    member(12, 2, "third change", "pending"),
  ],
};

const renderNav = (currentId: number) =>
  render(
    <MemoryRouter>
      <ChainNav chain={chain} currentId={currentId} />
    </MemoryRouter>,
  );

const list = () => document.querySelector(".chain-nav-list");

describe("ChainNav", () => {
  it("renders nothing without a chain", () => {
    const { container } = render(
      <MemoryRouter>
        <ChainNav chain={undefined} currentId={11} />
      </MemoryRouter>,
    );
    expect(container.firstChild).toBeNull();
  });

  it("lists every member, links the siblings, and marks the current one", () => {
    renderNav(11);
    // The header tracks the current member's 1-based position over the count.
    expect(screen.getByRole("button").textContent).toContain("2/3");

    expect(document.querySelectorAll(".chain-nav-row")).toHaveLength(3);

    // Siblings link through to their change; the current one is not a link.
    const links = screen.getAllByRole("link");
    expect(links.map((a) => a.getAttribute("href"))).toEqual([
      "/changes/10",
      "/changes/12",
    ]);

    // Current member: a non-link row, flagged for assistive tech, highlighted,
    // and the only one carrying its open-thread count.
    const current = must(
      document.querySelector<HTMLElement>(".chain-nav-row.current"),
      ".chain-nav-row.current",
    );
    expect(current.tagName).toBe("DIV");
    expect(current.getAttribute("aria-current")).toBe("page");
    expect(within(current).getByText("2 open")).toBeTruthy();
    expect(document.querySelectorAll(".unresolved-count")).toHaveLength(1);
  });

  it("badges a member pinned to an older revision than its latest", () => {
    renderNav(11);
    // The current member pins r0 while r2 lives on another chain.
    const current = must(
      document.querySelector<HTMLElement>(".chain-nav-row.current"),
      ".chain-nav-row.current",
    );
    expect(within(current).getByText("NEWER ELSEWHERE")).toBeTruthy();
    expect(document.querySelectorAll(".badge")).toHaveLength(1);
  });

  it("collapses and expands the list from the disclosure header", () => {
    renderNav(11);
    const toggle = screen.getByRole("button");

    // Defaults open: the sidebar has room, so the chain is visible up front.
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
