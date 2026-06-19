// The repo dashboard renders the spine-centered change graph (docs/api.md
// "Graph") against the mock fixtures (VITE_MOCK is set by the vitest config).
// Repo 1's open changes ascend above HEAD; the preserved Activity column
// carries each change's draft state — change 12 has a seeded request_changes
// decision, so its row shows "✎ request_changes".

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen, within } from "@testing-library/react";
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

describe("repo dashboard change graph", () => {
  it("renders open changes linking to their change pages", async () => {
    renderDashboard();
    const subject = await screen.findByText(
      "auth: document rotation and ship flow diagram",
    );
    expect(subject.closest("a")?.getAttribute("href")).toBe("/changes/12");
  });

  it("preserves the Activity column with each change's draft state", async () => {
    renderDashboard();
    await screen.findByText("auth: document rotation and ship flow diagram");

    // The Activity column header is preserved from the per-chain table.
    expect(screen.getByText("Activity")).toBeTruthy();
    // Change 12's seeded staged decision shows in its activity cell.
    const row = document.getElementById("chain-12");
    if (!(row instanceof HTMLElement)) throw new Error("no row for change 12");
    expect(within(row).getByText("✎ request_changes")).toBeTruthy();
  });
});
