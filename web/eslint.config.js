// Strict, type-aware linting for the web frontend — the first layer of
// hardening against an agent's first output, mirroring `clippy::pedantic`
// on the Rust side (see crates/nit/Cargo.toml).
//
// Two kinds of disable live here, and only two:
//
//   1. FORMATTER-OWNED (permanent) — rules that overlap with prettier
//      (run via treefmt). eslint-config-prettier turns these off across
//      core/ts/react; the @html-eslint formatting rules are turned off
//      explicitly below. Formatting is prettier's job, always.
//
//   2. BURN-DOWN ALLOW-LIST (temporary) — rules the strict presets enable
//      that the codebase doesn't satisfy yet. Each is silenced with its
//      first-pass violation count, and removed (with the code fixed) one
//      rule/group per commit until the block is empty. A silenced rule is
//      a debt, never a verdict — this list only shrinks.
import js from "@eslint/js";
import tseslint from "typescript-eslint";
import react from "eslint-plugin-react";
import reactHooks from "eslint-plugin-react-hooks";
import reactRefresh from "eslint-plugin-react-refresh";
import jsxA11y from "eslint-plugin-jsx-a11y";
import html from "@html-eslint/eslint-plugin";
import prettier from "eslint-config-prettier";
import globals from "globals";

export default tseslint.config(
  {
    ignores: [
      "dist/**",
      "node_modules/**",
      "**/*.tsbuildinfo",
      // Generated from crates/nit-types (nix run .#gen-types); ts-rs picks the
      // `type`/`Array<T>` style, prettier owns the formatting.
      "src/api/types.gen.ts",
    ],
  },

  // An inline `eslint-disable` is a debt with a reason, never free: a
  // stale one (the rule no longer fires) is itself an error — the
  // `#[expect]` model from the Rust side.
  { linterOptions: { reportUnusedDisableDirectives: "error" } },

  {
    files: ["**/*.{ts,tsx,mjs}"],
    extends: [
      js.configs.recommended,
      tseslint.configs.strictTypeChecked,
      tseslint.configs.stylisticTypeChecked,
    ],
    languageOptions: {
      parserOptions: {
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
      },
      globals: { ...globals.browser },
    },
    plugins: {
      react,
      "react-hooks": reactHooks,
      "react-refresh": reactRefresh,
      "jsx-a11y": jsxA11y,
    },
    settings: { react: { version: "detect" } },
    rules: {
      ...react.configs.flat.recommended.rules,
      ...react.configs.flat["jsx-runtime"].rules,
      ...reactHooks.configs.recommended.rules,
      ...jsxA11y.flatConfigs.strict.rules,
      "react-refresh/only-export-components": [
        "error",
        { allowConstantExport: true },
      ],

      // strictTypeChecked's restrict-template-expressions rejects every
      // non-string interpolation, numbers included. Allow numbers back: a
      // number coerces to its decimal string predictably, so `${count}` is
      // clear — whereas objects ("[object Object]") and nullish
      // ("undefined"), the rule's real targets, stay rejected.
      "@typescript-eslint/restrict-template-expressions": [
        "error",
        { allowNumber: true },
      ],

      // ── BURN-DOWN ALLOW-LIST (temporary; counts = first-pass hits) ──
    },
  },

  // ── Build/tooling files — not in tsconfig, so lint without type info ──
  {
    files: ["*.config.{ts,js}", "screenshots/**/*.{mjs,js}"],
    extends: [tseslint.configs.disableTypeChecked],
    languageOptions: {
      parserOptions: { projectService: false, project: null },
      globals: { ...globals.node },
    },
  },

  // ── index.html — HTML correctness rules; formatting stays with prettier ──
  {
    files: ["**/*.html"],
    ...html.configs["flat/recommended"],
    rules: {
      ...html.configs["flat/recommended"].rules,
      "@html-eslint/indent": "off",
      "@html-eslint/quotes": "off",
      "@html-eslint/attrs-newline": "off",
      "@html-eslint/element-newline": "off",
      "@html-eslint/no-extra-spacing-tags": "off",
      "@html-eslint/require-closing-tags": "off",
    },
  },

  // Last: defer all formatting rules to prettier (run via treefmt).
  prettier,
);
