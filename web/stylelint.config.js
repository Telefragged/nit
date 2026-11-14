// Strict CSS linting, the stylesheet counterpart to eslint.config.js.
// stylelint-config-standard contains no formatting rules, so prettier
// formats the stylesheets via treefmt.

/** @type {import("stylelint").Config} */
export default {
  extends: ["stylelint-config-standard"],
  rules: {
    // Our own classes follow the A/M/D/R status-letter convention
    // (.fstat-A), which matches the letter shown in the UI.
    "selector-class-pattern": null,
    // The sheets are grouped by component. Each override with a higher
    // specificity (.change-threads .editor, .meta-col .thread, …) wins by
    // specificity, not by source order.
    "no-descending-specificity": null,
    // Tailwind compiles the stylesheet (@tailwindcss/vite). It inlines an
    // import only in the string notation, and `@source` and `@theme` are
    // its at-rules.
    "import-notation": "string",
    "at-rule-no-unknown": [true, { ignoreAtRules: ["source", "theme"] }],
  },
};
