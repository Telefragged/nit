import { Link } from "react-router-dom";

export default function NotFound() {
  return (
    <main className="page">
      <h1>Not found</h1>
      <p className="subtitle">
        Nothing lives at this address.{" "}
        <Link to="/">Back to the repositories</Link>.
      </p>
    </main>
  );
}

export function ErrorPanel({ error }: { error: unknown }) {
  const message = error instanceof Error ? error.message : String(error);
  return (
    <div className="error-panel">
      <div className="title">Request failed</div>
      <div>{message}</div>
    </div>
  );
}
