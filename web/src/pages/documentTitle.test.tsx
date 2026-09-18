// The browser tab names the page against the mock fixtures (VITE_MOCK is
// set by the vitest config): repo 1 is /home/vetle/src/acme-runtime, and
// change 11 is "auth: rotate refresh tokens on use".

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, describe, expect, it } from "vitest";
import Dashboard from "./Dashboard";
import RepoList from "./RepoList";
import ReviewPage from "./ReviewPage";

afterEach(() => {
  cleanup();
  document.title = "nit";
});

function renderAt(url: string) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={[url]}>
        <Routes>
          <Route path="/" element={<RepoList />} />
          <Route path="/repos/:repoId" element={<Dashboard />} />
          <Route path="/changes/:id" element={<ReviewPage />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

describe("the document title", () => {
  it("names the repo on the change graph", async () => {
    renderAt("/repos/1");
    await waitFor(() => {
      expect(document.title).toBe("/home/vetle/src/acme-runtime - nit");
    });
  });

  it("names the change's subject on the review page", async () => {
    renderAt("/changes/11?against=base");
    await waitFor(() => {
      expect(document.title).toBe("auth: rotate refresh tokens on use - nit");
    });
  });

  it("leaves the plain product name on the repo list", async () => {
    renderAt("/");
    await screen.findByText("/home/vetle/src/acme-runtime");
    expect(document.title).toBe("nit");
  });

  it("restores the plain product name when the page unmounts", async () => {
    const view = renderAt("/repos/1");
    await waitFor(() => {
      expect(document.title).toBe("/home/vetle/src/acme-runtime - nit");
    });
    view.unmount();
    expect(document.title).toBe("nit");
  });
});
