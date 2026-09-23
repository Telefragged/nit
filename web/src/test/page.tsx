import {
  notifyManager,
  QueryClient,
  QueryClientProvider,
} from "@tanstack/react-query";
import { act, render } from "@testing-library/react";
import type { ReactNode } from "react";
import { MemoryRouter, Routes } from "react-router-dom";

const busy = (client: QueryClient) =>
  client.isFetching() + client.isMutating() > 0;

/** Resolves once nothing is fetching or mutating, and every observer has
 * heard the last cache event. */
function idle(client: QueryClient): Promise<void> {
  return new Promise((resolve) => {
    let last: (() => void) | undefined;
    const check = () => {
      if (busy(client)) return;
      // The observers hear this event on the notify queue, so a callback
      // queued now runs after them. React can commit and start a fetch
      // before the callback runs. That fetch's events queue a later
      // callback, and only the last one resolves.
      const done = () => {
        if (done !== last || busy(client)) return;
        for (const stop of stops) stop();
        resolve();
      };
      last = done;
      notifyManager.schedule(done);
    };
    const caches = [client.getQueryCache(), client.getMutationCache()];
    const stops = caches.map((cache) => cache.subscribe(check));
    check();
  });
}

/** Commits every fetch and mutation in flight, and every one their commits
 * start.
 *
 * Each round awaits `idle` inside `act`. React commits the queued work as
 * `act` returns, and the effects of that commit can start new fetches. A
 * fetch marks itself in flight in the same call that starts it, so the
 * loop sees it and runs another round. The loop ends after a round that
 * starts nothing.
 *
 * react-query's notify queue runs on a zero-delay task, after every queued
 * microtask. So an awaited fetch's continuation and the file tree's Preact
 * render have run when a round ends, and a call with nothing in flight
 * still commits the tree's repaint. */
async function settle(client: QueryClient): Promise<void> {
  do await act(() => idle(client));
  while (busy(client));
}

/** Renders `routes` at `url` with a fresh query client, then settles it. */
export async function renderPage(url: string, routes: ReactNode) {
  const client = new QueryClient({
    // A retry waits for a backoff timer.
    defaultOptions: { queries: { retry: false } },
  });
  const view = render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[url]}>
        <Routes>{routes}</Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
  await settle(client);
  return view;
}
