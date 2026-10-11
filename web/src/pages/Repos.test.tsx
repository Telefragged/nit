// The repo sidebar against the mock fixtures (VITE_MOCK is set by the
// vitest config): repo 4 is /home/vetle/src/lumen.

import { cleanup, screen } from "@testing-library/react";
import { Route } from "react-router-dom";
import { afterEach, describe, expect, it } from "vitest";
import { renderPage } from "../test/page";
import Repos, { NoRepo } from "./Repos";

afterEach(cleanup);

const renderAt = (url: string) =>
  renderPage(
    url,
    <Route path="/" element={<Repos />}>
      <Route index element={<NoRepo />} />
      <Route path="repos/:repoId" element={<main />} />
    </Route>,
  );

const current = () =>
  document.querySelectorAll('.repo-sidebar [aria-current="page"]');

describe("the repo sidebar", () => {
  it("highlights the repo whose graph is open", async () => {
    await renderAt("/repos/4");
    expect([...current()].map((a) => a.getAttribute("href"))).toEqual([
      "/repos/4",
    ]);
    expect(current()[0]?.textContent).toContain("lumen");
  });

  it("highlights no repo before one is picked", async () => {
    await renderAt("/");
    expect(
      screen.getByText("Pick a repository to see its change graph."),
    ).toBeTruthy();
    expect(current()).toHaveLength(0);
  });
});
