import * as React from "react";
import { mergeProps } from "@base-ui/react/merge-props";
import { useRender } from "@base-ui/react/use-render";
import { cn } from "@/lib/utils";

import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";

const SIDEBAR_MIN_WIDTH_PX = 256;
const SIDEBAR_MAX_WIDTH_PX = 320;
const SIDEBAR_ICON_WIDTH_PX = 48;
const SNAP_DISTANCE_PX = 48;

interface SidebarContextProps {
  open: boolean;
  setOpen: (open: boolean) => void;
}

const SidebarContext = React.createContext<SidebarContextProps | null>(null);

function useSidebar() {
  const context = React.useContext(SidebarContext);
  if (!context) {
    throw new Error("useSidebar must be used within a SidebarProvider.");
  }

  return context;
}

function SidebarProvider({
  className,
  style,
  children,
  ...props
}: React.ComponentProps<"div">) {
  const [open, setOpen] = React.useState(true);
  const contextValue = React.useMemo(() => ({ open, setOpen }), [open]);

  return (
    <SidebarContext.Provider value={contextValue}>
      <TooltipProvider>
        <div
          data-slot="sidebar-wrapper"
          style={
            {
              "--sidebar-width": `${SIDEBAR_MIN_WIDTH_PX}px`,
              "--sidebar-width-icon": `${SIDEBAR_ICON_WIDTH_PX}px`,
              ...style,
            } as React.CSSProperties
          }
          className={cn(
            "group/sidebar-wrapper flex min-h-svh w-full",
            className,
          )}
          {...props}
        >
          {children}
        </div>
      </TooltipProvider>
    </SidebarContext.Provider>
  );
}

function Sidebar({
  className,
  children,
  ...props
}: React.ComponentProps<"div">) {
  const { open } = useSidebar();

  return (
    <div
      className="group peer text-sidebar-foreground"
      data-state={open ? "expanded" : "collapsed"}
      data-collapsible={open ? "" : "icon"}
      data-slot="sidebar"
    >
      <SidebarGap />
      <div
        data-slot="sidebar-container"
        className={cn(
          "fixed inset-y-0 left-0 z-10 flex h-svh w-(--sidebar-width) border-r border-sidebar-border transition-[width] duration-200 ease-linear group-data-[collapsible=icon]:w-(--sidebar-width-icon) group-data-resizing/sidebar-wrapper:transition-none",
          className,
        )}
        {...props}
      >
        <div
          data-sidebar="sidebar"
          data-slot="sidebar-inner"
          className="flex size-full flex-col bg-sidebar"
        >
          {children}
        </div>
      </div>
    </div>
  );
}

function SidebarGap() {
  return (
    <div
      data-slot="sidebar-gap"
      className="relative w-(--sidebar-width) bg-transparent transition-[width] duration-200 ease-linear group-data-[collapsible=icon]:w-(--sidebar-width-icon) group-data-resizing/sidebar-wrapper:transition-none"
    />
  );
}

function useResizeDrag() {
  const { open, setOpen } = useSidebar();

  return (down: React.PointerEvent<HTMLButtonElement>) => {
    down.preventDefault();
    const rail = down.currentTarget;
    const wrapper = rail.closest("[data-slot=sidebar-wrapper]");
    if (!(wrapper instanceof HTMLElement)) return;
    rail.setPointerCapture(down.pointerId);
    const switchThresholdForThisDrag = open
      ? SIDEBAR_MIN_WIDTH_PX - SNAP_DISTANCE_PX
      : SIDEBAR_ICON_WIDTH_PX + SNAP_DISTANCE_PX;
    let shown = open;
    let dragged = false;
    const followPointer = () => {
      wrapper.dataset.resizing = "";
    };
    const move = (e: PointerEvent) => {
      if (!dragged && Math.abs(e.clientX - down.clientX) > 3) {
        dragged = true;
        followPointer();
      }
      if (!dragged) return;
      if (e.clientX > switchThresholdForThisDrag !== shown) {
        shown = !shown;
        setOpen(shown);
      }
      if (shown) {
        const width = Math.min(
          SIDEBAR_MAX_WIDTH_PX,
          Math.max(SIDEBAR_MIN_WIDTH_PX, e.clientX),
        );
        wrapper.style.setProperty("--sidebar-width", `${width}px`);
      }
    };
    const up = () => {
      if (!dragged) setOpen(!open);
    };
    const end = () => {
      rail.removeEventListener("pointermove", move);
      rail.removeEventListener("pointerup", up);
      rail.removeEventListener("lostpointercapture", end);
      delete wrapper.dataset.resizing;
    };
    rail.addEventListener("pointermove", move);
    rail.addEventListener("pointerup", up);
    rail.addEventListener("lostpointercapture", end);
  };
}

function SidebarRail({ className, ...props }: React.ComponentProps<"button">) {
  const onPointerDown = useResizeDrag();

  return (
    <button
      data-sidebar="rail"
      data-slot="sidebar-rail"
      aria-label="Resize sidebar"
      title="Drag to resize, click to toggle"
      tabIndex={-1}
      className={cn(
        "absolute inset-y-0 -right-4 z-20 flex w-4 -translate-x-1/2 cursor-col-resize after:absolute after:inset-y-0 after:start-1/2 after:w-[2px] hover:after:bg-border-strong",
        className,
      )}
      onPointerDown={onPointerDown}
      {...props}
    />
  );
}

