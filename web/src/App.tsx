import { Suspense } from "react";
import { Link, Outlet } from "react-router-dom";
import About from "./components/About";

export default function App() {
  return (
    <>
      <header className="topbar">
        <Link to="/" className="logo">
          nit
        </Link>
        <span className="spacer" />
        {import.meta.env.VITE_MOCK ? (
          <span
            className="mock-flag"
            title="Serving canned fixtures — no backend"
          >
            MOCK
          </span>
        ) : null}
        <About />
      </header>
      <Suspense fallback={null}>
        <Outlet />
      </Suspense>
    </>
  );
}
