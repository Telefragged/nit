---
name: document
description: Write and answer a nit document, which is narration that embeds live references to changes, which reviewers edit in the browser. Use to explain a chain of changes for review, to propose work before the changes exist, or when the watch delivers a document that a reviewer sent.
---

# nit:document: narrate changes for reviewers

A document is one Markdown text with live references to changes.
Reviewers open it in the browser, see each referenced diff and
attachment in place, and comment on the code there. A thread on an
embedded diff is an ordinary thread on the change.

Write a document when reviewers need the intent of a chain before its
lines: what the changes do together, in which order to look at
them, and which decisions need a second opinion. Put only what
reviewers must look at. Reference the code instead of a copy of it.

## Create and print

```sh
nit doc new --title "Refresh token rotation" -F narrative.md   # prints the number and the URL
nit doc show 3 > narrative.md                                  # the text on stdout, the version on stderr
nit doc list
```

Give reviewers the URL that `nit doc new` prints.

## References

The text is ordinary Markdown. A link on a line of its own, with an
empty line before it and after it, is a reference: the page shows the
part of the change there. The same link inside a sentence stays a link.

```markdown
## Reuse revokes the family

`rotate` revokes every token of a family on reuse.

[rotate.rs](/changes/412?revision=3&file=src/auth/rotate.rs&line=40-72)

The graph before and after:

[graph.png](/changes/412?revision=2&file=/attachments/graph.png)

[graph.png](/changes/412?revision=3&file=/attachments/graph.png)

[change 412](/changes/412)
```

| Address                            | Shows                                                |
| ---------------------------------- | ---------------------------------------------------- |
| `/changes/N`                       | A card with the subject and the status of the change |
| `/changes/N?revision=R&file=F`     | One file or one attachment of the change             |
| `...&line=A-B`, and `...&side=old` | Only those lines of the file, which count from 1     |
| `...&against=rM`                   | The interdiff from revision M                        |
| `/api/attachments/<hash>/<name>`   | A file that a reviewer uploaded to the document      |

The address is the address of the review page, so you can copy one from
a URL that nit prints. References always show revision R. Use that to
compare an earlier revision with a later one.

## What references can show

References with `line` show one item of a file, such as one type or one function, so reviewers do not search a whole file. An attachment of a change can be any file: the `--help` output of a command, an example of its output, a screenshot, an HTML page. Attach it with `nit attach`, and reference it as a file of the change.

```sh
nit doc --help > doc-help.txt
nit attach --change 412 --name cli/doc-help.txt doc-help.txt
```

```markdown
[nit doc --help](/changes/412?revision=3&file=/attachments/cli/doc-help.txt)
```

If the project defines what its reviewers look at, follow that. After a push that moves the lines of a reference, correct its `revision` and its `line` with `nit doc edit`.

## Edit

```sh
nit doc edit 3 --old "revokes" --new "revokes and logs"
nit doc edit 3 --old "## Open questions" -F section.md   # the new text from a file
nit doc append 3 -F more.md                              # a block at the end, after one empty line
```

`--old` must occur exactly once in the text. Add words around it until
it does. Reviewers can edit the same paragraph at that time, and
both edits remain. To move text, remove it with one edit and add it
with another.

## When a reviewer sends the document

Reviewers edit the document and then send it. The watch that the
plugin starts delivers the lines that the reviewers changed, as a diff.
The header of each hunk contains the first line of the hunk in the old
and in the new text, counted from 0, and the writers of the hunk. Act
on the lines: change the code and push, or answer in the document with
`nit doc`. `nit doc show 3 --since <version>` prints the same diff for
any earlier version.
