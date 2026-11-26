import { type ComponentType, Suspense } from "react";
import { createBrowserRouter } from "react-router-dom";
import App from "./App.tsx";
import NoRepo from "./pages/NoRepo.tsx";
import NotFound from "./pages/NotFound.tsx";
import FoldReady from "./components/FoldReady.tsx";

const lazyFoldReadyRoute = (
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
      { index: true, element: <NoRepo /> },
      lazyFoldReadyRoute(
        "repos/:repoId",
        () => import("./pages/Dashboard.tsx"),
      ),
      lazyFoldReadyRoute("changes/:id", () => import("./pages/ReviewPage.tsx")),
      lazyFoldReadyRoute(
        "documents/:number",
        () => import("./pages/DocumentPage.tsx"),
      ),
      { path: "*", element: <NotFound /> },
    ],
  },
]);
