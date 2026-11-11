import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import CommentEditor from "./CommentEditor";

afterEach(cleanup);

const renderEditor = (props: Partial<Parameters<typeof CommentEditor>[0]>) => {
  const onSave = vi.fn();
  const onCancel = vi.fn();
  render(
    <CommentEditor
      saving={false}
      onSave={onSave}
      onCancel={onCancel}
      {...props}
    />,
  );
  return { onSave, onCancel, text: screen.getByRole("textbox") };
};

const save = () =>
  screen.getByRole<HTMLButtonElement>("button", { name: "Save draft" });

describe("CommentEditor", () => {
  it("has the focus when it opens, and saves nothing while it is empty", () => {
    const { onSave, text } = renderEditor({});
    expect(document.activeElement).toBe(text);
    expect(save().disabled).toBe(true);
    fireEvent.keyDown(text, { key: "Enter", ctrlKey: true });
    expect(onSave).not.toHaveBeenCalled();
  });

  it("saves the trimmed text on Ctrl+Enter and on the button", () => {
    const { onSave, text } = renderEditor({});
    fireEvent.change(text, { target: { value: "  a note \n" } });
    fireEvent.keyDown(text, { key: "Enter", ctrlKey: true });
    fireEvent.click(save());
    expect(onSave.mock.calls).toEqual([
      ["a note", undefined],
      ["a note", undefined],
    ]);
  });

  it("asks before Escape discards typed text", () => {
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
    const { onCancel, text } = renderEditor({});
    fireEvent.change(text, { target: { value: "a note" } });
    fireEvent.keyDown(text, { key: "Escape" });
    expect(confirm).toHaveBeenCalledOnce();
    expect(onCancel).not.toHaveBeenCalled();

    confirm.mockReturnValue(true);
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onCancel).toHaveBeenCalledOnce();
    confirm.mockRestore();
  });

  it("closes on Escape without a question when no text was typed", () => {
    const confirm = vi.spyOn(window, "confirm");
    const { onCancel, text } = renderEditor({ initial: "a note" });
    fireEvent.keyDown(text, { key: "Escape" });
    expect(confirm).not.toHaveBeenCalled();
    expect(onCancel).toHaveBeenCalledOnce();
    confirm.mockRestore();
  });

  it("saves a change of the Resolved box alone", () => {
    const { onSave } = renderEditor({ initialResolved: false });
    expect(save().disabled).toBe(true);
    fireEvent.click(screen.getByRole("checkbox", { name: "Resolved" }));
    fireEvent.click(save());
    expect(onSave).toHaveBeenCalledWith("", true);
  });
});
