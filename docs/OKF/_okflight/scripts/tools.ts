// Tool pass: every tools/*.sh script (the gen-* regenerators and census
// scripts) becomes tools/<name>.md, described by its leading `#` comment.

import { readFileSync } from "node:fs";
import { join } from "node:path";
import type { ScaffoldContext } from "./scaffold-api";

export function scaffoldTools(ctx: ScaffoldContext) {
  for (const path of ctx.vcs.trackedFiles()) {
    if (!/^tools\/[^/]+\.sh$/.test(path)) continue;
    const name = path.split("/").pop()!.replace(/\.sh$/, "");
    const src = readFileSync(join(ctx.root, path), "utf8").replace(/^#![^\n]*\n/, "");
    const blurb = ctx.leadingComment(src, "#");
    const body = [
      ctx.mdSafe(ctx.sentence(blurb ?? ctx.titleFromSlug(name))),
      "",
      "## Source",
      "",
      `- Script: [\`${path}\`](../../../${path})`,
      "- Extraction rules: [`tools/extract`](../crates/extract.md) (the `wowdps-extract` crate).",
    ].join("\n");
    ctx.emit(
      `tools/${name}.md`,
      {
        type: "Tool",
        title: name,
        description: ctx.firstSentence(blurb ?? ctx.titleFromSlug(name)),
        resource: path,
        tags: ["tool", "game-data"],
        status: "stable",
        generated: ctx.generated(path),
      },
      body,
    );
  }
}
