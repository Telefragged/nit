import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useDiffLayout } from "./reviewSettings";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

function fakeWindow(wide: boolean) {
  const events = new EventTarget();
  vi.spyOn(window, "matchMedia").mockImplementation(
    (media) =>
      Object.assign(events, {
        media,
        matches: wide,
      }) as unknown as MediaQueryList,
  );
  return (next: boolean) => {
    wide = next;
    act(() => {
      events.dispatchEvent(new Event("change"));
    });
  };
}

describe("useDiffLayout", () => {
  it("returns a fixed layout whatever the window width", () => {
    fakeWindow(true);
    expect(renderHook(() => useDiffLayout("unified")).result.current).toBe(
      "unified",
    );
    fakeWindow(false);
    expect(renderHook(() => useDiffLayout("split")).result.current).toBe(
      "split",
    );
  });

  it("follows the window width for auto", () => {
    const resize = fakeWindow(true);
    const { result } = renderHook(() => useDiffLayout("auto"));
    expect(result.current).toBe("split");

    resize(false);
    expect(result.current).toBe("unified");
  });
});
