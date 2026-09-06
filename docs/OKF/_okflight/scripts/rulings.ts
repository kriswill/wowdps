// Ruling pass: CONTRACT.md's rulings table (one <tr> "the call" row per R#,
// followed by a colspan detail row) becomes rulings/r<N>.md — the call as the
// description, the detail as the body. CONTRACT.md stays the binding text;
// these docs are the graph's handle on it (what a ruling touches, which
// fixture gates it, which crates implement it — enrich by hand).

import { readFileSync } from "node:fs";
import { join } from "node:path";
import type { ScaffoldContext } from "./scaffold-api";

function untag(html: string): string {
  return html
    .replace(/<code>([^<]*)<\/code>/g, "`$1`")
    .replace(/<em>([^<]*)<\/em>/g, "*$1*")
    .replace(/<b>([^<]*)<\/b>/g, "**$1**")
    .replace(/<br\s*\/?>/g, " ")
    .replace(/<[^>]+>/g, "")
    .replace(/&amp;/g, "&")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"');
}

export function scaffoldRulings(ctx: ScaffoldContext) {
  const path = "CONTRACT.md";
  const md = readFileSync(join(ctx.root, path), "utf8");
  const table = md.match(/## Rulings[\s\S]*?<tbody>([\s\S]*?)<\/tbody>/);
  if (!table) {
    ctx.log("rulings: CONTRACT.md rulings table not found — pass skipped");
    return;
  }
  const rows = [...table[1].matchAll(/<tr>([\s\S]*?)<\/tr>/g)].map((m) => m[1]);
  for (let i = 0; i < rows.length; i++) {
    const cells = [...rows[i].matchAll(/<td(?:[^>]*)>([\s\S]*?)<\/td>/g)].map((m) => m[1]);
    if (cells.length !== 4 || !/^R\d+$/.test(cells[0].trim())) continue;
    const id = cells[0].trim();
    const name = untag(cells[1]).trim();
    const call = ctx.clean(untag(cells[2]));
    const conflict = ctx.clean(untag(cells[3]));
    const next = rows[i + 1] ?? "";
    const detailCell = next.match(/<td colspan="3">([\s\S]*?)<\/td>/);
    const detail = detailCell ? ctx.clean(untag(detailCell[1])) : "";
    const body = [
      `**The call:** ${ctx.mdSafe(ctx.sentence(call))}`,
      "",
      `**A conflicting change would:** ${ctx.mdSafe(ctx.sentence(conflict))}`,
      "",
      "## Detail",
      "",
      detail ? ctx.mdSafe(detail) : "_(the binding detail lives in CONTRACT.md)_",
      "",
      "## Source",
      "",
      `- Binding text: [\`${path}\`](../../../${path}) — rulings table, row ${id}.`,
    ].join("\n");
    ctx.emit(
      `rulings/${id.toLowerCase()}.md`,
      {
        type: "Ruling",
        title: `${id} ${name}`,
        description: ctx.firstSentence(call),
        resource: path,
        tags: ["ruling", "contract"],
        status: "stable",
        generated: ctx.generated(path),
      },
      body,
    );
  }
}
