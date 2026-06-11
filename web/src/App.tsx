import { Link, Outlet } from "react-router-dom";

export default function App() {
  return (
    <>
      <header className="topbar">
        <Link to="/" className="logo">
          nit
        </Link>
        <span className="spacer" />
        {import.meta.env.VITE_MOCK ? (
          <span className="mock-flag" title="Serving canned fixtures — no backend">
            MOCK
          </span>
        ) : null}
      </header>
      <Outlet />
    </>
  );
}
