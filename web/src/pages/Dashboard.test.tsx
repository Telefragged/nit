// Dashboard chain drawers: the draft-state count pill and the per-chain batch
// submit (docs/api.md "Reviewer decisions"), against the mock fixtures
// (VITE_MOCK is set by the vitest config). Repo 1's chain (tip 12) has change
// 11 carrying comment drafts and change 12 a seeded staged decision, so the
// drawer shows "✎ 2 drafts" and "Submit (1)".

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, describe, expect, it } from "vitest";
import Dashboard from "./Dashboard";

afterEach(cleanup);

function renderDashboard(repo = 1) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={[`/repos/${repo}`]}>
        <Routes>
          <Route path="/repos/:repoId" element={<Dashboard />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

/** The chain-12 drawer (its toggle carries the chain name "feat/token-rotation"). */
async function drawer(): Promise<HTMLElement> {
  const name = await screen.findByText("feat/token-rotation");
  const section = name.closest(".chain-drawer");
  if (!(section instanceof HTMLElement)) throw new Error("no drawer");
  return section;
}

describe("chain drawer draft-state count + submit", () => {
  it("counts changes with draft state and offers submit for staged decisions", async () => {
    renderDashboard();
    const d = await drawer();

    // change 11 (comment drafts) + change 12 (a staged decision) = 2 changes
    // with draft state; multiple comments on one change still count once.
    expect(within(d).getByText("✎ 2 drafts")).toBeTruthy();
    // Only the one staged decision is submittable.
    expect(within(d).getByRole("button", { name: "Submit (1)" })).toBeTruthy();

    // The staged decision shows on its change's row when the drawer is open.
    fireEvent.click(within(d).getByText("feat/token-rotation"));
    expect(within(d).getByText("✎ request_changes")).toBeTruthy();
  });

  it("submitting publishes the staged decision and clears it from the chain", async () => {
    renderDashboard();
    const d = await drawer();

    fireEvent.click(within(d).getByRole("button", { name: "Submit (1)" }));

    // The decision published: the submit button is gone (nothing staged) and
    // the count drops to the lone comment-draft change.
    await waitFor(() => {
      expect(within(d).queryByRole("button", { name: /^Submit/ })).toBeNull();
    });
    expect(within(d).getByText("✎ 1 draft")).toBeTruthy();
  });
});
