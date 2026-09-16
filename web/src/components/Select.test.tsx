import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import Select from "./Select";

afterEach(cleanup);

const options = [
  { value: "base", label: "Base" },
  { value: "0", label: "r0", detail: "6 comments" },
  { value: "1", label: "r1", detail: "3 comments", disabled: true },
];

const renderSelect = (onChange = vi.fn(), value = "base", rows = options) => {
  render(
    <Select
      label="Diff base"
      placeholder="nothing to pick"
      value={value}
      onChange={onChange}
      options={rows}
    />,
  );
  return screen.getByLabelText<HTMLButtonElement>("Diff base");
};

describe("Select", () => {
  it("names the chosen option in the head and the rest in the list", () => {
    const head = renderSelect(vi.fn(), "0");
    expect(head.textContent).toBe("r0 6 comments");
    expect(screen.queryByRole("listbox")).toBeNull();

    fireEvent.click(head);
    expect(screen.getAllByRole("option").map((o) => o.textContent)).toEqual([
      "Base",
      "r0 6 comments",
      "r1 3 comments",
    ]);
    expect(
      screen.getByRole("option", { name: "r0 6 comments" }).ariaSelected,
    ).toBe("true");
  });

  it("reports the chosen option and closes", () => {
    const onChange = vi.fn();
    const head = renderSelect(onChange);
    fireEvent.click(head);
    fireEvent.click(screen.getByRole("option", { name: "r0 6 comments" }));
    expect(onChange).toHaveBeenCalledWith("0");
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("offers a disabled option without letting it be chosen", () => {
    const onChange = vi.fn();
    fireEvent.click(renderSelect(onChange));
    const option = screen.getByRole<HTMLButtonElement>("option", {
      name: "r1 3 comments",
    });
    expect(option.disabled).toBe(true);
    fireEvent.click(option);
    expect(onChange).not.toHaveBeenCalled();
    expect(screen.getByRole("listbox")).not.toBeNull();
  });

  it("closes on a second press of the head", () => {
    const head = renderSelect();
    fireEvent.click(head);
    expect(screen.getByRole("listbox")).not.toBeNull();
    fireEvent.click(head);
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("closes on Escape", () => {
    const head = renderSelect();
    fireEvent.click(head);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("falls back to the placeholder when the value names no option", () => {
    const head = renderSelect(vi.fn(), "nine");
    expect(head.textContent).toBe("nothing to pick");
    // Still openable: the reviewer has to be able to pick their way out.
    expect(head.disabled).toBe(false);
  });

  it("disables itself when there is nothing to pick", () => {
    render(
      <Select
        label="Tag"
        placeholder="no tags"
        value=""
        onChange={vi.fn()}
        options={[]}
      />,
    );
    const head = screen.getByLabelText<HTMLButtonElement>("Tag");
    expect(head.disabled).toBe(true);
    expect(head.textContent).toBe("no tags");
  });
});
