import { Outlet } from "react-router-dom";
import { SidebarLayout } from "./components/AppSidebar";

export default function App() {
  return (
    <SidebarLayout>
      <Outlet />
    </SidebarLayout>
  );
}
