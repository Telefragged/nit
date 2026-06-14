// jsdom does no layout, so scrollHeight/offset/clientHeight are 0 there. We
// stub them on the textarea prototype to drive the resize math and assert
// the inline height the hook computes — on mount (the edit case, opened with
// text) and on a value change (a newline typed).

import { cleanup, render, screen } from "@testing-library/react";
import { useRef } from "react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { useAutosize } from "./useAutosize";

const metrics = { scrollHeight: 0, offsetHeight: 0, clientHeight: 0 };
const keys = ["scrollHeight", "offsetHeight", "clientHeight"] as const;

beforeEach(() => {
  for (const key of keys) {
    Object.defineProperty(HTMLTextAreaElement.prototype, key, {
      configurable: true,
      get: () => metrics[key],
    });
  }
});
afterEach(() => {
  cleanup();
  for (const key of keys) delete HTMLTextAreaElement.prototype[key];
});

function Harness({ value }: { value: string }) {
  const ref = useRef<HTMLTextAreaElement>(null);
  useAutosize(ref, value);
  return <textarea ref={ref} value={value} readOnly />;
}

describe("useAutosize", () => {
  it("fits existing text on mount, adding the border-box border back", () => {
    // 6px border (offset − client) on top of the 120px scrolled content.
    Object.assign(metrics, {
      scrollHeight: 120,
      offsetHeight: 50,
      clientHeight: 44,
    });
    render(<Harness value="line one\nline two" />);
    expect(screen.getByRole("textbox").style.height).toBe("126px");
  });

  it("grows when the value gains a line", () => {
    Object.assign(metrics, {
      scrollHeight: 120,
      offsetHeight: 50,
      clientHeight: 44,
    });
    const { rerender } = render(<Harness value="one line" />);
    Object.assign(metrics, { scrollHeight: 200 });
    rerender(<Harness value="one line\nplus another" />);
    expect(screen.getByRole("textbox").style.height).toBe("206px");
  });
});