function SidebarContent({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="sidebar-content"
      data-sidebar="content"
      className={cn(
        "no-scrollbar flex min-h-0 flex-1 flex-col gap-0 overflow-auto group-data-[collapsible=icon]:overflow-hidden",
        className,
      )}
      {...props}
    />
  );
}

function SidebarGroup({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="sidebar-group"
      data-sidebar="group"
      className={cn("relative flex w-full min-w-0 flex-col p-2", className)}
      {...props}
    />
  );
}

function SidebarGroupContent({
  className,
  ...props
}: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="sidebar-group-content"
      data-sidebar="group-content"
      className={cn("w-full text-sm", className)}
      {...props}
    />
  );
}

function SidebarMenu({ className, ...props }: React.ComponentProps<"ul">) {
  return (
    <ul
      data-slot="sidebar-menu"
      data-sidebar="menu"
      className={cn("flex w-full min-w-0 flex-col gap-0", className)}
      {...props}
    />
  );
}

function SidebarMenuItem({ className, ...props }: React.ComponentProps<"li">) {
  return (
    <li
      data-slot="sidebar-menu-item"
      data-sidebar="menu-item"
      className={cn("group/menu-item relative", className)}
      {...props}
    />
  );
}

function SidebarMenuButton({
  render,
  isActive = false,
  collapsedTooltip,
  className,
  ...props
}: useRender.ComponentProps<"button"> &
  React.ComponentProps<"button"> & {
    isActive?: boolean;
    collapsedTooltip?: string;
  }) {
  const { open } = useSidebar();
  const comp = useRender({
    defaultTagName: "button",
    props: mergeProps<"button">(
      {
        className: cn(
          "peer/menu-button group/menu-button flex h-8 w-full items-center gap-2 overflow-hidden rounded-md p-2 text-left text-sm ring-sidebar-ring outline-hidden transition-[width,height,padding] group-data-[collapsible=icon]:size-8! group-data-[collapsible=icon]:p-2! hover:bg-sidebar-accent hover:text-sidebar-accent-foreground focus-visible:ring-2 active:bg-sidebar-accent active:text-sidebar-accent-foreground disabled:pointer-events-none disabled:opacity-50 aria-disabled:pointer-events-none aria-disabled:opacity-50 data-open:hover:bg-sidebar-accent data-open:hover:text-sidebar-accent-foreground data-active:bg-sidebar-accent data-active:font-medium data-active:text-sidebar-accent-foreground [&_svg]:size-4 [&_svg]:shrink-0 [&>span:last-child]:truncate",
          className,
        ),
      },
      props,
    ),
    render: !collapsedTooltip ? render : <TooltipTrigger render={render} />,
    state: {
      slot: "sidebar-menu-button",
      sidebar: "menu-button",
      active: isActive,
    },
  });

  if (!collapsedTooltip) {
    return comp;
  }

  return (
    <Tooltip>
      {comp}
      <TooltipContent side="right" align="center" hidden={open}>
        {collapsedTooltip}
      </TooltipContent>
    </Tooltip>
  );
}

function SidebarMenuSub({ className, ...props }: React.ComponentProps<"ul">) {
  return (
    <ul
      data-slot="sidebar-menu-sub"
      data-sidebar="menu-sub"
      className={cn(
        "mx-3.5 flex min-w-0 translate-x-px flex-col gap-1 border-l border-sidebar-border px-2.5 py-0.5 group-data-[collapsible=icon]:hidden",
        className,
      )}
      {...props}
    />
  );
}

function SidebarMenuSubItem({
  className,
  ...props
}: React.ComponentProps<"li">) {
  return (
    <li
      data-slot="sidebar-menu-sub-item"
      data-sidebar="menu-sub-item"
      className={cn("group/menu-sub-item relative", className)}
      {...props}
    />
  );
}

function SidebarMenuSubButton({
  render,
  isActive = false,
  className,
  ...props
}: useRender.ComponentProps<"a"> &
  React.ComponentProps<"a"> & {
    isActive?: boolean;
  }) {
  return useRender({
    defaultTagName: "a",
    props: mergeProps<"a">(
      {
        className: cn(
          "flex h-7 min-w-0 -translate-x-px items-center gap-2 overflow-hidden rounded-md px-2 text-sm text-sidebar-foreground ring-sidebar-ring outline-hidden group-data-[collapsible=icon]:hidden hover:bg-sidebar-accent hover:text-sidebar-accent-foreground hover:no-underline focus-visible:ring-2 active:bg-sidebar-accent active:text-sidebar-accent-foreground disabled:pointer-events-none disabled:opacity-50 aria-disabled:pointer-events-none aria-disabled:opacity-50 data-active:bg-sidebar-accent data-active:text-sidebar-accent-foreground [&>span:last-child]:truncate [&>svg]:size-4 [&>svg]:shrink-0 [&>svg]:text-sidebar-accent-foreground",
          className,
        ),
      },
      props,
    ),
    render,
    state: {
      slot: "sidebar-menu-sub-button",
      sidebar: "menu-sub-button",
      active: isActive,
    },
  });
}

export {
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
};
