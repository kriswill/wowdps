// This repo's okf scaffolder — the wowdps-specific metadata pass, invoked by
// `okf scaffold` via okflight.toml `[scaffold] script`. One pass per
// scaffolded type, each in its own file beside this entry:
//   crates.ts   — one Crate doc per workspace member (crates/*, tools/extract)
//   rulings.ts  — one Ruling doc per R# row of CONTRACT.md's rulings table
//   tools.ts    — one Tool doc per tools/*.sh generator / census script
// Idempotence, --force and the written/skipped summary are owned by the
// injected ctx.emit (okflight's scaffold-api.ts); the passes use only the
// injected API plus node builtins — no runtime import from an okf checkout.
// The `_` prefix keeps this directory out of okf's bundle walk, so the bundle
// itself stays pure markdown and OKF-conformant.

import type { ScaffoldContext } from "./scaffold-api";
import { scaffoldCrates } from "./crates";
import { scaffoldRulings } from "./rulings";
import { scaffoldTools } from "./tools";

export default async function scaffold(ctx: ScaffoldContext) {
  scaffoldCrates(ctx);
  scaffoldRulings(ctx);
  scaffoldTools(ctx);
}
