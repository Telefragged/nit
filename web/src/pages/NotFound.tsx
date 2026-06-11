import { Link } from "react-router-dom";

export default function NotFound() {
  return (
    <main className="page">
      <h1>Not found</h1>
      <p className="subtitle">
        Nothing lives at this address. <Link to="/">Back to the dashboard</Link>.
      </p>
    </main>
  );
}

/** Shared error panel for failed queries. */
export function ErrorPanel({ error }: { error: unknown }) {
  const message = error instanceof Error ? error.message : String(error);
  return (
    <div className="error-panel">
      <div className="title">Request failed</div>
      <div>{message}</div>
      <div style={{ marginTop: 6 }}>
        <Link to="/">Back to the dashboard</Link>
      </div>
    </div>
  );
}
