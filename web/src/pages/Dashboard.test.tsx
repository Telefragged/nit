// The repo dashboard renders the change graph, centered on the canonical ref, against the
// mock fixtures (VITE_MOCK is set by the vitest config).
// Repo 1's open changes ascend above HEAD; the Activity column carries each
// change's draft state, read per node from its drafts overlay. Change 12 has
// a seeded request_changes decision, so its row shows "✎ request_changes".

import { cleanup, fireEvent, screen, within } from "@testing-library/react";
import { Route } from "react-router-dom";
import { afterEach, describe, expect, it } from "vitest";
import { renderPage } from "../test/page";
import Dashboard from "./Dashboard";

afterEach(cleanup);

const renderDashboard = (repo = 1, search = "") =>
  renderPage(
    `/repos/${repo}${search}`,
    <Route path="/repos/:repoId" element={<Dashboard />} />,
  );

describe("repo dashboard change graph", () => {
  it("renders open changes linking to their change pages", async () => {
    await renderDashboard();
    const subject = screen.getByText(
      "auth: document rotation and ship flow diagram",
    );
    expect(subject.closest("a")?.getAttribute("href")).toBe("/changes/12");
  });

  it("preserves the Activity column with each change's draft state", async () => {
    await renderDashboard();
    const subject = screen.getByText(
      "auth: document rotation and ship flow diagram",
    );

    expect(screen.getByText("Activity")).toBeTruthy();
    const row = subject.closest(".graph-row");
    if (!(row instanceof HTMLElement)) throw new Error("no row for change 12");
    expect(within(row).getByText("✎ request_changes")).toBeTruthy();
  });

  it("groups by the tag key the URL names, labelling each run", async () => {
    await renderDashboard(4, "?group=session-id");
    const alpha = screen.getByText("alpha", { selector: ".graph-gap-label" });
    const beta = screen.getByText("beta", { selector: ".graph-gap-label" });
    expect(screen.getByLabelText("Group by")).toHaveProperty(
      "value",
      "session-id",
    );
    // Beta's run sits above alpha's run. The change stacked on alpha's tip
    // is the first row of beta's run.
    expect(
      beta.compareDocumentPosition(alpha) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it("keeps only the changes carrying the value the URL names", async () => {
    await renderDashboard(4, "?group=session-id&value=beta");
    expect(
      screen.getByText("lumen: evict cached manifests by age"),
    ).toBeTruthy();
    expect(screen.getByLabelText("Only")).toHaveProperty("value", "beta");
    expect(screen.queryByText("lumen: parse the manifest lazily")).toBeNull();
    // The filter excluded the stacked change's parent: a break mark cuts
    // its edge to the fork.
    expect(document.querySelector(".graph-break")).not.toBeNull();
  });

  it("offers the tag keys the repo's live changes carry", async () => {
    await renderDashboard(4);
    expect(screen.getByRole("option", { name: "session-id" })).toBeTruthy();
    // Repo 4 puts `branch` on its abandoned change alone.
    expect(screen.queryByRole("option", { name: "branch" })).toBeNull();
  });

  it("breaks the chain where an abandoned change sat", async () => {
    // Repo 4's gamma session stacks a live change on an abandoned one.
    await renderDashboard(4, "?group=session-id&value=gamma");
    expect(
      screen.getByText("lumen: verify a pinned manifest on read"),
    ).toBeTruthy();
    expect(
      screen.queryByText("lumen: pin manifests to a content hash"),
    ).toBeNull();
    expect(document.querySelector(".graph-break")).not.toBeNull();
  });

  it("restores the last grouping and clears the filter", async () => {
    localStorage.setItem("nit.graph-group.4", "session-id");
    await renderDashboard(4, "?value=beta");

    expect(
      screen.getByText("alpha", { selector: ".graph-gap-label" }),
    ).toBeTruthy();
    expect(screen.getByLabelText("Group by")).toHaveProperty(
      "value",
      "session-id",
    );
    expect(screen.getByLabelText("Only")).toHaveProperty("value", "");
  });

  it("groups by the URL's key, not the remembered one", async () => {
    localStorage.setItem("nit.graph-group.4", "session-id");
    await renderDashboard(4, "?group=none-such");

    expect(screen.getByText("lumen: parse the manifest lazily")).toBeTruthy();
    expect(screen.getByLabelText("Group by")).toHaveProperty(
      "value",
      "none-such",
    );
  });

  it("remembers the grouping per repo", async () => {
    await renderDashboard(4);
    fireEvent.change(screen.getByLabelText("Group by"), {
      target: { value: "session-id" },
    });

    expect(
      screen.getByText("alpha", { selector: ".graph-gap-label" }),
    ).toBeTruthy();
    expect(localStorage.getItem("nit.graph-group.4")).toBe("session-id");
    expect(localStorage.getItem("nit.graph-group.1")).toBeNull();
  });
});
