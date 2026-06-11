import { useEffect, useState } from "react";

export default function App() {
  const [health, setHealth] = useState<string>("connecting…");

  useEffect(() => {
    fetch("/api/health")
      .then((r) => r.json())
      .then((d) => setHealth(d.status))
      .catch(() => setHealth("backend unreachable"));
  }, []);

  return (
    <main>
      <h1>nit</h1>
      <p>backend: {health}</p>
    </main>
  );
}
