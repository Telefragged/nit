import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it } from "vitest";
import { user } from "../test/page";
import ReviewSettingsMenu from "./ReviewSettings";
import { DEFAULTS, type ReviewSettings } from "../lib/reviewSettings";

afterEach(cleanup);

/** Stands in for the page: the settings are its state, and `changes` gets
 * each change. */
function Harness({ changes }: { changes: ReviewSettings[] }) {
  const [settings, setSettings] = useState(DEFAULTS);
  return (
    <ReviewSettingsMenu
      settings={settings}
      onChange={(next) => {
        changes.push(next);
        setSettings(next);
      }}
    />
  );
}

const open = () => {
  const changes: ReviewSettings[] = [];
  render(<Harness changes={changes} />);
  fireEvent.click(screen.getByRole("button", { name: "Settings" }));
  return changes;
};

const pressed = (name: string) =>
  screen.getByRole("button", { name }).getAttribute("aria-pressed");

describe("the review settings popover", () => {
  it("changes a setting at once and stays open", () => {
    const changes = open();
    expect(pressed("Unified")).toBe("true");

    fireEvent.click(screen.getByRole("button", { name: "Side-by-side" }));
    fireEvent.click(screen.getByRole("button", { name: "Ignore" }));
    expect(changes).toEqual([
      { layout: "split", ported: "open", whitespace: "compare" },
      { layout: "split", ported: "open", whitespace: "ignore" },
    ]);
    expect(pressed("Side-by-side")).toBe("true");
    expect(
      screen.queryByRole("dialog", { name: "Review settings" }),
    ).not.toBeNull();
  });

  it("closes on Escape", async () => {
    open();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});
