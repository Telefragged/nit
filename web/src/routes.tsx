import { createBrowserRouter, Outlet } from "react-router-dom";
import App from "./App.tsx";
import RepoList from "./pages/RepoList.tsx";
import NotFound from "./pages/NotFound.tsx";
import FoldReady from "./components/FoldReady.tsx";

export const router = createBrowserRouter([
  {
    path: "/",
    element: <App />,
    // The router resolves a lazy route before its first render, and asks
    // for this while it does.
    HydrateFallback: () => null,
    children: [
      { index: true, element: <RepoList /> },
      // Both routes below fold, and neither is on the path to the repo
      // list, so they load their page on demand behind one gate.
      {
        element: (
          <FoldReady>
            <Outlet />
          </FoldReady>
        ),
        children: [
          {
            path: "repos/:repoId",
            lazy: async () => ({
              Component: (await import("./pages/Dashboard.tsx")).default,
            }),
          },
          {
            path: "changes/:id",
            lazy: async () => ({
              Component: (await import("./pages/ReviewPage.tsx")).default,
            }),
          },
        ],
      },
      { path: "*", element: <NotFound /> },
    ],
  },
]);
