// The change page is event-driven: it subscribes for a ChangeProjection,
// then folds the live tail (crates/nit-wasm) into the published state. These
// tests drive the mock stream directly. The mock hands an appended entry to
// the page's listener in the same call, so the act around the append commits
// the fold.

import { act, cleanup, fireEvent, screen } from "@testing-library/react";
import { Route } from "react-router-dom";
import { afterEach, describe, expect, it } from "vitest";

import { mockAppend } from "../api/fixtures/stream";
import { shaOf as sha } from "../api/fixtures/builders";
import { renderPage } from "../test/page";
import ReviewPage from "./ReviewPage";

afterEach(cleanup);

const renderReview = (url: string) =>
  renderPage(url, <Route path="/changes/:id" element={<ReviewPage />} />);

const revHead = () => screen.getByLabelText<HTMLButtonElement>("Revision");
/** The rows the open picker offers, each as it reads on screen. */
const revOptions = () =>
  screen.queryAllByRole("option").map((o) => o.textContent);

describe("event-driven change page", () => {
  it("makes a pushed revision selectable without jumping to it", async () => {
    await renderReview("/changes/11");
    // The list stays open across the live event, so the rows it gains are
    // the assertion.
    fireEvent.click(revHead());
    // The projection on subscribe gives r0 and r1 before any live event.
    expect(revOptions()).toEqual(["r0 6 comments", "r1 3 comments"]);
    expect(revHead().textContent).toBe("r1 3 comments");

    act(() => {
      mockAppend(11, "2026-06-28T00:00:00.000Z", {
        kind: "revision",
        payload: {
          commit_sha: sha("c11r3"),
          parent_sha: sha("c11r2"),
          fork_sha: sha("base"),
          message: "auth: rotate v3\n\nChange-Id: I9a41c7e2b3d4f5a6",
          resets_status: true,
        },
      });
    });

    expect(revOptions()).toEqual(["r0 6 comments", "r1 3 comments", "r2"]);
    expect(revHead().textContent).toBe("r1 3 comments");
  });

  it("lists the change log and appends a live entry to it", async () => {
    await renderReview("/changes/12");
    const rows = () => document.querySelectorAll(".change-log .log-row");
    const before = rows().length;
    expect(before).toBeGreaterThan(0);

    act(() => {
      mockAppend(12, "2026-06-28T00:00:00.000Z", {
        kind: "tags",
        payload: { tags: { "session-id": "live-tag" } },
      });
    });

    expect(rows().length).toBe(before + 1);
    // Change 12 carries a session-id already, so the entry replaces it.
    expect(rows()[before]?.textContent).toContain("+1 −1 tags");
    expect(rows()[before]?.textContent).toContain("+ session-id=live-tag");
  });

  it("names the status a pushed revision reset", async () => {
    await renderReview("/changes/12");
    const rows = () => document.querySelectorAll(".change-log .log-row");

    act(() => {
      mockAppend(12, "2026-06-28T00:00:00.000Z", {
        kind: "review",
        payload: {
          revision: 0,
          verdict: "approve",
          message: "ship it",
          comments: [],
        },
      });
      mockAppend(12, "2026-06-28T00:01:00.000Z", {
        kind: "revision",
        payload: {
          commit_sha: sha("c12r1"),
          parent_sha: sha("c12r0"),
          fork_sha: sha("base"),
          message: "c12 v2\n\nChange-Id: I12",
          resets_status: true,
        },
      });
    });

    expect(rows()[rows().length - 1]?.textContent).toContain(
      "status reset to PENDING",
    );
  });
});
