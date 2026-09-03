// The repo dashboard renders the change graph, centered on the canonical ref, against the
// mock fixtures (VITE_MOCK is set by the vitest config).
// Repo 1's open changes ascend above HEAD; the Activity column carries each
// change's draft state, fetched per node from GET /api/changes/{id} — change
// 12 has a seeded request_changes decision, so its row shows
// "✎ request_changes" once that fetch resolves.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, describe, expect, it } from "vitest";
import Dashboard from "./Dashboard";

afterEach(cleanup);

function renderDashboard(repo = 1, search = "") {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={[`/repos/${repo}${search}`]}>
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
    const subject = await screen.findByText(
      "auth: document rotation and ship flow diagram",
    );

    expect(screen.getByText("Activity")).toBeTruthy();
    // Change 12's seeded draft decision shows in its activity cell — it
    // arrives from the per-change fetch, so await it rather than reading sync.
    const row = subject.closest(".graph-row");
    if (!(row instanceof HTMLElement)) throw new Error("no row for change 12");
    expect(await within(row).findByText("✎ request_changes")).toBeTruthy();
  });

  it("groups by the tag key the URL names, labelling each run", async () => {
    renderDashboard(4, "?group=session");
    const alpha = await screen.findByText("alpha", {
      selector: ".graph-gap-label",
    });
    const beta = screen.getByText("beta", { selector: ".graph-gap-label" });
    expect(screen.getByLabelText("Group by")).toHaveProperty(
      "value",
      "session",
    );
    // Beta's run sits above alpha's run. The change stacked on alpha's tip
    // is the first row of beta's run.
    expect(
      beta.compareDocumentPosition(alpha) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it("keeps only the changes carrying the value the URL names", async () => {
    renderDashboard(4, "?group=session&value=beta");
    await screen.findByText("lumen: evict cached manifests by age");
    expect(screen.getByLabelText("Only")).toHaveProperty("value", "beta");
    expect(screen.queryByText("lumen: parse the manifest lazily")).toBeNull();
    // The filter excluded the stacked change's parent: a break mark cuts
    // its edge to the fork.
    expect(document.querySelector(".graph-break")).not.toBeNull();
  });

  it("offers the tag keys the repo's changes carry", async () => {
    renderDashboard(4);
    expect(await screen.findByRole("option", { name: "session" })).toBeTruthy();
  });

  it("restores the last grouping and clears the filter", async () => {
    localStorage.setItem("nit.graph-group.4", "session");
    renderDashboard(4, "?value=beta");

    await screen.findByText("alpha", { selector: ".graph-gap-label" });
    expect(screen.getByLabelText("Group by")).toHaveProperty(
      "value",
      "session",
    );
    expect(screen.getByLabelText("Only")).toHaveProperty("value", "");
  });

  it("groups by the URL's key, not the remembered one", async () => {
    localStorage.setItem("nit.graph-group.4", "session");
    renderDashboard(4, "?group=none-such");

    await screen.findByText("lumen: parse the manifest lazily");
    expect(screen.getByLabelText("Group by")).toHaveProperty(
      "value",
      "none-such",
    );
  });

  it("remembers the grouping per repo", async () => {
    renderDashboard(4);
    // The selector offers `session` once the repo's tags arrive.
    await screen.findByRole("option", { name: "session" });
    fireEvent.change(screen.getByLabelText("Group by"), {
      target: { value: "session" },
    });

    await screen.findByText("alpha", { selector: ".graph-gap-label" });
    expect(localStorage.getItem("nit.graph-group.4")).toBe("session");
    expect(localStorage.getItem("nit.graph-group.1")).toBeNull();
  });
});
