---
type: Decision
title: Resolve An Encounter Through An Ordered Layer Stack
description: 'The encounter rubric resolves each encounter through an ordered stack of sources in three tiers (base, curated, user): the base''s four files, every curated set in the order laid (a tuned file embedded beside its draft, or a checkout''s tree on disk in its place, then each runtime bundle), then the user''s own files, with a provenance per key path; main carries the base alone, so the curated layer can ship apart and a user can override anything on their own machine.'
tags: [replay, rubric, design]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-17T14:00:00-07:00 }
sources:
  - id: readme
    resource: ../../../crates/encounter-rubric/README.md
    title: The rubric's schema reference — How an encounter resolves, Three tiers
  - id: plan
    resource: ../../plan-fight-replay.md
    title: Fight replay plan — §3 layers 3–4, the curated definitions per tier
---

**Where:** [wowdps-encounter-rubric](../crates/encounter-rubric.md)
(`Tier`, `Source`, `Origin`, `Provenance`, `Rubric::with_curated`,
`with_curated_dir`, `from_dir`, `base_origin`, `with_user`,
`with_user_dir`, `encounter_traced`,
`layers_of`, `trash`, `trash_traced`, `trash_layers_of`), for
[A Fight Replay Captured Once And Interpreted Per Tier](fight-replay-captured-once.md).
It supersedes
[Keep The Rubric's Base Apart From Its Curated Layer](rubric-base-and-curated.md),
whose base standing alone it keeps.

## Context

The rubric is the plan's curated definitions per tier[^plan]. It grew on
the replay branch with two tiers: the BASE (the season and instance files
and the generated draft, the encounter's map and NPCs among them) and
CURATED (a hand-tuned file laid over it). Both lived side by
side in one embedded tree, and the resolver knew of exactly one tuned file
per encounter. Bringing the crate to the public repository raised three
needs that shape did not meet:

- **The base ships alone.** The tuned files are the work that makes the
  replay say what the fight does; they stay out of the public repository
  and must be able to arrive later as a separate set, without the base
  depending on them.
- **More than one curated set.** A set laid in at runtime (a bundle) has to
  stack over an embedded tuned file, and two bundles over each other, in a
  defined order.
- **The user's own say.** Someone tuning their own replay needs to override
  any key on their machine without touching the repository, and a mistake
  in their files must not take the rubric down.

A reader also had no way to ask why an encounter says what it says: which
file set a given key.

## Decision

An encounter resolves through an ordered stack of `Source`s[^readme]:
`Season`, `Instance`, `Draft`, `Drawing` (the base's four), then
`Curated(Origin)` for each curated set in the order laid (`Embedded`, a
tuned file beside its draft, first; then each `Bundle(name)`), then `User`.
Each names its `Tier` (`Base < Curated < User`); a reader asks for a tier
per encounter and `with_tier` caps the rubric, whose default is now `User`,
everything the machine has.

- **A set is one block.** A curated or user set is a file tree shaped like
  the crate's `seasons/` (paths `<season>/<instance>/<file>`): its
  `season.toml` (`[defaults]` alone), its `instance.toml`
  (`[defaults]` and `[encounter.<id>]` drawings) and `<id>-<slug>.toml`,
  laid in that order and as a whole over everything under it. Every season
  and instance it names must be the base's; a draft is never in one.
- **Difficulty overrides stay last.** Every source's `[difficulty.<name>]`
  overrides are kept apart and laid after all of the stack's own values,
  the most general difficulty first and each difficulty's in stack order,
  which is the order the two-tier resolver already used. A tuned file
  embedded on the replay branch therefore answers exactly as before.
- **Two ways in for curated.** An embedded tuned file classifies as
  `Curated(Embedded)` as before; `with_curated(origin, files)` lays a
  bundle in at runtime, whole or not at all (a file it cannot lay, or an
  encounter it makes unreadable where the tiers under it read, refuses it).
- **The user layer never fails the rubric.** `with_user` lays files one by
  one; `with_user_dir` reads `$XDG_CONFIG_HOME/wowdps/rubric/` through
  `proto::dirs`. A file that cannot be read, parsed or laid, or that would
  make an encounter unreadable on any difficulty, goes to `user_errors` and
  is left out, as `with_texts` treats a bad sidecar.
- **Provenance per key path.** The one merge rule (tables key by key,
  anything else whole) runs with an optional `Provenance` beside it: per
  key path (`ability.serpents-bite.shape.length`, `map.stencil`), the
  source that last set it and the difficulty override it came through. A
  list or scalar laid over a subtree forgets the subtree's paths.
  `encounter_traced` answers it with the same `Encounter` `encounter_at`
  gives.
- **`layers_of` replaces the boolean.** It lists the sources with a file an
  encounter reads; `has_curated` and `has_user` read it.

## Consequences

- The public repository holds the generated base and every hand-made map
  and NPC amendment, inside the crate (`crates/encounter-rubric/seasons/`,
  a directory per season, the user's tree mirroring it), and nothing of the
  curated layer; the embedded test
  resolves and checks every encounter at every tier, which on main is the
  base standing alone.
- A curated set can be built and distributed on its own terms (the product
  plan's later step fills `Origin::Bundle`) without touching the base.
- `Rubric::version()` folds every laid file into its hash, the curated sets'
  and the user's too, so a stored interpretation regrades when any of them
  changes; laying nothing leaves it as it was. A reader that records it
  should record the tier it read at as well.
- Validating a user file or a bundle resolves each encounter it touches on
  every difficulty, twice at most; a `season.toml` touches a whole season.
  That is milliseconds at load, never per pull.
- Not done here: no vocabulary gate for what a curated or user layer may
  name (an entry the base lacks is simply added), deferred by choice.
  Nothing on main reads the crate yet; the replay branch does, through
  the old name until it merges this.

## Amendment 2026-10-13: a checkout's tuned files in the embedded ones' place

The editors (the private wowdps-editors repository) save an encounter's
tuned file into a wowdps checkout. A reader that embeds the rubric saw
the save only after a rebuild. `Rubric::with_curated_dir(origin, dir)`
reads the tuned files of a `seasons/` tree on disk and lays them as
`Curated(Origin::Dir(origin))`, a third way in for curated:

- **In place of the embedded files, not over them.** A key the editor took
  out of a tuned file must leave the curated tier, so the tree's set takes
  the embedded set's place (first among the curated, under every bundle),
  and an encounter only the embedded files brought goes with them.
  `Origin::Dir` orders between `Embedded` and `Bundle`, as the stack lays
  them.
- **Tuned files alone.** A checkout's drafts, `instance.toml` and
  `season.toml` are the base's, which a curated set may not give (and
  which would read as curated if laid as one); the base stays the build's,
  so a map edit still takes a rebuild.
- **The same checks, refused whole.** The set goes through what
  `with_curated` applies; a file is named by its place on disk. A tree
  holding the build's own tuned files resolves every encounter, its layers
  and its provenance as the embedded rubric does (a test lays the crate's
  `seasons/` so).
- **Versioned.** The tree's files fold into `version()`, so a reader can
  read the tree again cheaply and keep what it had where the version did
  not move: the replay spike's `--seasons` does that as each pull is read.

## Amendment 2026-10-17: an instance's trash is a subject of its own

A keystone run is the unit: its trash, route and timer matter as much as
its boss fights, and the rubric applied only inside each boss's fight, with
nothing between (the replay's Mythic+ plan, `docs/plan-mplus.md` on the
replay branch, its direction and Step 1). Two things were added, one per tier:

- **Identities in the base, at the instance's scope.** An instance file
  gives its own creatures, `[npc.<key>]` with the same `Npc` and their
  enemy `forces`, and the dungeon's whole `forces`. The instance, not a
  level, because a creature may stand on more than one level and a reader
  finds a creature by its id alone, without the floor, by the map id a
  run's log writes
  (`Rubric::instance_by_map`, `Instance::npc_of`). A set's instance file
  may give only `defaults` and `encounter`, so the table is the base's by
  construction, as a level is: who is in the dungeon and what each is
  worth looks the same at every tier.
- **Mechanics in the curated layer, in an encounter's shape.** A
  `trash.toml` per instance (`FileRole::Trash`) is laid as a tuned file is
  (embedded, a tree on disk's, a bundle's, the user's) over the instance's
  creatures, and resolves to the existing `Encounter`: `encounter` 0, the
  instance's name and slug, at a keystone through its fallback chain
  (`Rubric::trash`). Reusing the type keeps `gate`, `check`, `only`, the
  provenance and the replay's rules unchanged, and an editor's fight
  sections save into it as they do a boss's. What a run's trash does was
  found by investigating eleven keys, as a boss's mechanics were, so it is
  curated; the identities it names are extraction and census, so they are
  base. `TRASH_TABLES` keeps what a run has elsewhere out of it (its
  levels' `map`, `view` and `travel`, its bosses' `phase` and `room`), and
  `check_trash` (what `check` finds, and more) refuses a `start` trigger
  and a bare `after`, which would count from the key's start.
- **The stack's checks name a subject.** Laying a set reads every reading
  of what its files are read by; those were keyed by encounter id and are
  now a `Subject`: an encounter, or an instance's trash. A bundle that
  breaks a trash is refused naming it, and a user's trash file a later set
  leaves unreadable is taken out, as an encounter's file is.

A trash file may add a creature the instance lacks, as a curated set may
add an NPC to a boss: the vocabulary gate the Consequences above defer is
still deferred. The trash has no clock of its own yet; whether a trash
entry counts from its pack's engage is the plan's open question, and the
whole-run form comes first. The schema moved to 2 with the creatures, so
an older build refuses a file in 2 by its number; a `trash.toml` it
refuses by its name instead, taking it for a misnamed encounter file, and
so refuses a whole tree holding one (the editors at an older pin among
them).

## Amendment 2026-10-17: a checkout's whole tree in the build's place

The 2026-10-13 way in left the base the build's, so a map, a level or an
instance's creatures the editors saved into a checkout still took a
rebuild, and a keystone run's replay, drawn over its levels and counted by
its creatures, is mostly base (the Mythic+ plan). `Rubric::from_dir(dir)` reads every
file of a tree on disk through the path `from_files` takes:

- **The whole tree, in the build's place.** Its seasons, instances (their
  levels and creatures among them), drafts and drawings are the base, its
  tuned and trash files the first curated set,
  `Curated(Origin::Dir(path))`, so `with_curated_dir` and every bundle lay
  over it as over the build's; `base_origin` names where the base came
  from. Not the tree's base laid over the build's: no set may give a
  draft or a level, and a key taken out of the tree must leave, so the
  tree replaces the build's rubric whole, and a reader keeps the build's
  for when the tree is refused.
- **Refused whole, and read before it is taken.** As the build's reader
  refuses a tree, a file that does not read or place refuses it, named by
  its place on disk; so does a path that is no directory or holds no
  `season.toml`. The build's tree is held readable by its test; a tree
  being edited has none, so `from_dir` also reads every encounter and
  every instance's trash on every difficulty at the base and the curated
  tier, and every level, and refuses the tree where one fails, as
  `with_curated_dir` refuses a set that breaks an encounter (about 30 ms
  over the seasons in a release build). A reader then keeps what it had,
  never a rubric with an encounter that will not read. So a tree is held
  to more than the build's reader holds a build's files to (`from_files`
  never reads them so, its test does): a draft or an instance file saved
  mid-work that fails on one difficulty refuses the whole tree, and holds
  the replay spike on its last good rubric until it is fixed (its panel
  saying "Rubric · disk (stale)", its `.done` the refusal). A writer, the
  editors among them, saves each file whole, written beside it and moved
  over it, or a reader may read it half written.
- **Strays are not the tree's.** A dot-file or directory (an editor's
  `.#instance.toml` lock, a link to nothing; a swap file), a link to
  nothing, a file gone since its directory was listed and a non-UTF-8
  name on anything but a `.toml` are passed over, by the walk and by
  `build.rs` alike, so one never refuses a tree the build would read.
- **A walk before a parse.** `TreeFiles::read` reads a tree's files
  without parsing them, and its `fingerprint` hashes them: a reader that
  keeps the fingerprint its rubric was made from parses only a tree that
  changed (`from_tree`, `with_user_tree` lay what the walk read). The
  spike reads the tree again at every pull read, so its unchanged case
  must cost a walk, not a parse and a check (150 to 250 ms in a dev
  build).
- **Its own version.** The files hash as the build's do, then the tree's
  path is folded in: the same files give another version than the
  build's, as `with_curated_dir`'s do, because the provenance names the
  tree. The replay spike swaps a launch's fallback for the tree's
  rubric as soon as the tree reads.

The replay spike's `--seasons` reads the tree so (`spike: rubric: base
and curated from DIR`) as each pull is read and on the control file's
`reread`, its panel's label saying "Rubric · disk". Still not read again:
a cached floor render, an arena's, a room's or a level's, is drawn as the
cache holds it until gen-floors renders it again; the spike names a boss
pull's stale render against the tree's rubric (`warn_stale`), a key run's
room and level renders not at all (a level's params fold in the walked
ground's posts). The extractor's tools read a tree with their own walk
and `from_files` (`--rubric DIR`), and could take `from_dir`. Landed in
`8446b72` and `dc710f8`; the review's fixes (strays, the walk first, a
refusal said once and shown, one rubric a load) in `fb210ae` and
`e417620`.

## Amendment 2026-10-17: a level's pull tables are the base's

A keystone's usual route is its levels' packs in order, the Mythic+ plan's
step 5. A level gives them as `[level.<ui_map>.pull.<key>]` (`Pull`: an
`order`, an `area` of the level's map, its members as keys of the
instance's creatures, a key repeated for each of that creature, and
optional `spots`, one a member). They are base, as the rest of a level
is: packs and routes are map data from the same census of runs as the
areas, and a set's instance file giving a level is already refused. Two
choices follow from the stack:

- **No defaults.** The season's and the instance's `[defaults]` reach a
  level for its map, view and travel alone (`LEVEL_DEFAULTS`, apart from
  `LEVEL_TABLES`): a pull is one level's. A `[defaults.pull]` is refused
  where its file is read, the base's or a set's, as is any default that
  is no encounter's table (`DEFAULTS_TABLES`, held to the `Encounter`'s
  tables by a test): every encounter under a `[defaults]` lays it in
  whole, and the `Encounter` refuses a key it lacks, so one stray default
  made every encounter of its season or instance unreadable.
- **The creatures laid in, not passed.** `check_level` takes a level
  alone, and the editors call it so. `resolve_level` lays the instance's
  creatures into a `Level::npc` no file gives, which the check holds a
  pull's members to and `Pull::forces` counts. A level read from its text
  alone (an editor's) has none, so `check_level_with` takes them given,
  and a level's equality and dump leave them out, so the level read and
  the level answered compare alike. A pull's share of the enemy forces
  is derived from its members, never written, so it cannot go stale as
  the season's values move, and an area's name stops carrying one.

`Level`'s Debug is written by hand as the derived form was, `pull` shown
only where there are some and the instance's creatures never, so
`fixtures/resolved-levels.txt` holds every level as before; the schema
stays 2, no committed file having given a pull.

## Amendment 2026-10-30: the curated set outside the crate

On the replay branch the tuned and trash files left the crate's
`seasons/` for the repository's `curated/seasons/`, and the replay lays
them over the embedded base as one bundle (`curated`), the third way in
this record gives, where they read as `Curated(Embedded)` before: every
encounter, trash and level resolves as it did, at every tier. The crate's
own tree is then the base alone, as on main, and `--seasons` lays a
checkout's `curated/seasons` over its base with `with_curated_tree`, the
walked form of `with_curated_dir`:
[Keep The Curated Set Out Of The Rubric Crate's Tree](curated-set-leaves-the-rubric-tree.md).

[^readme]: `crates/encounter-rubric/README.md`, How an encounter resolves.
[^plan]: `docs/plan-fight-replay.md` §3, the definitions per tier.
