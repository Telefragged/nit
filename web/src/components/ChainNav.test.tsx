import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it } from "vitest";
import type { Chain, ChangeStatus } from "../api/types";
import ChainNav from "./ChainNav";

afterEach(cleanup);

/** A queried node that must exist for the test to make sense. */
function must<T>(value: T | null | undefined, what: string): T {
  if (value == null) throw new Error(`expected ${what}`);
  return value;
}

function change(
  id: number,
  position: number,
  subject: string,
  status: ChangeStatus,
  unresolved = 0,
) {
  return {
    id,
    position,
    change_key: `I${id}`,
    subject,
    status,
    revision: 1,
    last_reviewed_revision: null,
    commit_sha: `sha${id}`,
    short_sha: `sha${id}`,
    counts: { revisions: 1, published_comments: 0, drafts: 0, unresolved },
  };
}

const chain: Chain = {
  id: 1,
  repo_path: "/repo",
  branch: "feat/x",
  base: "main",
  status: "active",
  state: "waiting_for_review",
  partial: false,
  last_scan_error: null,
  web_url: "http://x/chains/1",
  created_at: "2026-06-14T00:00:00Z",
  updated_at: "2026-06-14T00:00:00Z",
  changes: [
    change(10, 0, "first change", "approved"),
    change(11, 1, "second change", "changes_requested", 2),
    change(12, 2, "third change", "pending"),
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

  it("lists every change, links the siblings, and marks the current one", () => {
    renderNav(11);
    // The header tracks the current change's 1-based position over the count.
    expect(screen.getByRole("button").textContent).toContain("2/3");

    expect(document.querySelectorAll(".chain-nav-row")).toHaveLength(3);

    // Siblings link through to their change; the current one is not a link.
    const links = screen.getAllByRole("link");
    expect(links.map((a) => a.getAttribute("href"))).toEqual([
      "/changes/10",
      "/changes/12",
    ]);

    // Current change: a non-link row, flagged for assistive tech, highlighted,
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
