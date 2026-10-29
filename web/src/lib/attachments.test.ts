import { describe, expect, it } from "vitest";
import { addedFile } from "../api/fixtures/builders";
import { reviewFiles } from "./attachments";

describe("reviewFiles", () => {
  const plan = { name: "plan.md", hash: "a".repeat(64), revision: 0 };

  it("joins an attachment file to the attachment of its name", () => {
    const [file] = reviewFiles(
      [addedFile("/attachments/plan.md", "# Plan")],
      [plan],
    );
    expect(file?.attachment).toEqual(plan);
  });

  it("joins no attachment to a file of the commit", () => {
    const [file] = reviewFiles([addedFile("plan.md", "# Plan")], [plan]);
    expect(file?.attachment).toBeUndefined();
  });
});
