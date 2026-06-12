// Per-line syntax highlighting for diffs. Language comes from the file
// extension; unknown extensions are skipped silently (docs/frontend.md).

import hljs from "highlight.js/lib/core";
import bash from "highlight.js/lib/languages/bash";
import c from "highlight.js/lib/languages/c";
import cpp from "highlight.js/lib/languages/cpp";
import css from "highlight.js/lib/languages/css";
import go from "highlight.js/lib/languages/go";
import ini from "highlight.js/lib/languages/ini";
import javascript from "highlight.js/lib/languages/javascript";
import json from "highlight.js/lib/languages/json";
import markdown from "highlight.js/lib/languages/markdown";
import python from "highlight.js/lib/languages/python";
import rust from "highlight.js/lib/languages/rust";
import sql from "highlight.js/lib/languages/sql";
import typescript from "highlight.js/lib/languages/typescript";
import xml from "highlight.js/lib/languages/xml";
import yaml from "highlight.js/lib/languages/yaml";

hljs.registerLanguage("bash", bash);
hljs.registerLanguage("c", c);
hljs.registerLanguage("cpp", cpp);
hljs.registerLanguage("css", css);
hljs.registerLanguage("go", go);
hljs.registerLanguage("ini", ini);
hljs.registerLanguage("javascript", javascript);
hljs.registerLanguage("json", json);
hljs.registerLanguage("markdown", markdown);
hljs.registerLanguage("python", python);
hljs.registerLanguage("rust", rust);
hljs.registerLanguage("sql", sql);
hljs.registerLanguage("typescript", typescript);
hljs.registerLanguage("xml", xml);
hljs.registerLanguage("yaml", yaml);

const EXT_LANG: Record<string, string> = {
  sh: "bash",
  bash: "bash",
  c: "c",
  h: "c",
  cc: "cpp",
  cpp: "cpp",
  hpp: "cpp",
  css: "css",
  go: "go",
  toml: "ini",
  ini: "ini",
  js: "javascript",
  jsx: "javascript",
  mjs: "javascript",
  json: "json",
  md: "markdown",
  py: "python",
  rs: "rust",
  sql: "sql",
  ts: "typescript",
  tsx: "typescript",
  html: "xml",
  xml: "xml",
  yml: "yaml",
  yaml: "yaml",
};

export function languageFor(path: string): string | null {
  const dot = path.lastIndexOf(".");
  if (dot < 0) return null;
  return EXT_LANG[path.slice(dot + 1).toLowerCase()] ?? null;
}

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

/**
 * Highlight one diff line to an HTML string. Line-at-a-time loses multi-line
 * constructs (block comments resume as code) — accepted for v1.
 */
export function highlightLine(text: string, language: string | null): string {
  if (!language) return escapeHtml(text);
  try {
    return hljs.highlight(text, { language, ignoreIllegals: true }).value;
  } catch {
    return escapeHtml(text);
  }
}

/**
 * Wrap the text range [start, end) of a highlighted line in
 * `<span class="{className}">`. Offsets are into the raw line text;
 * walking the rendered DOM keeps entity escaping and hljs token spans
 * intact, splitting the mark across token boundaries. Repeated
 * application stacks marks (text offsets are invariant under added
 * tags), so overlapping ranges simply nest.
 */
export function markTextRange(
  html: string,
  start: number,
  end: number,
  className: string,
): string {
  if (start >= end) return html;
  const tpl = document.createElement("template");
  tpl.innerHTML = html;
  const walker = document.createTreeWalker(tpl.content, NodeFilter.SHOW_TEXT);
  const texts: Text[] = [];
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    texts.push(n as Text);
  }
  let offset = 0;
  for (const node of texts) {
    const nodeStart = offset;
    offset += node.data.length;
    const from = Math.max(start, nodeStart) - nodeStart;
    const to = Math.min(end, offset) - nodeStart;
    if (from >= to) continue;
    const span = document.createElement("span");
    span.className = className;
    span.textContent = node.data.slice(from, to);
    const frag = document.createDocumentFragment();
    if (from > 0) frag.append(node.data.slice(0, from));
    frag.append(span);
    if (to < node.data.length) frag.append(node.data.slice(to));
    node.replaceWith(frag);
  }
  return tpl.innerHTML;
}

/** Intraline change emphasis: [`markTextRange`] with the diff tint. */
export function markIntraline(
  html: string,
  start: number,
  end: number,
): string {
  return markTextRange(html, start, end, "intraline");
}
