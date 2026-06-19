import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createBrowserRouter, RouterProvider } from "react-router-dom";
import App from "./App.tsx";
import RepoList from "./pages/RepoList.tsx";
import Dashboard from "./pages/Dashboard.tsx";
import ReviewPage from "./pages/ReviewPage.tsx";
import NotFound from "./pages/NotFound.tsx";
import "./styles.css";

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: 1,
      refetchOnWindowFocus: false,
      staleTime: 2_000,
    },
  },
});

const router = createBrowserRouter([
  {
    path: "/",
    element: <App />,
    children: [
      { index: true, element: <RepoList /> },
      { path: "repos/:repoId", element: <Dashboard /> },
      { path: "changes/:id", element: <ReviewPage /> },
      { path: "*", element: <NotFound /> },
    ],
  },
]);

const rootEl = document.getElementById("root");
if (!rootEl) throw new Error("missing #root element");
createRoot(rootEl).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  </StrictMode>,
);
