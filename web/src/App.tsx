import { Link, Outlet } from "react-router-dom";
import About from "./components/About";
import { SidebarLayout } from "./components/AppSidebar";

export default function App() {
  return (
    <SidebarLayout>
      <header className="topbar">
        <Link to="/" className="logo" aria-label="nit" title="nit">
          <img src="/favicon.svg" alt="" width={20} height={20} />
        </Link>
        <span className="spacer" />
        {import.meta.env.VITE_MOCK ? (
          <span
            className="mock-flag"
            title="The page shows fixtures, and no server is behind it"
          >
            Mock data
          </span>
        ) : null}
        <About />
      </header>
      <Outlet />
    </SidebarLayout>
  );
}
