// Strict CSS linting — the stylesheet counterpart to eslint.config.js (and
// clippy::pedantic on the backend). stylelint-config-standard is the strict
// baseline; it carries no formatting rules that fight prettier (those were
// removed upstream in v15), so formatting stays prettier's job via treefmt.
//
// The rules below are a temporary BURN-DOWN ALLOW-LIST: each is a rule the
// standard config enables that styles.css doesn't satisfy yet, silenced
// (null) with its first-pass hit count and removed — with the CSS fixed —
// one rule/group per commit until this block is empty.

/** @type {import("stylelint").Config} */
export default {
  extends: ["stylelint-config-standard"],
  rules: {
    "no-descending-specificity": null, // 7
    "selector-class-pattern": null, // 7
  },
};
