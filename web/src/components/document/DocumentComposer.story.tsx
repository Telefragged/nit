import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import DocumentComposer from "./DocumentComposer";

const queryClient = new QueryClient();

export const Default = () => (
  <QueryClientProvider client={queryClient}>
    <div className="document-page" style={{ width: 1068 }}>
      <DocumentComposer onAppend={() => undefined} />
    </div>
  </QueryClientProvider>
);
