import { cleanup, screen } from "@testing-library/react";
import { Outlet, Route } from "react-router-dom";
import { afterEach, describe, expect, it } from "vitest";
import { repos } from "../api/fixtures/data";
import { renderPage, user } from "../test/page";
import { SidebarLayout } from "./AppSidebar";

afterEach(cleanup);

const lumenId = repos.find((r) => r.git_dir.endsWith("/lumen/.git"))?.id;
const lumenHref = `/repos/${String(lumenId)}`;

const renderAt = (url: string) =>
  renderPage(
    url,
    <Route
      path="/"
      element={
        <SidebarLayout>
          <Outlet />
        </SidebarLayout>
      }
    >
      <Route index element={<main />} />
      <Route path="repos/:repoId" element={<main />} />
    </Route>,
  );

const active = () =>
  document.querySelectorAll('[data-sidebar="menu-sub-button"][data-active]');

describe("the app sidebar", () => {
  it("links each repo to its change graph", async () => {
    await renderAt("/");
    const link = [
      ...document.querySelectorAll('[data-sidebar="menu-sub-button"]'),
    ].find((a) => a.querySelector("span")?.textContent === "lumen");
    expect(link?.getAttribute("href")).toBe(lumenHref);
  });

  it("highlights the repo whose graph is open", async () => {
    await renderAt(lumenHref);
    expect([...active()].map((a) => a.getAttribute("href"))).toEqual([
      lumenHref,
    ]);
  });

  it("highlights no repo before one is picked", async () => {
    await renderAt("/");
    expect(active()).toHaveLength(0);
  });

  it("hides the repos when the reviewer collapses the group", async () => {
    const view = await renderAt("/");
    await user.click(screen.getByRole("button", { name: "Repositories" }));
    await view.settle();
    expect(
      document.querySelectorAll('[data-sidebar="menu-sub-button"]'),
    ).toHaveLength(0);
  });
});
