import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import ReviewSettingsMenu from "./ReviewSettings";
import type { ReviewSettings } from "../lib/reviewSettings";

afterEach(cleanup);

/** The page's half of the contract: it holds the applied settings. */
function Harness({ applied }: { applied: ReviewSettings[] }) {
  const [settings, setSettings] = useState<ReviewSettings>({
    mode: "full",
    layout: "unified",
    ported: "open",
    whitespace: "compare",
  });
  return (
    <ReviewSettingsMenu
      settings={settings}
      onApply={(next) => {
        applied.push(next);
        setSettings(next);
      }}
    />
  );
}

/** Opens the popup and picks side-by-side, leaving the draft dirty. */
function openAndChange(applied: ReviewSettings[] = []) {
  render(<Harness applied={applied} />);
  fireEvent.click(screen.getByRole("button", { name: "Settings" }));
  fireEvent.click(screen.getByRole("button", { name: "Side-by-side" }));
  return applied;
}

const backdrop = () => screen.getByRole("dialog", { name: "Settings" });

describe("the review settings popup", () => {
  it("applies the draft and closes", () => {
    const applied = openAndChange();
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    expect(applied).toEqual([
      { mode: "full", layout: "split", ported: "open", whitespace: "compare" },
    ]);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("applies the whitespace knob", () => {
    const applied: ReviewSettings[] = [];
    render(<Harness applied={applied} />);
    fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    fireEvent.click(screen.getByRole("button", { name: "Ignore" }));
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    expect(applied).toEqual([
      { mode: "full", layout: "unified", ported: "open", whitespace: "ignore" },
    ]);
  });

  it("drops the draft on Cancel without asking", () => {
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    const applied = openAndChange();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(confirm).not.toHaveBeenCalled();
    expect(applied).toEqual([]);
    expect(screen.queryByRole("dialog")).toBeNull();

    // The next opening starts from the applied settings, not the dropped draft.
    fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    expect(screen.getByRole("button", { name: "Unified" }).className).toBe(
      "active",
    );
    confirm.mockRestore();
  });

  it("warns before a backdrop press drops changed settings", () => {
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
    openAndChange();
    fireEvent.mouseDown(backdrop());
    expect(confirm).toHaveBeenCalledOnce();
    expect(screen.queryByRole("dialog")).not.toBeNull();

    confirm.mockReturnValue(true);
    fireEvent.mouseDown(backdrop());
    expect(screen.queryByRole("dialog")).toBeNull();
    confirm.mockRestore();
  });

  it("closes on a backdrop press without asking when nothing changed", () => {
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    render(<Harness applied={[]} />);
    fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    fireEvent.mouseDown(backdrop());
    expect(confirm).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).toBeNull();
    confirm.mockRestore();
  });
});
