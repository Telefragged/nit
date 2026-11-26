import { cleanup, screen } from "@testing-library/react";
import { Outlet, Route } from "react-router-dom";
import { afterEach, describe, expect, it } from "vitest";
import { repos } from "../api/fixtures/data";
import { renderPage, user } from "../test/page";
import { SidebarLayout } from "./AppSidebar";

afterEach(() => {
  cleanup();
  localStorage.clear();
});

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

const sidebarWidth = () =>
  document
    .querySelector<HTMLElement>('[data-slot="sidebar-wrapper"]')
    ?.style.getPropertyValue("--sidebar-width");

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

  it("opens collapsed when the reviewer collapsed it before", async () => {
    localStorage.setItem("nit.sidebar-open", "false");
    await renderAt("/");
    expect(
      document.querySelector<HTMLElement>('[data-slot="sidebar"]')?.dataset
        .state,
    ).toBe("collapsed");
  });

  it("opens at the width that the reviewer dragged it to before", async () => {
    localStorage.setItem("nit.sidebar-width", "300");
    await renderAt("/");
    expect(sidebarWidth()).toBe("300px");
  });

  it("keeps a saved width inside the allowed widths", async () => {
    localStorage.setItem("nit.sidebar-width", "999");
    await renderAt("/");
    expect(sidebarWidth()).toBe("320px");
  });
});
