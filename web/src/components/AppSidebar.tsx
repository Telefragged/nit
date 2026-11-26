import type { ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";
import { NavLink, useParams } from "react-router-dom";
import { ChevronRight, FolderGit2 } from "lucide-react";
import {
  Sidebar,
  SidebarContent,
  SidebarGroup,
  SidebarGroupContent,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarMenuSub,
  SidebarMenuSubButton,
  SidebarMenuSubItem,
  SidebarProvider,
  SidebarRail,
} from "@/components/ui/sidebar";
import {
  Collapsible,
  CollapsiblePanel,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { listRepos } from "../api/client";
import { repoPath } from "../lib/repo";
import { ErrorPanel } from "../pages/NotFound";

export function SidebarLayout({ children }: { children: ReactNode }) {
  return (
    <SidebarProvider>
      <AppSidebar />
      <div className="flex min-w-0 flex-1 flex-col">{children}</div>
    </SidebarProvider>
  );
}

function AppSidebar() {
  const repoId = Number(useParams().repoId);
  const query = useQuery({ queryKey: ["repos"], queryFn: listRepos });
  return (
    <Sidebar>
      <SidebarContent>
        <SidebarGroup aria-busy={query.isLoading}>
          <SidebarGroupContent>
            <SidebarMenu>
              <Collapsible defaultOpen render={<SidebarMenuItem />}>
                <SidebarMenuButton
                  collapsedTooltip="Repositories"
                  render={<CollapsibleTrigger />}
                >
                  <FolderGit2 />
                  <span>Repositories</span>
                  <ChevronRight className="ml-auto transition-transform in-data-panel-open:rotate-90" />
                </SidebarMenuButton>
                <CollapsiblePanel>
                  {query.isError ? (
                    <ErrorPanel error={query.error} />
                  ) : query.data?.repos.length === 0 ? (
                    <div className="px-2 text-sm text-muted-foreground group-data-[collapsible=icon]:hidden">
                      No repositories. Run <code>nit push</code> from a repo to
                      register one.
                    </div>
                  ) : (
                    <SidebarMenuSub>
                      {query.data?.repos.map((repo) => {
                        const path = repoPath(repo.git_dir);
                        return (
                          <SidebarMenuSubItem key={repo.id}>
                            <SidebarMenuSubButton
                              isActive={repo.id === repoId}
                              title={path}
                              render={<NavLink to={`/repos/${repo.id}`} />}
                            >
                              <span className="min-w-0 flex-1 truncate">
                                {path.slice(path.lastIndexOf("/") + 1)}
                              </span>
                              {repo.open_changes > 0 ? (
                                <span
                                  className="text-xs text-muted-foreground tabular-nums"
                                  title={`${repo.open_changes} open ${repo.open_changes === 1 ? "change" : "changes"}`}
                                >
                                  {repo.open_changes}
                                </span>
                              ) : null}
                            </SidebarMenuSubButton>
                          </SidebarMenuSubItem>
                        );
                      })}
                    </SidebarMenuSub>
                  )}
                </CollapsiblePanel>
              </Collapsible>
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>
      <SidebarRail />
    </Sidebar>
  );
}
