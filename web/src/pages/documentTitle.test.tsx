// The browser tab names the page against the mock fixtures (VITE_MOCK is
// set by the vitest config): repo 1 is /home/vetle/src/acme-runtime, and
// change 11 is "auth: rotate refresh tokens on use".

import { cleanup, screen } from "@testing-library/react";
import { Route } from "react-router-dom";
import { afterEach, describe, expect, it } from "vitest";
import { renderPage } from "../test/page";
import Dashboard from "./Dashboard";
import RepoList from "./RepoList";
import ReviewPage from "./ReviewPage";

afterEach(() => {
  cleanup();
  document.title = "nit";
});

const renderAt = (url: string) =>
  renderPage(
    url,
    <>
      <Route path="/" element={<RepoList />} />
      <Route path="/repos/:repoId" element={<Dashboard />} />
      <Route path="/changes/:id" element={<ReviewPage />} />
    </>,
  );

describe("the document title", () => {
  it("names the repo on the change graph", async () => {
    await renderAt("/repos/1");
    expect(document.title).toBe("/home/vetle/src/acme-runtime - nit");
  });

  it("names the change's subject on the review page", async () => {
    await renderAt("/changes/11?against=base");
    expect(document.title).toBe("auth: rotate refresh tokens on use - nit");
  });

  it("leaves the plain product name on the repo list", async () => {
    await renderAt("/");
    expect(screen.getByText("/home/vetle/src/acme-runtime")).toBeTruthy();
    expect(document.title).toBe("nit");
  });

  it("restores the plain product name when the page unmounts", async () => {
    const view = await renderAt("/repos/1");
    expect(document.title).toBe("/home/vetle/src/acme-runtime - nit");
    view.unmount();
    expect(document.title).toBe("nit");
  });
});
