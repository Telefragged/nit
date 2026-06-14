// Strict CSS linting — the stylesheet counterpart to eslint.config.js (and
// clippy::pedantic on the backend). stylelint-config-standard is the strict
// baseline; it carries no formatting rules that fight prettier (those were
// removed upstream in v15), so formatting stays prettier's job via treefmt.
//
// The temporary burn-down allow-list is empty: every standard rule is
// enabled. The two below are turned off permanently and with a reason —
// they genuinely don't fit this codebase, the way `similar_names = allow`
// doesn't fit the Rust side — never a silent "this one is noise".

/** @type {import("stylelint").Config} */
export default {
  extends: ["stylelint-config-standard"],
  rules: {
    // highlight.js stamps its own token classes (.function_, .class_,
    // .hljs-built_in, …) onto highlighted code; styles.css themes them, and
    // they are third-party names we cannot rename to kebab-case. Our own
    // classes follow the A/M/D/R status-letter convention (.fstat-A) that
    // mirrors the letter shown in the UI.
    "selector-class-pattern": null,
    // Every current occurrence is a benign override: the higher-specificity
    // selector (.change-threads .editor, .meta-col .thread, …) wins by
    // specificity, not source order, so there is no cascade bug to fix. The
    // sheet is grouped by component; reordering to ascending specificity
    // would scatter related rules across hundreds of lines for no visual
    // change.
    "no-descending-specificity": null,
  },
};
