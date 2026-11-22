import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import {
  reviewCtx,
  threadComment,
  uiThread,
} from "../../api/fixtures/builders";
import { ReviewContext } from "../../pages/reviewContext";
import CommentThread from "../CommentThread";
import FloatingPanel from "./FloatingPanel";

const queryClient = new QueryClient();
const ctx = reviewCtx({ selected: 1, latestRevision: 1 });

export function Thread() {
  return (
    <MemoryRouter>
      <QueryClientProvider client={queryClient}>
        <ReviewContext.Provider value={ctx}>
          <div style={{ width: 440 }}>
            <FloatingPanel
              label="region 230,180 112x42"
              onClose={() => undefined}
            >
              <div className="meta-item">
                <CommentThread
                  thread={uiThread({
                    id: 3,
                    anchor: {
                      file: {
                        version: "r1",
                        path: "/attachments/sign-in.png",
                        anchor: {
                          region: {
                            rect: { x: 230, y: 180, width: 112, height: 42 },
                          },
                        },
                      },
                    },
                    comments: [
                      threadComment(
                        30,
                        "The button has the color of a link. Use the primary button.",
                      ),
                    ],
                  })}
                />
              </div>
            </FloatingPanel>
          </div>
        </ReviewContext.Provider>
      </QueryClientProvider>
    </MemoryRouter>
  );
}
