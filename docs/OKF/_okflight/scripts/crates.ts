// Crate pass: every Cargo workspace member becomes crates/<name>.md. The
// description comes from Cargo.toml's `description`; the body's blurb from the
// crate root's `//!` doc comment (lib.rs, else main.rs). Members are read
// from the root Cargo.toml's [workspace] members list, so a new crate needs
// no edit here.

import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import type { ScaffoldContext } from "./scaffold-api";

function workspaceMembers(root: string): string[] {
  const toml = readFileSync(join(root, "Cargo.toml"), "utf8");
  const m = toml.match(/members\s*=\s*\[([^\]]*)\]/);
  if (!m) return [];
  return [...m[1].matchAll(/"([^"]+)"/g)].map((x) => x[1]);
}

function cargoField(toml: string, key: string): string | null {
  const m = toml.match(new RegExp(`^${key}\\s*=\\s*"([^"]*)"`, "m"));
  return m ? m[1] : null;
}

export function scaffoldCrates(ctx: ScaffoldContext) {
  for (const dir of workspaceMembers(ctx.root)) {
    const manifest = join(dir, "Cargo.toml");
    if (!existsSync(join(ctx.root, manifest))) continue;
    const toml = readFileSync(join(ctx.root, manifest), "utf8");
    const name = cargoField(toml, "name") ?? dir.split("/").pop()!;
    const description = cargoField(toml, "description") ?? ctx.titleFromSlug(name);
    const rootFile = ["src/lib.rs", "src/main.rs"]
      .map((f) => join(dir, f))
      .find((f) => existsSync(join(ctx.root, f)));
    const blurb = rootFile
      ? ctx.leadingComment(readFileSync(join(ctx.root, rootFile), "utf8"), "//!")
      : null;
    const bins = [...toml.matchAll(/^\[\[bin\]\]\s*\nname\s*=\s*"([^"]+)"/gm)].map((x) => x[1]);
    const slug = name.replace(/^wowdps-/, "");
    const body = [
      ctx.mdSafe(ctx.sentence(blurb ?? description)),
      "",
      "## Source",
      "",
      `- Manifest: [\`${manifest}\`](../../../${manifest})`,
      ...(rootFile ? [`- Root: [\`${rootFile}\`](../../../${rootFile})`] : []),
      ...(bins.length ? [`- Binaries: ${bins.map((b) => `\`${b}\``).join(", ")}`] : []),
      "",
      "## Contract",
      "",
      "Public signatures and dependency policy: [`CONTRACT.md`](../../../CONTRACT.md).",
    ].join("\n");
    ctx.emit(
      `crates/${slug}.md`,
      {
        type: "Crate",
        title: name,
        description: ctx.firstSentence(description),
        resource: dir,
        tags: ["crate"],
        status: "stable",
        generated: ctx.generated(dir),
      },
      body,
    );
  }
}
