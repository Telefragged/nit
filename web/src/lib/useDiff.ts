import { useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import { getDiff } from "../api/client";
import type { ChangeDetail, ChangeVersion, Whitespace } from "../api/types";
import { attachmentsOf, reviewFiles } from "./attachments";
import { treeOrder } from "./diffview";
import { revisionVersion } from "./version";

/** The diff of a change from `against` to revision `selected`.
 *
 * `files` is its files in the order of the file tree, each attachment
 * file with its attachment. `attachKey` is the hashes of the attachments
 * that the two versions show. The diff includes those attachments, so an
 * attach entry from the stream changes the key and refetches the diff.
 * The old diff stays until the new one arrives. The read waits for
 * `published`. */
export function useDiff(
  published: ChangeDetail | undefined,
  changeNumber: number,
  selected: number,
  against: ChangeVersion,
  whitespace: Whitespace,
) {
  const shown = attachmentsOf(published, revisionVersion(selected));
  const attachKey = [...attachmentsOf(published, against), ...shown]
    .map((a) => a.hash)
    .join(",");
  const diffView = [changeNumber, selected, against, whitespace];
  const diff = useQuery({
    queryKey: ["diff", ...diffView, attachKey],
    queryFn: () => getDiff(changeNumber, selected, against, whitespace),
    enabled: published !== undefined,
    retry: false,
    placeholderData: (previous, query) =>
      diffView.every((part, i) => query?.queryKey[i + 1] === part)
        ? previous
        : undefined,
  });
  const files = useMemo(
    () => treeOrder(reviewFiles(diff.data?.files ?? [], shown)),
    [diff.data, shown],
  );
  return { diff, files, attachKey };
}
