import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import { add, ctx, del } from "../../api/fixtures/builders";
import DocumentDiff from "./DocumentDiff";

const queryClient = new QueryClient({
  defaultOptions: { queries: { staleTime: Infinity } },
});

const INTRO = [
  "Three changes add rotation of refresh tokens.",
  "",
  "Read the schema first.",
  "",
  "Then read the rotation.",
  "",
  "The documentation is the last change.",
  "",
];
const END = INTRO.length;

const LINES = [
  ...INTRO.map((text, i) => ctx(i + 1, i + 1, text)),
  ctx(END + 1, END + 1, "## Reuse revokes the family"),
  ctx(END + 2, END + 2, ""),
  del(END + 3, "`rotate` looks up the presented token."),
  add(
    END + 3,
    "`rotate` looks up the presented token. A token that was rotated before is a sign of theft, so the store revokes the **whole family**.",
  ),
  ctx(END + 4, END + 4, ""),
  del(END + 5, "[change 10](/changes/10)"),
  add(END + 5, "[change 11](/changes/11)"),
  ctx(END + 6, END + 6, ""),
  ctx(END + 7, END + 7, "- The lookup returns an error for an unknown token."),
  del(END + 8, "- The caller receives the old token."),
  add(END + 8, "- The caller receives the successor."),
  add(END + 9, ""),
  add(END + 10, "The second use of one token fails."),
];

export const Changed = () => (
  <MemoryRouter>
    <QueryClientProvider client={queryClient}>
      <div className="document-page" style={{ width: 1068 }}>
        <DocumentDiff lines={LINES} />
      </div>
    </QueryClientProvider>
  </MemoryRouter>
);
