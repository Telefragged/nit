// @generated from crates/nit-document by `nix run .#gen-types` — DO NOT EDIT.
// Change the Rust types, then regenerate.

import type {
  AttachmentHash,
  ChangeNumber,
  RevisionNumber,
  Side,
  Tags,
} from "./types.gen";

/**
 * The side that made an edit.
 */
export type Writer = "author" | "reviewer";

/**
 * What a link on a line of its own shows on the document page.
 *
 * Its text form is an address on the nit server. References to
 * changes have the address of the review page:
 *
 * ```rust
 * use nit_document::Reference;
 *
 * let card: Reference = "/changes/412".parse().unwrap();
 * assert!(matches!(card, Reference::Change { .. }));
 *
 * let lines = "/changes/412?revision=3&file=src%2Ffold.rs&line=40-72";
 * let file: Reference = lines.parse().unwrap();
 * assert_eq!(file.to_string(), lines);
 *
 * assert!("/chains/7".parse::<Reference>().is_err());
 * ```
 */
export type Reference =
  | { type: "change"; change: ChangeNumber }
  | {
      type: "file";
      change: ChangeNumber;
      revision: RevisionNumber;
      against: RevisionNumber | null;
      file: string;
      lines: LineRange | null;
    }
  | { type: "upload"; hash: AttachmentHash; name: string };

/**
 * A range of lines on one side of a diff. Lines count from 1, and the
 * range includes `end`.
 *
 * ```rust
 * use nit_document::LineRange;
 * use nit_types::domain::Side;
 *
 * assert!(LineRange::new(Side::New, 40, 72).is_ok());
 * assert!(LineRange::new(Side::New, 0, 3).is_err());
 * assert!(LineRange::new(Side::Old, 9, 8).is_err());
 * ```
 */
export type LineRange = { side: Side; start: number; end: number };

/**
 * A reference in a text, with the place of its paragraph.
 */
export type PlacedReference = {
  /**
   * The first line of the paragraph in the text, counted from 0.
   */
  line: number;
  reference: Reference;
};

/**
 * A point in the history of a document: the edits that a replica
 * contains. Its text form is the last edit of each concurrent branch,
 * for example `41@7`, or `-` for a document with no edit.
 */
export type Version = string;

/**
 * One version in the history of a document: a sequence of edits by one
 * writer, with no edit of the other writer between them.
 *
 * [`Document::runs`] puts every edit of a document in one order, in
 * which each edit follows the edits that its writer had seen. Concurrent
 * edits take the order of their logical clocks, so every replica with
 * the same edits has the same runs.
 */
export type Run = {
  writer: Writer;
  /**
   * The version after the last edit of the run. It contains every
   * edit of this run and of the runs before it.
   */
  version: Version;
  /**
   * The time of the last edit of the run, in RFC 3339, from the clock
   * of its replica.
   */
  at: string;
};

/**
 * The number of a document on a server.
 */
export type DocumentNumber = number;

/**
 * The `POST /api/documents` request: an empty document.
 */
export type CreateDocument = {
  title: string;
  /**
   * The tags of the document. They never change.
   */
  tags: Tags;
};

/**
 * A document without its text.
 */
export type DocumentSummary = {
  number: DocumentNumber;
  title: string;
  /**
   * The edits that the server has.
   */
  version: Version;
  /**
   * The version that a reviewer last sent to the author.
   */
  sent: Version | null;
};

/**
 * The `GET /api/documents` response.
 */
export type DocumentList = { documents: Array<DocumentSummary> };

/**
 * Selects documents. With no tag, the query selects every document.
 */
export type DocumentQuery = {
  /**
   * Documents that have every one of these tags.
   */
  tag?: Array<string>;
};

/**
 * What a client of `WS /api/documents/stream` follows. The client
 * sends it as a text frame, and a later subscription replaces it.
 *
 * The server sends the edits that the client lacks as a binary frame
 * ([`update_frame`]), and each sent version as a [`DocumentSent`]
 * text frame.
 */
export type DocumentSubscription = {
  /**
   * The documents to receive edits of, each with the version that
   * the client has.
   */
  updates: { [key in DocumentNumber]: Version };
  /**
   * The documents to receive the sent versions of.
   */
  sent: DocumentQuery | null;
  /**
   * The sent version of each document that the client has seen.
   */
  seen: { [key in DocumentNumber]: Version };
};

/**
 * A text frame from `WS /api/documents/stream`: a reviewer sent
 * this version of the document to the author.
 */
export type DocumentSent = { document: DocumentNumber; version: Version };
