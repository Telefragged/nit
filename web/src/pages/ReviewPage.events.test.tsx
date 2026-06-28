// The change page is event-driven: it subscribes for a ChangeProj snapshot,
// then folds the live tail (crates/nit-wasm) into the published state. These
// tests drive the mock stream directly — append an entry, then wait (bounded by
// a timeout, never polling) for the page to fold and re-render.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, describe, expect, it } from "vitest";

import { mockAppend } from "../api/fixtures/stream";
import ReviewPage from "./ReviewPage";

afterEach(cleanup);

function renderReview(url: string) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={[url]}>
        <Routes>
          <Route path="/changes/:id" element={<ReviewPage />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

const revSelect = () => screen.getByLabelText<HTMLSelectElement>("Revision");
const revOptions = () => Array.from(revSelect().options).map((o) => o.value);

describe("event-driven change page", () => {
  it("makes a pushed revision selectable without jumping to it", async () => {
    renderReview("/changes/11");
    // Snapshot on subscribe gives r0 and r1 before any live event.
    await waitFor(() => {
      expect(revOptions()).toEqual(["0", "1"]);
    });
    expect(revSelect().value).toBe("1");

    mockAppend(11, "2026-06-28T00:00:00.000Z", {
      kind: "revision",
      payload: {
        commit_sha: "c11r3",
        parent_sha: "c11r2",
        base_sha: "base",
        message: "auth: rotate v3\n\nChange-Id: I9a41c7e2b3d4f5a6",
        resets_status: true,
      },
    });

    await waitFor(() => {
      expect(revOptions()).toEqual(["0", "1", "2"]);
    });
    expect(revSelect().value).toBe("1");
  });

  it("folds a review published over the websocket into the page", async () => {
    renderReview("/changes/12");
    await screen.findByLabelText("Revision");

    mockAppend(12, "2026-06-28T00:00:00.000Z", {
      kind: "review",
      payload: {
        review_id: 9001,
        revision: 0,
        verdict: "comment",
        message: "folded-live cover note",
        comments: [],
      },
    });

    expect(await screen.findByText("folded-live cover note")).toBeTruthy();
  });
});
