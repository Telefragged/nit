import { useQuery } from "@tanstack/react-query";
import { useParams } from "react-router-dom";
import { getChange } from "../api/client";
import { ErrorPanel } from "./NotFound";

// Placeholder — the full review surface (diff, threads, review bar) lands in
// follow-up commits.
export default function ReviewPage() {
  const { id } = useParams();
  const changeId = Number(id);
  const query = useQuery({
    queryKey: ["change", changeId],
    queryFn: () => getChange(changeId),
  });

  if (query.isError) {
    return (
      <main className="page">
        <ErrorPanel error={query.error} />
      </main>
    );
  }
  if (query.isPending) {
    return (
      <main className="page">
        <div className="skeleton" style={{ width: 280, height: 18 }} />
      </main>
    );
  }
  return (
    <main className="page">
      <h1>{query.data.subject}</h1>
      <p className="subtitle">review surface under construction</p>
    </main>
  );
}
