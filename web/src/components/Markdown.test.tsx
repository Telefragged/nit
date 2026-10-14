import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter, useLocation } from "react-router-dom";
import { afterEach, describe, expect, it } from "vitest";
import Markdown from "./Markdown";

afterEach(cleanup);

describe("a link in a comment", () => {
  let path = "";
  function LocationProbe() {
    const loc = useLocation();
    path = loc.pathname + loc.search;
    return null;
  }
  const renderBody = (text: string) =>
    render(
      <MemoryRouter initialEntries={["/changes/1"]}>
        <Markdown text={text} />
        <LocationProbe />
      </MemoryRouter>,
    );

  it("moves this page when it points into nit", () => {
    const href = `${window.location.origin}/changes/2?file=a.rs&line=3`;
    renderBody(`[here](${href})`);
    const link = screen.getByRole("link", { name: "here" });

    expect(link.getAttribute("target")).toBeNull();
    fireEvent.click(link);
    expect(path).toBe("/changes/2?file=a.rs&line=3");
  });

  it("opens a new tab when it points elsewhere", () => {
    renderBody("[docs](https://example.com/page)");
    const link = screen.getByRole("link", { name: "docs" });

    expect(link.getAttribute("target")).toBe("_blank");
    expect(link.getAttribute("href")).toBe("https://example.com/page");
  });
});
