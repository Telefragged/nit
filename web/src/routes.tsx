import { type ComponentType, Suspense } from "react";
import { createBrowserRouter } from "react-router-dom";
import App from "./App.tsx";
import Repos, { NoRepo } from "./pages/Repos.tsx";
import NotFound from "./pages/NotFound.tsx";
import FoldReady from "./components/FoldReady.tsx";

/** A route whose page folds. The page loads on demand, and only the page
 * waits for the fold, so the repo sidebar renders without it. */
const folding = (
  path: string,
  page: () => Promise<{ default: ComponentType }>,
) => ({
  path,
  lazy: async () => {
    const { default: Page } = await page();
    return {
      element: (
        <Suspense fallback={null}>
          <FoldReady>
            <Page />
          </FoldReady>
        </Suspense>
      ),
    };
  },
});

export const router = createBrowserRouter([
  {
    path: "/",
    element: <App />,
    // The router resolves a lazy route before its first render, and asks
    // for this while it does.
    HydrateFallback: () => null,
    children: [
      {
        element: <Repos />,
        children: [
          { index: true, element: <NoRepo /> },
          folding("repos/:repoId", () => import("./pages/Dashboard.tsx")),
        ],
      },
      folding("changes/:id", () => import("./pages/ReviewPage.tsx")),
      { path: "*", element: <NotFound /> },
    ],
  },
]);
