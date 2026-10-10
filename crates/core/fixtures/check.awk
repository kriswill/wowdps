#!/usr/bin/env gawk -f
#
# check.awk — independent expected-value computer for wowdps fixtures.
#
# Reads a WoW advanced combat log and emits per-segment / per-player totals as a
# stable TSV. This is the VALIDATOR's own implementation of the CONTRACT.md R1-R6,
# R7 (+ its 2026-10-02 combat clock), R17, R18, R19 (+ the R2 amendment), R20, R21,
# R22 (+ friendly fire), R26, R27 and R28 semantics, written from the log grammar.
# It never calls, links, or consults the Rust implementation — that is the whole
# point: the Rust is graded against this, not the other way round. R18 (aura
# spans with caster and target) runs over a hard-coded copy of the FIXTURES'
# role-spell ids (ROLE, in BEGIN), never the generated Rust table; R20 (the
# shield ledger) likewise over the fixtures' absorb-spell ids (SHIELD); R21 (the
# stacked-debuff ledger) needs no table at all — admission is a data property.
#
# Usage:  gawk -f check.awk sample.txt sample.txt     # file passed TWICE (2 passes)
#   pass 1 builds the pet -> owner map (pets act before SPELL_SUMMON)
#   pass 2 accumulates totals
#
# Output: TSV, one row per (segment, player, metric). Sorted, diffable.
#
# Field offsets are 0-based per fixtures/FORMAT-NOTES.md (awk fields are 1-based,
# so awk $(n+1) == documented offset n). Offsets verified against a real retail
# log, build 12.0.7.

function strip(s) { gsub(/^"|"$/, "", s); return s }

# Unit-flag bits: 0x400 Player, 0x1000 Pet, 0x2000 Guardian
function isPlayerFlags(f,   v) { v = strtonum(f); return and(v, 0x400) != 0 }
function isPetFlags(f,     v) { v = strtonum(f); return and(v, 0x3000) != 0 }

# Attribute an acting unit to a meter row (a player GUID), or "" if it gets no row.
#
# The nil GUID must be rejected BEFORE the flag check: real logs emit SPELL_DAMAGE
# with sourceGUID 0000000000000000 and sourceFlags 0x514 (Raid|Friendly|
# PlayerControlled|Player) — 36 such lines in the reference log. Trusting the flags
# alone creates a phantom "unknown player" meter row.
#
# Ownership is scoped to the current EPOCH (R6): a mid-log COMBAT_LOG_VERSION means
# the logger restarted, so the pet-owner map is reset and a pet whose SPELL_SUMMON
# happened before the boundary is no longer attributable.
#
# R17 uses the SAME function on the DESTINATION: a hit on a pet is taken by its
# owner (folded), a hit on an NPC is taken by nobody.
function actor(guid, flags) {
    if (guid == "" || guid == "0000000000000000") return ""
    if (isPlayerFlags(flags)) return guid
    if ((guid SUBSEP epoch) in owner) return owner[guid SUBSEP epoch]
    return ""
}

# "7/27/2026 20:05:01.100-4" -> ms within the day (all fixtures are single-day)
function tsms(ts,   p, hms, sec, parts) {
    p = index(ts, " ")
    hms = substr(ts, p + 1)
    sub(/[-+][0-9]+$/, "", hms)
    split(hms, parts, ":")
    sec = parts[3] + 0
    return ((parts[1] * 3600) + (parts[2] * 60)) * 1000 + int(sec * 1000 + 0.5)
}

function newSeg(kind, name, ts, enc, diff) {
    nseg++
    segKind[nseg] = kind; segName[nseg] = name
    segStart[nseg] = ts;  segEnd[nseg] = ""; segOk[nseg] = ""
    segEnc[nseg] = enc;   segDiff[nseg] = diff
    cur = nseg
}

function note(seg, guid, metric, v) {
    if (guid == "") return
    val[seg SUBSEP guid SUBSEP metric] += v
    seen[seg SUBSEP guid] = 1
    if (!(guid in pname)) pname[guid] = guid
}

# ---- R17 destination side. `taken` = amount + absorbed (the log's amount is
# post-block, so blocked is NOT added); `absorbed` / `blocked` are the PARTIAL
# parts riding the damage event, and a hit a shield took WHOLE adds its amount
# to `taken` and `absorbed` alike (absorbed_whole(), R1); a full BLOCK's amount
# goes to `prevented`. The destination is attributed exactly like a source
# (players by flag, pets folded onto their owner; NPC destinations are nobody's).
# R17's destination universe is a friendly GUID — `Player-` or `Pet-`. A
# guardian summoned as a `Creature-` unit (Niuzao) folds onto its owner for
# every OFFENSIVE number (R4/R5) but never earns a Taken row, so damage to it
# sits in neither side of the identity (R22 keeps its self-harm off `damage`
# all the same, in `self_harm` but not in the `on_friendly` half the identity
# uses).
function friendlyGuid(g) {
    return (g ~ /^Player-/ || g ~ /^Pet-/)
}

# R17 amendment (2026-10-08): `reduced` = what armor and damage reduction took
# off — the suffix's unmitigated amount (off 1 after the base amount; an ABSORB
# miss's unmitigated after its amountMissed) less what came out of it: the RAW
# base amount (overkill INCLUDED, the log's amount being post-block and
# pre-overkill) + absorbed + blocked. Floored at 0: a vulnerability debuff lifts
# a hit above its unmitigated amount, which is not negative mitigation.
function reduced(unmit, out) { return (unmit > out) ? unmit - out : 0 }

function taken(dguid, dflags, amt, absorbed, blocked, unmit, base,   t) {
    if (!friendlyGuid(dguid)) return
    t = actor(dguid, dflags); if (t == "") return
    note(cur, t, "taken", amt + absorbed)
    note(cur, t, "absorbed", absorbed)
    note(cur, t, "blocked", blocked)
    note(cur, t, "reduced", reduced(unmit, base + absorbed + blocked))
    # R18 taken series: the same amount on a 10 s grid from the segment's start
    # (ENCOUNTER_START for an encounter, the first combat line for trash).
    # Exactly what `taken` records — a stagger tick never reaches here (R17).
    tk10[cur SUBSEP t SUBSEP int((now - segStart[cur]) / 10000)] += amt + absorbed
}

# ---- R18 aura spans. A Buff SPELL_AURA_APPLIED / _REFRESH whose spell is in
# ROLE opens a span keyed by (target, spell, caster) — the raw dst guid, the
# aura id, the raw src guid as `src` — so two casters of one spell on one target
# are two spans, each closed by its own removal; a re-apply / refresh by the
# SAME caster while its span is open is a no-op; SPELL_AURA_REMOVED closes that
# key's open span. A refresh or removal with NO open span opens one at the
# SEGMENT'S START (the buff predated the segment) — at most once per key per
# segment, a later orphan is dropped. The ruling folds `Pet-` targets onto their
# owners exactly like `taken`; this checker gates on the PLAYER flag (0x400)
# only because no committed fixture lands a role buff on a pet — the folding
# path is gated by tests/spans.rs, not here. Every aura line is passive:
# it never opens, extends or splits a segment (passive_stale(), as for a miss),
# so an aura after ENCOUNTER_END or past the trash gap lands nowhere. A span
# still open at the segment's end is closed AT READ TIME (END below) at the
# segment's end — the encounter's ENCOUNTER_END, a trash segment's last combat.
# Item marks (R12) are NOT spans and are not computed here; ROLE is consulted
# before any item logic would be, so a role spell never becomes an item mark.
function span_open(tgt, spell, src, at,   k) {
    k = cur SUBSEP tgt SUBSEP spell SUBSEP src
    nspan++
    spanSeg[nspan] = cur; spanTgt[nspan] = tgt; spanSpell[nspan] = spell
    spanSrc[nspan] = src; spanAt[nspan] = at; spanEnd[nspan] = ""
    spanKind[nspan] = ROLE[spell]
    openIdx[k] = nspan
    # count metrics land now (and register the row); the ms are read at END
    note(cur, tgt, "spans", 1)
    if (ROLE[spell] == "External") { note(cur, src, "externals_given", 1); note(cur, tgt, "externals_received", 1) }
}
function aura_apply(refresh,   spell, tgt, k) {
    if (strip($13) != "BUFF") return
    if (!isPlayerFlags($8)) return
    spell = $10 + 0
    if (!(spell in ROLE)) return
    if (passive_stale()) return
    tgt = $6; k = cur SUBSEP tgt SUBSEP spell SUBSEP $2
    if (openIdx[k] + 0 > 0) return                      # re-apply / refresh while open: no-op
    if (refresh) { if (retro[k]) return; retro[k] = 1 }  # segment-start rule: once per key
    span_open(tgt, spell, $2, refresh ? segStart[cur] : now)
}
function aura_remove(   spell, tgt, k) {
    if (strip($13) != "BUFF") return
    if (!isPlayerFlags($8)) return
    spell = $10 + 0
    if (!(spell in ROLE)) return
    if (passive_stale()) return
    tgt = $6; k = cur SUBSEP tgt SUBSEP spell SUBSEP $2
    if (openIdx[k] + 0 == 0) {                          # segment-start rule: once per key
        if (retro[k]) return
        retro[k] = 1; span_open(tgt, spell, $2, segStart[cur])
    }
    spanEnd[openIdx[k]] = now
    openIdx[k] = 0
}
# where a segment's clock stops: the read-time close of a span still open
function segClose(s) { return (segKind[s] == "Encounter" && segEnd[s] != "") ? segEnd[s] : segLast[s] }

# ---- R20 shield ledger. One state machine per key (segment, target, spell,
# caster) — the raw dst guid, the shield's aura id, the raw src guid, exactly
# the span key — where the caster of a shield aura IS its absorber (the log's
# `SPELL_ABSORBED` names the absorber where the aura names the caster; the
# census found 0 mismatches). The row lands on the absorber (actor() of the
# src / absorber, so a pet's shield would be its owner's; an NPC's is nobody's).
#
# Transitions (docs/plan-role-pivots-step5.md §0), every aura line through the
# passive gate, an absorb after it has opened/extended the segment as combat:
#   APPLIED  with a trailer opens `applied = remaining = a` (known); without
#            one opens unknown-applied; an apply while the key is OPEN first
#            closes the old shield with `wasted = remaining` when known.
#   REFRESH  the trailer is the shield's NEW RUNNING TOTAL, never a delta:
#            r > remaining → applied += r − remaining (a refresh up); r <
#            remaining → wasted += remaining − r (a refresh DOWN overwrites);
#            then remaining = r. No trailer, or no open shield: no-op.
#   ABSORBED (a non-NON_HEALING_ABSORBS shield) consumed += amount; an
#            over-absorb (amount > remaining) RAISES applied by the excess so
#            applied = consumed + wasted holds by construction, remaining → 0;
#            on a key not open it opens an unknown-applied shield.
#   REMOVED  wasted += the trailer when present, else `remaining` when known,
#            else nothing (the waste stays unknown); the shield closes. With
#            no open shield: no-op (a removal is not evidence of a shield).
#            The ENGINE's rule for a trailer off a known balance (real logs
#            only): ABOVE it, applied += the difference (raise-only, like the
#            over-absorb — stacking shields grow with no REFRESH line); BELOW
#            it, applied is left alone and the shield closes as unknown (the
#            row visibly inconsistent). This awk is STRICTER — see B3: the
#            fixtures are hand-balanced, so any disagreement is a fixture
#            bug, never a shield that grew.
#   segment close: every open shield folds with `consumed` and `count` ONLY
#            (applied and wasted dropped, unknown += 1) — Σ consumed over a
#            player's rows = `absorbheal` EXACTLY, the gated identity.
# Gate: an aura ledgers only when its spell is in SHIELD, the FIXTURES' absorb
# spells hard-coded (the Rust table is generated: crates/core/src/
# absorb_spells.rs, every SpellEffect with EffectAura 69) — Feast of Souls,
# Bone Shield and every `BUFF,0,0` carry a trailer and must never open a row.
# An absorb naming a spell OUTSIDE the set still ledgers (unknown-applied).
# Self-check (B3): whenever a REMOVED trailer and the running remaining are
# both known they must agree — a mismatch is a warning on stderr and a
# non-zero exit, which verify.sh treats as a FAIL. (The fixtures have no
# grow / shrink case on purpose; the engine's raise-only + unknown rule is
# exercised by crates/core/tests/shields.rs and the real-log gate.)
#
# Metrics: `absorb_applied` = Σ applied over CLOSED shields with a known
# applied; `absorb_wasted` = Σ wasted over closed shields with a known waste,
# printed BLANK when no such shield exists (the meter's None); `shields_unknown`
# = the count of shields whose applied was unknown (closed) + every shield still
# open at the segment's close.
function sh_open(k, owner, spell, applied, known, consumed) {
    shOpen[k] = 1; shOwner[k] = owner; shSpell[k] = spell
    shApplied[k] = applied; shKnown[k] = known; shRem[k] = applied; shRemKnown[k] = known
    shConsumed[k] = consumed; shWasted[k] = 0; shWasteKnown[k] = 0
}
function sh_close(k, s,   o, r) {
    o = shOwner[k]; s = substr(k, 1, index(k, SUBSEP) - 1) + 0
    if (shKnown[k]) note(s, o, "absorb_applied", shApplied[k])
    if (shWasteKnown[k]) { note(s, o, "absorb_wasted", shWasted[k]); wasteKnown[s SUBSEP o] = 1 }
    if (!shKnown[k]) note(s, o, "shields_unknown", 1)
    r = s SUBSEP o SUBSEP shSpell[k]
    shRowCount[r]++; shRowApplied[r] += shKnown[k] ? shApplied[k] : 0
    shRowConsumed[r] += shConsumed[k]; shRowWasted[r] += shWasteKnown[k] ? shWasted[k] : 0
    shRowUnknown[r] += !shKnown[k]
    if (SHIELDS) printf "shield close: seg %d owner %s spell %d key %s applied %s consumed %d wasted %s\n", s, o, shSpell[k], k, (shKnown[k] ? shApplied[k] : "?"), shConsumed[k], (shWasteKnown[k] ? shWasted[k] : "?") > "/dev/stderr"
    shOpen[k] = 0
}
function shield_aura(ev,   spell, owner, k, amt) {
    if (strip($13) != "BUFF") return
    spell = $10 + 0
    if (!(spell in SHIELD)) return
    if (passive_stale()) return
    owner = actor($2, $4); if (owner == "") return
    k = cur SUBSEP $6 SUBSEP spell SUBSEP $2
    amt = (NF >= 14) ? $14 + 0 : ""
    if (ev == "SPELL_AURA_APPLIED") {
        if (shOpen[k]) {
            if (shRemKnown[k]) { shWasted[k] += shRem[k]; shWasteKnown[k] = 1 }
            sh_close(k)
        }
        sh_open(k, owner, spell, amt + 0, amt != "", 0)
    } else if (ev == "SPELL_AURA_REFRESH") {
        if (!shOpen[k] || amt == "") return
        if (shRemKnown[k]) {
            if (amt > shRem[k]) shApplied[k] += amt - shRem[k]
            else if (amt < shRem[k]) { shWasted[k] += shRem[k] - amt; shWasteKnown[k] = 1 }
        }
        shRem[k] = amt; shRemKnown[k] = 1
    } else {                                              # SPELL_AURA_REMOVED
        if (!shOpen[k]) return
        if (amt != "") {
            if (shRemKnown[k] && shRem[k] != amt) {
                printf "check.awk: shield %s remaining %d != REMOVED trailer %d at %s\n", k, shRem[k], amt, ts > "/dev/stderr"
                shieldBad = 1
            }
            shWasted[k] += amt; shWasteKnown[k] = 1
        } else if (shRemKnown[k]) { shWasted[k] += shRem[k]; shWasteKnown[k] = 1 }
        sh_close(k)
    }
}
function shield_absorb(dst, spell, ag, owner, amt,   k) {
    k = cur SUBSEP dst SUBSEP spell SUBSEP ag
    if (!shOpen[k]) { sh_open(k, owner, spell, 0, 0, amt); return }
    shConsumed[k] += amt
    if (shRemKnown[k]) {
        if (amt > shRem[k]) { if (shKnown[k]) shApplied[k] += amt - shRem[k]; shRem[k] = 0 }
        else shRem[k] -= amt
    }
}
# A *_MISSED line: count 1 on the friendly destination; BLOCK's amount and ABSORB's
# amountMissed are PREVENTED damage. A miss with no open segment (cur == 0) is
# dropped — R17: a miss never opens a segment.
# R17 mirror of Meter::open_segment_for_passive: a *_MISSED line or a stagger
# SPELL_ABSORBED writes only into an OPEN segment that is not past the trash gap
# (it is not combat, so it never opens, extends or splits one — but it must not
# be credited to a pull the next hit is about to split away from either).
function passive_stale() {
    if (cur == 0 || segEnd[cur] != "") return 1
    if (segKind[cur] == "Trash" && lastCombat != "" && now - lastCombat > TRASH_GAP) return 1
    return 0
}

# ---- R7 amendment (2026-10-02), the COMBAT clock rates run on. An ENGAGEMENT
# line is a DAMAGE line between a unit the group controls — flags friendly
# (0x10) AND player-controlled (0x100) — and an enemy, flags hostile (0x40) or
# neutral (0x20), either way round. Never a passive *_MISSED line (a dodge, a
# hit a shield took whole): those may not extend a segment, and the clock only
# sums stretches inside first..last, so a trash clock never outruns its R7
# duration. Per segment, the stretches between consecutive engagement lines are
# summed, a stretch longer than ENGAGE_QUIET left out (the walk to the next
# pack); heals and HoT ticks keep a segment OPEN (R4) but never run this clock.
# In END: an encounter's clock is its R7 duration, a trash segment's this sum
# (its R7 duration when it saw no engagement line at all), and `dps` divides by
# it.
function engaged(sf, df,   s, d) {
    s = strtonum(sf); d = strtonum(df)
    return (and(s, 0x110) == 0x110 && and(d, 0x60) != 0) || (and(s, 0x60) != 0 && and(d, 0x110) == 0x110)
}
function engage(   g) {
    if (!cur) return
    if (cur in engLast) {
        g = now - engLast[cur]
        if (g <= 0) return
        if (g <= ENGAGE_QUIET) engMs[cur] += g
    }
    engLast[cur] = now
}

# ---- R22 amendment (2026-10-02), FRIENDLY FIRE: a hit on a friendly GUID
# (`Player-` / `Pet-`) that is not the attacker itself, from a unit on the
# victim's own side — both flags player-controlled (0x100) and sharing a
# reaction bit, friendly (0x10) or hostile (0x40) — or from a unit a PLAYER
# SUMMONED (SPELL_SUMMON only, followed up the chain) whose own flags, as last
# seen (`uflags`), share that reaction bit with a player-controlled victim: a
# totem's last lines carry a neutral NPC's 0xa28, and the summoner's side is
# what tells our Spirit Link Totem from an arena enemy's totem hitting us.
# Spirit Link Totem's redistribution is the case in the wild. Never `damage`;
# reported as `friendly_fire`; the victim's `taken` still counts it.
function summoner(g,   i) {
    for (i = 0; i < 8 && ((g SUBSEP epoch) in summoned); i++) g = summoned[g SUBSEP epoch]
    return g
}
function friendly_fire(sguid, sflags, dguid, dflags,   s, d, top) {
    if (!friendlyGuid(dguid) || sguid == dguid) return 0
    s = strtonum(sflags); d = strtonum(dflags)
    if (and(s, d, 0x100) && and(s, d, 0x50)) return 1
    top = summoner(sguid)
    return and(d, 0x100) && top != sguid && friendlyGuid(top) && (top in uflags) && and(strtonum(uflags[top]), d, 0x50)
}

# ---- R26 (step 3), the ATTACKER's side of a miss: a *_MISSED line by one of
# ours (a player by flag, a pet folded) that is not its own target, through the
# passive gate — Miss % beside the ability's hits. Kept apart from `note`.
function dealt_miss(sguid, sflags, dguid,   a) {
    if (passive_stale()) return
    if (sguid == dguid) return
    a = actor(sguid, sflags); if (a == "") return
    missv[cur SUBSEP a]++
}

# ---- R26 (step 3), a DoT's uptime: a PLAYER's DEBUFF on an enemy (a
# Creature-/Vehicle- unit that is not ours through a summon) opens or closes the
# union of time that debuff (by NAME) is up on any enemy — opened by APPLIED or
# REFRESH (a refresh the segment saw no apply for opens there, never earlier),
# closed when its last target's REMOVED lands. Passive gate; an open union
# closes at the segment's close (segClose), at END.
function dot_aura(on,   k, tgt) {
    if (strip($13) != "DEBUFF") return
    if ($2 == "" || $2 == "0000000000000000" || !isPlayerFlags($4)) return
    tgt = $6
    if (tgt !~ /^(Creature|Vehicle)-/) return
    if (actor(tgt, $8) != "") return
    if (passive_stale()) return
    k = cur SUBSEP $2 SUBSEP strip($11)
    if (on) {
        if ((k SUBSEP tgt) in dotOn) return
        if (!dotN[k]) dotSince[k] = now
        dotOn[k SUBSEP tgt] = 1; dotN[k]++
    } else if ((k SUBSEP tgt) in dotOn) {
        delete dotOn[k SUBSEP tgt]; dotN[k]--
        if (dotN[k] == 0) dotMs[k] += now - dotSince[k]
    }
}

function missed(dguid, dflags, kind, amt,   t) {
    if (passive_stale()) return
    if (!friendlyGuid(dguid)) return                       # R17's universe, like taken()
    t = actor(dguid, dflags); if (t == "") return
    note(cur, t, "misses", 1)
    if (kind == "BLOCK") note(cur, t, "prevented", amt + 0)
}

# ---- R1: a *_MISSED ABSORB is a hit a shield took WHOLE — amount 0, absorbed =
# amountMissed — counted wherever a partial absorb's absorbed part is: the
# attacker's `damage` (a pet's folded, `petdamage`, a tick's `damage_periodic`;
# `self_harm` on itself, R22, and a self-sourced Stagger tick's `stagger_ticked`),
# and on a friendly victim `taken` and `absorbed` (plus its miss count and its
# taken series bucket) and the R21 cells. Never a miss on the attacker's ability.
# Passive, unlike a damage line: it never opens, extends or splits a segment.
function absorbed_whole(periodic, spell, label, amt, unmit,   a, t) {
    if (passive_stale()) return
    if (spell == 124255 && $2 == $6) {
        if (friendlyGuid($6)) { t = actor($6, $8); note(cur, t, "stagger_ticked", amt) }
    } else if (friendlyGuid($6)) {
        t = actor($6, $8)
        if (t != "") {
            note(cur, t, "taken", amt); note(cur, t, "absorbed", amt); note(cur, t, "misses", 1)
            note(cur, t, "reduced", reduced(unmit, amt))   # R17 amendment: base 0 + absorbed
            tk10[cur SUBSEP t SUBSEP int((now - segStart[cur]) / 10000)] += amt
        }
        stack_hit($6, $8, spell, label, amt)
    }
    a = actor($2, $4); if (a == "") return
    if (a == actor($6, $8)) { note(cur, a, "self_harm", amt); return }
    if (friendly_fire($2, $4, $6, $8)) { note(cur, a, "friendly_fire", amt); return }   # R22 amendment
    note(cur, a, "damage", amt)
    if ($2 != a) note(cur, a, "petdamage", amt)
    if (periodic) note(cur, a, "damage_periodic", amt)
}

# ---- R21 stacked-debuff conditioning. Per (segment, raw victim, aura id) a
# LEVEL: a DEBUFF on a friendly (actor() of the destination is a player, pets
# folded) from a source the group does NOT control (actor() of the source is
# "" — a hostile NPC or the nil environment unit; a player's own debuff on a
# player never conditions) — APPLIED = 1, *_DOSE = the trailer (the aura's
# NEW RUNNING TOTAL, both families: a REMOVED_DOSE counts DOWN), REFRESH =
# unchanged, REMOVED = gone; a dose / refresh / removal with no entry OPENS
# one at its level (the debuff predated the segment). Every aura line is
# passive (passive_stale(), never opens or extends a segment). BUFF doses
# never enter. Then every Taken hit (the same lines and the same amount as
# `taken` — a *_MISSED is not a hit) lands in ONE CELL PER OPEN DEBUFF on the
# victim, keyed (owner, damage label, damage id, aura id, level): hits, sum,
# max. A hit under two open debuffs lands in two cells. Level 0 is never a
# cell (the reader derives it from the unconditioned by-ability row). The
# ledger is segment-local: a new segment starts empty.
#
# Metrics (per player, per segment): `stack_hits` = Σ hits over cells,
# `stack_sum` = Σ sum, `stack_max` = max over cells, `stack_cells` = the
# number of distinct cells, `stack_auras` = distinct debuffs seen open. The
# per-cell table itself is gated by crates/core/tests/stacks.rs.
function debuff_aura(ev,   spell, victim, k, lvl) {
    if (strip($13) != "DEBUFF") return
    if (passive_stale()) return
    if (!friendlyGuid($6)) return                          # R17's universe, like taken()
    victim = actor($6, $8); if (victim == "") return
    if (actor($2, $4) != "") return                       # a controlled source never conditions
    spell = $10 + 0
    k = cur SUBSEP $6 SUBSEP spell
    # The death rule: a REMOVED closes the entry AT this millisecond — a hit
    # written later in the same millisecond (the killing blow: the client
    # strips a dying player's auras first) still lands at the level.
    if (ev == "SPELL_AURA_REMOVED") { if (k in dl) dlClose[k] = now; return }
    if (ev == "SPELL_AURA_APPLIED") lvl = 1
    else if (ev == "SPELL_AURA_REFRESH") { if ((k in dl) && dlClose[k] == "") return; lvl = 1 }
    else lvl = $14 + 0                                     # *_DOSE: the new running total
    if (lvl < 1) return
    dl[k] = lvl; dlClose[k] = ""
    if (!((cur SUBSEP victim SUBSEP spell) in seenAura)) { seenAura[cur SUBSEP victim SUBSEP spell] = 1; val[cur SUBSEP victim SUBSEP "stack_auras"]++ }
}
function stack_hit(dguid, dflags, dspell, dlabel, amt,   victim, k, kk, c) {
    if (!friendlyGuid(dguid)) return                       # R17's universe, like taken()
    victim = actor(dguid, dflags); if (victim == "") return
    for (k in dl) {
        split(k, kk, SUBSEP)
        if (kk[1] + 0 != cur || kk[2] != dguid) continue
        if (dlClose[k] != "" && dlClose[k] < now) { delete dl[k]; delete dlClose[k]; continue }
        c = cur SUBSEP victim SUBSEP dlabel SUBSEP dspell SUBSEP kk[3] SUBSEP dl[k]
        if (!(c in cellHits)) val[cur SUBSEP victim SUBSEP "stack_cells"]++
        cellHits[c]++; cellSum[c] += amt
        if (amt > cellMax[c]) cellMax[c] = amt
        val[cur SUBSEP victim SUBSEP "stack_hits"]++
        val[cur SUBSEP victim SUBSEP "stack_sum"] += amt
        if (amt > val[cur SUBSEP victim SUBSEP "stack_max"]) val[cur SUBSEP victim SUBSEP "stack_max"] = amt
        if (STACKS) printf "stack hit: seg %d owner %s %s(%d) aura %d level %d amount %d\n", cur, victim, dlabel, dspell, kk[3], dl[k], amt > "/dev/stderr"
    }
}

# ---- R29 (v45) the replay cut. Per closed boss pull (an ENCOUNTER_START's
# segment closed by its END, a version seam or the next START) and per
# finished keystone run (CHALLENGE_MODE_START to its END, both lines its
# own): the FLOOR — the UiMap id the advanced block reports players on most
# (pass 1; a tie to the lower id) —, every post on EVERY floor (a Player /
# Creature / Vehicle block whose health reads, max above 0, with x, y,
# facing and a map above 0) and the posts per floor, the event rows by kind
# (the boss rows: each ENCOUNTER_START and ENCOUNTER_END the cut holds), the
# placed rows by kind, the world markers (those standing on the cut's
# instance map at its start, then each placement there and each removal)
# and the units the rows number. Written from the ruling's words, never the
# Rust: the advanced block starts at off12 on a spell-shaped line (SPELL_,
# RANGE_, DAMAGE_SHIELD, DAMAGE_SPLIT, every _SUPPORT), else at off9;
# hostility is learned from the lines the parser models alone. A cut is a
# context `s`: a pull's segment number, a run's "K<n>".
function r_pk() { return (ev ~ /^(SPELL_|RANGE_|DAMAGE_SHIELD|DAMAGE_SPLIT)/ || ev ~ /_SUPPORT$/) ? 13 : 10 }
function r_num(s) { return s ~ /^-?[0-9]+(\.[0-9]+)?$/ }
function r_block(pk) {
    rguid = ""; rmap = 0
    if (ev == "SPELL_ABSORBED" || ev == "COMBATANT_INFO") return 0
    if ($pk !~ /^(Player|Pet|Creature|Vehicle|GameObject|BattlePet|Vignette)-/) return 0
    if ($(pk + 3) + 0 <= 0) return 0
    if (!r_num($(pk + 14)) || !r_num($(pk + 15)) || !r_num($(pk + 17))) return 0
    if ($(pk + 16) !~ /^[0-9]+$/ || $(pk + 16) + 0 == 0) return 0
    rguid = $pk; rmap = $(pk + 16) + 0
    return 1
}
function r_ours(g) { return g ~ /^(Player|Pet)-/ }
# A hit's source is anything but one of ours: a player or a pet, or a unit one
# of ours SUMMONED (R22's chain — a totem's last lines carry a neutral NPC's
# 0xa28, so no flag tells it); never the nil unit, whatever flags it wears.
function r_friend(g,   top) {
    if (r_ours(g)) return 1
    if (g == "0000000000000000" || g !~ /-/) return 0
    top = summoner(g)
    return top != g && r_ours(top)
}
function r_side(s, g, f) { if (g ~ /-/ && and(strtonum(f), 0x40)) rH[s SUBSEP g] = 1 }
function r_unit(s, g) { if (!((s SUBSEP g) in rU)) { rU[s SUBSEP g] = 1; rv[s SUBSEP "replay_units"]++ } }
function r_src(s, g) { if (g != "" && g != "0000000000000000") r_unit(s, g) }
function r_target(s, g) { if (g != "0000000000000000" && g ~ /-/) r_unit(s, g) }
function r_hostile(s, g) { return !r_ours(g) && ((s SUBSEP g) in rH) }
function r_count(s, kind) { rv[s SUBSEP "replay_" kind]++ }
function r_place(s, item) { rP[s SUBSEP (++rPn[s])] = item }
# The most-voted floor of `votes` (pass 1's, keyed (n, map)) for cut n.
function r_floor(votes, n,   k, kk, best, fl) {
    best = 0; fl = 0
    for (k in votes) {
        split(k, kk, SUBSEP)
        if (kk[1] != n) continue
        if (votes[k] > best || (votes[k] == best && kk[2] + 0 < fl)) { best = votes[k]; fl = kk[2] + 0 }
    }
    return fl
}
# One line of an open cut `s`: the hostility it teaches, its post (on any
# floor), its event row, and a placed candidate (resolved when the cut
# closes, once every other row has numbered its units).
function r_line(s) {
    if (ev ~ RBOTH) { r_side(s, $2, $4); r_side(s, $6, $8) }
    else if (ev ~ /^SPELL_EMPOWER_/) r_side(s, $2, $4)
    else if (ev == "UNIT_DIED" || ev == "UNIT_DESTROYED") r_side(s, $6, $8)
    if (r_block(r_pk()) && rguid ~ /^(Player|Creature|Vehicle)-/) {
        rPosts[s SUBSEP rguid]++; rMapP[s SUBSEP rmap]++; r_count(s, "posts"); r_unit(s, rguid)
    }
    if (ev == "UNIT_DIED") {
        if ($6 ~ /^Player-/) { if ($10 != "1") { r_count(s, "death"); r_unit(s, $6) } }
        else if ($6 ~ /^(Creature|Vehicle)-/) { r_count(s, "npc_died"); r_unit(s, $6) }
    } else if (ev == "SPELL_RESURRECT") {
        if ($6 ~ /^Player-/) { r_count(s, "rez"); r_unit(s, $6); r_src(s, $2) }
    } else if ((ev == "SPELL_CAST_START" || ev == "SPELL_CAST_SUCCESS") && r_hostile(s, $2)) {
        r_count(s, ev == "SPELL_CAST_START" ? "cast_start" : "cast_success"); r_unit(s, $2); r_target(s, $6)
    } else if (ev == "SPELL_INTERRUPT" && !r_ours($6) && r_ours($2)) {
        r_count(s, "interrupt"); r_unit(s, $6); r_unit(s, $2)
    } else if (ev ~ /^SPELL_CAST_(START|SUCCESS|FAILED)$/ && $2 ~ /^Player-/) {
        r_count(s, ev == "SPELL_CAST_START" ? "pcast_start" : (ev == "SPELL_CAST_SUCCESS" ? "pcast_success" : "pcast_failed"))
        r_unit(s, $2); r_target(s, $6)
    } else if (ev ~ /^(SPELL_DAMAGE|SPELL_PERIODIC_DAMAGE|RANGE_DAMAGE|SPELL_BUILDING_DAMAGE|DAMAGE_SHIELD)$/) {
        # A hit makes a row only where its victim's own block put them — on
        # whatever floor.
        if ($6 ~ /^Player-/ && !r_friend($2) && $10 + 0 != 0 && r_block(13) && rguid == $6) {
            r_count(s, "hit"); r_unit(s, $6); r_src(s, $2)
        }
    } else if (ev ~ /^(SPELL_MISSED|SPELL_PERIODIC_MISSED|RANGE_MISSED|DAMAGE_SHIELD_MISSED)$/) {
        if ($6 ~ /^Player-/ && !r_friend($2)) { r_count(s, "hit"); r_unit(s, $6); r_src(s, $2) }
    } else if (ev == "SPELL_AURA_APPLIED" || ev == "SPELL_AURA_REMOVED") {
        if ($6 ~ /^Player-/ && ($2 == "0000000000000000" || r_hostile(s, $2))) {
            r_count(s, ev == "SPELL_AURA_APPLIED" ? "debuff_applied" : "debuff_removed"); r_unit(s, $6); r_src(s, $2)
        }
    } else if (ev == "SPELL_AURA_APPLIED_DOSE" || ev == "SPELL_AURA_REMOVED_DOSE") {
        if ($6 ~ /^Player-/ && $14 ~ /^[0-9]+$/ && ($2 == "0000000000000000" || r_hostile(s, $2))) {
            r_count(s, "debuff_dose"); r_unit(s, $6); r_src(s, $2)
        }
    }
    if (ev == "SPELL_CAST_SUCCESS" && $2 ~ /^Player-/ && (($10 + 0) in PLACE)) r_place(s, "cast" SUBSEP $2)
    else if ((ev == "SPELL_SUMMON" || ev == "SPELL_CREATE") && $2 ~ /^Player-/ && (($10 + 0) in PLACE))
        r_place(s, "summon" SUBSEP $2 SUBSEP (($6 ~ /-/ && $6 != "0000000000000000") ? $6 : ""))
    else if (ev == "SPELL_AURA_APPLIED" && (($10 + 0) in TELL)) r_place(s, "aura" SUBSEP $6)
    else if ((ev == "SPELL_HEAL" || ev == "SPELL_PERIODIC_HEAL") && (($10 + 0) in TELL)) {
        if (r_block(13) && rguid == $6) r_place(s, "heal" SUBSEP $6)
    } else if (ev == "UNIT_DIED" || ev == "UNIT_DESTROYED") r_place(s, "gone" SUBSEP $6)
}
# A world marker line, for an open cut `s` on instance map `map`: a
# placement there, a placement elsewhere of a marker standing there (it
# left: a removal row), any removal.
function r_marker(s, map) {
    if (ev == "WORLD_MARKER_REMOVED") r_count(s, "markers")
    else if ($2 + 0 == map || ((map SUBSEP ($3 + 0)) in mOn)) r_count(s, "markers")
}
# A closed cut's rows: its placed candidates resolved in line order (a unit
# only they name is numbered last: a summon's unit, a gone one; a touch by
# aura counts only a unit numbered already, a heal's wherever its target
# stood), then the counts, its posts per floor (by id) and each player's.
function r_emit(s, head,   i, it, n2, k, kk, rpl, rmp) {
    for (i = 1; i <= rPn[s]; i++) {
        split(rP[s SUBSEP i], it, SUBSEP)
        if (it[1] == "cast") { r_unit(s, it[2]); r_count(s, "placed_cast") }
        else if (it[1] == "summon") {
            r_unit(s, it[2])
            if (it[3] != "") { rSum[s SUBSEP it[3]] = 1; r_unit(s, it[3]) }
            r_count(s, "placed_summon")
        }
        else if (it[1] == "aura") { if ((s SUBSEP it[2]) in rU) r_count(s, "placed_touch") }
        else if (it[1] == "heal") r_count(s, "placed_touch")
        else if (it[1] == "gone") { if ((s SUBSEP it[2]) in rSum) { r_unit(s, it[2]); r_count(s, "placed_gone") } }
    }
    rv[s SUBSEP "replay_floor"] = rFl[s]
    for (i = 1; i in rnames; i++)
        printf "%s\t*\treplay_%s\t%d\n", head, rnames[i], rv[s SUBSEP "replay_" rnames[i]] + 0
    n2 = 0
    for (k in rMapP) { split(k, kk, SUBSEP); if (kk[1] == s) rmp[++n2] = kk[2] + 0 }
    asort(rmp)
    for (i = 1; i <= n2; i++)
        printf "%s\t%d\treplay_map_posts\t%d\n", head, rmp[i], rMapP[s SUBSEP rmp[i]]
    n2 = 0
    for (k in rPosts) { split(k, kk, SUBSEP); if (kk[1] == s && kk[2] ~ /^Player-/) rpl[++n2] = kk[2] }
    asort(rpl)
    for (i = 1; i <= n2; i++)
        printf "%s\t%s\treplay_posts\t%d\n", head, rpl[i], rPosts[s SUBSEP rpl[i]]
}

BEGIN {
    FPAT = "([^,]*)|(\"[^\"]*\")"
    OFS = "\t"
    # R2: self-absorbs that are not healing (R17: reported as `stagger` on the defender)
    excl[114556] = 1; excl[31850] = 1; excl[31230] = 1; excl[115069] = 1
    # CC spells present in the fixture (contract: small built-in list, exactness not gated)
    cc[5246] = 1     # Intimidating Shout (fear)
    cc[117526] = 1   # Binding Shot (root/stun)
    TRASH_GAP = 60000
    ENGAGE_QUIET = 30000   # R7 amendment: the combat clock's quiet cut
    # R18 role-spell table — the FIXTURES' ids only, hard-coded (aura id =
    # the buff the log applies, never the cast id). The Rust table is
    # crates/core/src/role_spells.rs (generated, curated membership); these
    # must be a SUBSET of it. The gate is on the meter's numbers, not the table.
    ROLE[132404] = "ActiveMitigation"   # Shield Block
    ROLE[871]    = "Defensive"          # Shield Wall
    ROLE[342246] = "Defensive"          # Alter Time
    ROLE[11426]  = "Defensive"          # Ice Barrier (taken.txt + shields.txt: v34 curates the mage barriers)
    ROLE[45438]  = "Defensive"          # Ice Block   (taken.txt: curated census-exempt, v34)
    ROLE[33206]  = "External"           # Pain Suppression
    ROLE[47788]  = "External"           # Guardian Spirit
    ROLE[10060]  = "External"           # Power Infusion
    ROLE[80353]  = "External"           # Time Warp
    ROLE[395152] = "SupportBuff"        # Ebon Might
    ROLE[410089] = "SupportBuff"        # Prescience
    ROLE[190319] = "Cooldown"           # Combustion
    ROLE[77535]  = "ActiveMitigation"   # Blood Shield (shields.txt: an R18 AM span AND an R20 shield)
    ROLE[195181] = "ActiveMitigation"   # Bone Shield  (shields.txt: an AM span, never a shield)
    # R20 absorb-spell set — the FIXTURES' shield aura ids only (the Rust table
    # is generated: absorb_spells.rs, EffectAura 69); a subset of it.
    SHIELD[17]    = 1                   # Power Word: Shield
    SHIELD[11426] = 1                   # Ice Barrier
    SHIELD[77535] = 1                   # Blood Shield
    # MUST be initialised numerically: pass 1 and pass 2 both build `owner` keys as
    # (guid SUBSEP epoch). An uninitialised epoch is "" in pass 1 but 0 after the
    # pass-2 reset, and "guid\0" != "guid\0"0 — pet attribution silently vanishes.
    epoch = 0
    # R29 (v45) the replay cut's placed spells — the FIXTURES' ids only (the
    # Rust table is the encounter rubric's generated `placed` table, of which
    # these are a subset): what places something, and what tells where.
    PLACE[111771] = 1   # Demonic Gateway
    PLACE[192077] = 1   # Wind Rush Totem
    PLACE[48018]  = 1   # Demonic Circle
    TELL[113942]  = 1   # Demonic Gateway's trip
    TELL[192082]  = 1   # Wind Rush
    TELL[145629]  = 1   # Anti-Magic Zone
    TELL[73921]   = 1   # Healing Rain
    # The lines the parser models with a source and a destination: what R29
    # learns hostility from (an unmodelled line — a swing's _LANDED twin, a
    # drain — teaches nothing).
    RBOTH = "^(SWING_DAMAGE|SPELL_DAMAGE|SPELL_PERIODIC_DAMAGE|RANGE_DAMAGE|SPELL_BUILDING_DAMAGE|DAMAGE_SHIELD|ENVIRONMENTAL_DAMAGE|SWING_MISSED|SPELL_MISSED|SPELL_PERIODIC_MISSED|RANGE_MISSED|DAMAGE_SHIELD_MISSED|SPELL_HEAL|SPELL_PERIODIC_HEAL|SPELL_ABSORBED|SPELL_INTERRUPT|SPELL_AURA_APPLIED|SPELL_AURA_REMOVED|SPELL_AURA_REFRESH|SPELL_AURA_APPLIED_DOSE|SPELL_AURA_REMOVED_DOSE|SPELL_DISPEL|SPELL_STOLEN|SPELL_CAST_SUCCESS|SPELL_CAST_START|SPELL_CAST_FAILED|SPELL_ENERGIZE|SPELL_PERIODIC_ENERGIZE|SPELL_CREATE|SPELL_RESURRECT|SPELL_INSTAKILL|SPELL_SUMMON|SPELL_DAMAGE_SUPPORT|SPELL_PERIODIC_DAMAGE_SUPPORT|RANGE_DAMAGE_SUPPORT|SWING_DAMAGE_LANDED_SUPPORT|SPELL_HEAL_SUPPORT|SPELL_PERIODIC_HEAL_SUPPORT)$"
}

{
    # epoch is advanced by both passes; reset it when pass 2 begins or the two
    # passes disagree about which epoch a pet's owner was recorded in.
    if (FNR == 1 && NR != FNR) epoch = 0

    i = index($0, "  ")
    if (i == 0) { blanks++; next }          # blank / no-timestamp line
    ts = substr($0, 1, i - 1)
    rest = substr($0, i + 2)
    $0 = rest
    ev = $1
    now = tsms(ts)
}

# ---------------------------------------------------------------- pass 1: owners
FNR == NR {
    # R29: the floor votes, per boss pull and per keystone run in file order.
    if (ev == "ENCOUNTER_START") { rk1++; rin1 = 1 }
    else if (ev == "ENCOUNTER_END" || ev == "COMBAT_LOG_VERSION") rin1 = 0
    else if (rin1 && r_block(r_pk()) && rguid ~ /^Player-/) rvote[rk1 SUBSEP rmap]++
    if (ev == "CHALLENGE_MODE_START") { kk1++; kin1 = 1 }
    else if (ev == "CHALLENGE_MODE_END" || ev == "COMBAT_LOG_VERSION") kin1 = 0
    else if (kin1 && r_block(r_pk()) && rguid ~ /^Player-/) kvote[kk1 SUBSEP rmap]++
    # R6: a COMBAT_LOG_VERSION after the first line is a hard boundary.
    if (ev == "COMBAT_LOG_VERSION") { if (seenVersion) epoch++; seenVersion = 1; next }
    if (ev == "SPELL_SUMMON") owner[$6 SUBSEP epoch] = $2
    # SWING_DAMAGE advanced block describes the SOURCE; block offset 1 = owner_guid
    # => absolute offset 10 => awk $11
    else if (ev == "SWING_DAMAGE" && NF >= 38 && $11 != "0000000000000000" && $11 != "")
        owner[$2 SUBSEP epoch] = $11
    next
}

# ---- R6 hard boundary: close any open segment, advance the epoch (resets owners)
ev == "COMBAT_LOG_VERSION" {
    if (seen2) {
        epoch++
        if (cur && segEnd[cur] == "") segEnd[cur] = now
        cur = 0
        lastCombat = ""
    }
    seen2 = 1
    next
}

# ---------------------------------------------------------------- pass 2: totals
{
    # R22 amendment: every unit's flags as last seen, for friendly_fire()'s
    # summoner-side test (the nil unit's are meaningless and never asked).
    if ($2 ~ /^(Player|Pet|Creature|Vehicle)-/ && $4 ~ /^0x/) uflags[$2] = $4
    if ($6 ~ /^(Player|Pet|Creature|Vehicle)-/ && $8 ~ /^0x/) uflags[$6] = $8
    isCombat = 0
    if (ev == "SWING_DAMAGE" || ev == "SPELL_DAMAGE" || ev == "SPELL_PERIODIC_DAMAGE" ||
        ev == "RANGE_DAMAGE" || ev == "ENVIRONMENTAL_DAMAGE" || ev == "SPELL_HEAL" ||
        ev == "SPELL_PERIODIC_HEAL" || ev == "SPELL_ABSORBED") isCombat = 1
    # R2/R17 lockstep with the Rust scanner (index.rs is_combat): a SPELL_ABSORBED
    # whose absorb spell is one of the NON_HEALING_ABSORBS (stagger, cheat-death)
    # is NOT combat — it never opens, extends or gap-splits a segment. Same
    # arity discrimination as the R2/R3 block below.
    if (ev == "SPELL_ABSORBED") {
        if (NF == 22) asp = $17 + 0; else if (NF == 19) asp = $14 + 0; else asp = -1
        if (asp in excl) isCombat = 0
    }
    # R17: *_MISSED is never combat — it records into an already-open segment only
    # and never extends one (the index scanner ignores it; lockstep).
    # ---- R28 (v44) the power series: a line whose ADVANCED block describes a
    # PLAYER (block offset 0 is a `Player-` guid; the block starts at off12 on a
    # spell-shaped line — SPELL_ / RANGE_ / DAMAGE_SHIELD, (v45) DAMAGE_SPLIT and every _SUPPORT —
    # else at off9; its max health, block offset 3, above 0) and carries a pool
    # (block offsets 10–12: type, current, max — the first of an `a|b` pair —
    # max above 0) writes the player's second of that type: the LAST report of
    # a second wins, the type keeps the largest max. Raw guid, never folded (a
    # pet's pool is its own). SPELL_ABSORBED has no advanced block. Passive, and
    # read BEFORE this line can open or extend a segment (the Rust reads the
    # advanced block ahead of the event), so a line past a pull's end or the
    # trash gap lands nowhere.
    if (ev != "SPELL_ABSORBED") {
        pk = (ev ~ /^(SPELL_|RANGE_|DAMAGE_SHIELD|DAMAGE_SPLIT)/ || ev ~ /_SUPPORT$/) ? 13 : 10
        if ($pk ~ /^Player-/ && $(pk + 3) + 0 > 0 && !passive_stale()) {
            pt = $(pk + 10); pc = $(pk + 11); pm = $(pk + 12)
            sub(/\|.*/, "", pt); sub(/\|.*/, "", pc); sub(/\|.*/, "", pm)
            if (pt ~ /^[0-9]+$/ && pc ~ /^[0-9]+$/ && pm ~ /^[0-9]+$/ && pm + 0 > 0) {
                psec = int((now - segStart[cur]) / 1000)
                powv[cur SUBSEP $pk SUBSEP pt SUBSEP psec] = pc + 0
                if (pm + 0 > powm[cur SUBSEP $pk SUBSEP pt]) powm[cur SUBSEP $pk SUBSEP pt] = pm + 0
            }
        }
    }
}

# ---- R29 the world markers: the standing set over the whole log (a marker
# number is one object: a placement moves it to its map, off any other; a
# removal takes it off every map), counted while a boss pull or a keystone
# run is open (`r_marker`).
ev == "WORLD_MARKER_PLACED" {
    if (cur && (cur in rEnc)) r_marker(cur, rMap[cur])
    if (kc != "") r_marker(kc, rMap[kc])
    for (k in mOn) { split(k, kk, SUBSEP); if (kk[2] + 0 == $3 + 0) delete mOn[k] }
    mOn[($2 + 0) SUBSEP ($3 + 0)] = 1
    next
}
ev == "WORLD_MARKER_REMOVED" {
    if (cur && (cur in rEnc)) r_marker(cur, rMap[cur])
    if (kc != "") r_marker(kc, rMap[kc])
    for (k in mOn) { split(k, kk, SUBSEP); if (kk[2] + 0 == $2 + 0) delete mOn[k] }
    next
}

# ---- R29 one line of an open boss pull, and of an open keystone run (its
# START and END are its own first and last lines, read as no post or row).
cur && (cur in rEnc) && ev != "ENCOUNTER_START" && ev != "ENCOUNTER_END" { r_line(cur) }
kc != "" && ev !~ /^(ENCOUNTER|CHALLENGE_MODE)_(START|END)$/ { r_line(kc) }

# ---- R29 a keystone run opens its replay cut at its CHALLENGE_MODE_START —
# its floor (pass 1's votes), its instance map (the START's), the markers
# standing there — and closes it at its END (timed or not, as the END says).
ev == "CHALLENGE_MODE_START" {
    nk++; kc = "K" nk
    kName[kc] = strip($2) " +" ($5 + 0); kStart[kc] = now
    rMap[kc] = $3 + 0; rFl[kc] = r_floor(kvote, nk)
    for (k in mOn) { split(k, kk, SUBSEP); if (kk[1] + 0 == rMap[kc]) r_count(kc, "markers") }
    next
}
ev == "CHALLENGE_MODE_END" {
    if (kc != "") { kEnd[kc] = now; kOk[kc] = ($3 + 0 == 1) ? "timed" : "over"; kc = "" }
    next
}

ev == "ENCOUNTER_START" {
    if (cur && segEnd[cur] == "") segEnd[cur] = now
    newSeg("Encounter", strip($3), now, $2 + 0, $4 + 0)
    # R29: the pull opens its replay cut — its floor (pass 1's votes), its
    # instance map, the markers standing there — with its boss row, as an
    # open keystone run takes one.
    rk2++; rEnc[cur] = 1; rMap[cur] = $6 + 0; rFl[cur] = r_floor(rvote, rk2)
    for (k in mOn) { split(k, kk, SUBSEP); if (kk[1] + 0 == rMap[cur]) r_count(cur, "markers") }
    r_count(cur, "boss_engaged")
    if (kc != "") r_count(kc, "boss_engaged")
    encStart = now
    next
}

ev == "ENCOUNTER_END" {
    # R29: the boss row a pull ends with, and an open keystone run's.
    if (cur && (cur in rEnc) && segEnd[cur] == "") r_count(cur, ($6 + 0 == 1) ? "boss_killed" : "boss_wiped")
    if (kc != "") r_count(kc, ($6 + 0 == 1) ? "boss_killed" : "boss_wiped")
    if (cur) { segEnd[cur] = now; segOk[cur] = ($6 + 0 == 1) ? "kill" : "wipe" }
    cur = 0
    next
}

# open / roll a Trash segment (R4: encounters close exactly at ENCOUNTER_END)
isCombat {
    if (cur == 0 || (segKind[cur] == "Trash" && lastCombat != "" && now - lastCombat > TRASH_GAP))
        newSeg("Trash", "Trash", now)
    lastCombat = now
    if (segFirst[cur] == "") segFirst[cur] = now
    segLast[cur] = now
}

# R22 amendment: "one of ours SUMMONED" is known from the SPELL_SUMMON on, never
# before it (pass 1's `owner` map is the whole file's, for R5's pets-act-early).
ev == "SPELL_SUMMON" { summoned[$6 SUBSEP epoch] = $2 }

# ---- R1 damage: amount = base_amount + absorbed-field - overkill (2026-10-02:
# the log's amount includes the part past the victim's last health point, and
# the game's own meter counts none of it); extra = overkill clamped >=0, still
# reported. Every ledger the hit reaches takes the same amount — `taken`, the
# R21 cells, a stagger tick's `stagger_ticked`, `self_harm`, `friendly_fire`.
# SWING_DAMAGE only (LANDED is the same swing); *_SUPPORT and DAMAGE_SPLIT excluded.
# R17: the same event is recorded a second time on its DESTINATION (`taken`).
ev == "SWING_DAMAGE" {
    ok  = ($31 + 0 > 0) ? $31 + 0 : 0  # off30 overkill
    if (engaged($4, $8) && !friendly_fire($2, $4, $6, $8)) engage()   # R7 amendment (friendly fire never engages)
    taken($6, $8, $29 - ok, $35 + 0, $34 + 0, $30 + 0, $29 + 0)   # R17: off28 base, off34 absorbed, off33 blocked, off29 unmitigated
    stack_hit($6, $8, 0, "Melee", $29 + $35 - ok)  # R21
    a = actor($2, $4); if (a == "") next
    amt = $29 + $35 - ok               # off28 base_amount + off34 absorbed - overkill
    if (a == actor($6, $8)) { note(cur, a, "self_harm", amt) }   # R22
    else if (friendly_fire($2, $4, $6, $8)) { note(cur, a, "friendly_fire", amt) }   # R22 amendment
    else {
        note(cur, a, "damage", amt); note(cur, a, "overkill", ok)
        if ($2 != a) note(cur, a, "petdamage", amt)
    }
    pname[a] = pname[a]
    next
}

ev == "SPELL_DAMAGE" || ev == "SPELL_PERIODIC_DAMAGE" || ev == "RANGE_DAMAGE" {
    if (NF != 42) next                 # truncated/malformed
    ok  = ($34 + 0 > 0) ? $34 + 0 : 0  # off33 overkill
    if (engaged($4, $8) && !friendly_fire($2, $4, $6, $8)) engage()   # R7 amendment (friendly fire never engages)
    # R17: a self-sourced Stagger tick (124255, src == dst) re-deals damage the
    # staggered hit already had Taken in full: excluded from `taken`, tallied as
    # `stagger_ticked`. R22: it is also NOT damage done — a self-sourced tick
    # is self-harm, tallied below as `self_harm` instead of `damage`.
    if ($10 + 0 == 124255 && $2 == $6) { if (friendlyGuid($6)) { t = actor($6, $8); note(cur, t, "stagger_ticked", $32 - ok) } }
    else { taken($6, $8, $32 - ok, $38 + 0, $37 + 0, $33 + 0, $32 + 0); stack_hit($6, $8, $10 + 0, strip($11), $32 + $38 - ok) }   # off31 base, off32 unmitigated, off37 absorbed, off36 blocked; R21
    a = actor($2, $4); if (a == "") next
    amt = $32 + $38 - ok               # off31 base_amount + off37 absorbed - overkill
    if (a == actor($6, $8)) { note(cur, a, "self_harm", amt) }   # R22
    else if (friendly_fire($2, $4, $6, $8)) { note(cur, a, "friendly_fire", amt) }   # R22 amendment
    else {
        note(cur, a, "damage", amt); note(cur, a, "overkill", ok)
        if ($2 != a) note(cur, a, "petdamage", amt)
        if (ev == "SPELL_PERIODIC_DAMAGE") note(cur, a, "damage_periodic", amt)   # R26: a tick
    }
    next
}

# R17: ENVIRONMENTAL_DAMAGE — no spell block; envType sits at off28 AFTER the
# (target) advanced block, then the usual 10-field damage suffix: base off29,
# blocked off34, absorbed off35 (39 fields). The source is the nil unit, so it
# deals nothing; the destination takes it.
ev == "ENVIRONMENTAL_DAMAGE" {
    if (NF != 39) next
    ok = ($32 + 0 > 0) ? $32 + 0 : 0   # off31 overkill (R1: out of every amount)
    taken($6, $8, $30 - ok, $36 + 0, $35 + 0, $31 + 0, $30 + 0)   # off30 unmitigated
    stack_hit($6, $8, 0, strip($29), $30 + $36 - ok)   # R21: the envType is the label
    next
}

# ---- R17 misses: no damage twin. Index FORWARD from missType — the ST/AOE trailer
# on SPELL_* / SPELL_PERIODIC_* makes end-relative offsets wrong. A miss against an
# NPC (a player's spell EVADEd, a swing DODGEd by the boss…) has no friendly
# destination and is taken by nobody.
ev == "SWING_MISSED" {                       # missType off9, isOffHand off10, amount off11
    if ($10 == "ABSORB") { absorbed_whole(0, 0, "Melee", $12 + 0, $13 + 0); next }   # R1; off12 unmitigated
    dealt_miss($2, $4, $6)                     # R26: the attacker's side
    missed($6, $8, $10, $12)
    next
}
ev == "SPELL_MISSED" || ev == "SPELL_PERIODIC_MISSED" || ev == "RANGE_MISSED" ||
ev == "DAMAGE_SHIELD_MISSED" {               # missType off12, isOffHand off13, amount off14
    if ($13 == "ABSORB") { absorbed_whole(ev == "SPELL_PERIODIC_MISSED", $10 + 0, strip($11), $15 + 0, $16 + 0); next }   # R1; off15 unmitigated
    dealt_miss($2, $4, $6)                     # R26: the attacker's side
    missed($6, $8, $13, $15)
    next
}

# ---- R2 healing: effective = amount - overheal; extra = overheal
# R2 amendment (healing received): the same effective amount is recorded a
# second time on the DESTINATION as `healed_received` — from ANY source (an
# NPC's heal on a player counts, symmetric with R17 counting NPC attackers), a
# heal on a pet is its owner's, and `self_healed` is the subset with src guid ==
# dst guid. The NON_HEALING_ABSORBS exclusion applies to both sides. Absorbs are
# NOT received healing (R3: a consumed shield is damage prevented, already in
# R17's `absorbed`), and a *_HEAL_SUPPORT line is the supporter's share of a
# heal already counted here, never received healing.
ev == "SPELL_HEAL" || ev == "SPELL_PERIODIC_HEAL" {
    if (NF != 36) next
    if ($10 + 0 in excl) next
    amount = $33 + 0                   # off32 amount (INCLUDES overheal)
    over   = $34 + 0                   # off33 overheal
    habs   = $35 + 0                   # off34 absorbed — a heal-absorb ate it (R2, v44)
    t = actor($6, $8)
    if (t != "" && friendlyGuid($6)) {                     # R17's universe, like taken()
        note(cur, t, "healed_received", amount - over)
        if ($2 == $6) note(cur, t, "self_healed", amount - over)
    }
    a = actor($2, $4); if (a == "") next
    note(cur, a, "heal", amount - over); note(cur, a, "overheal", over)
    if (ev == "SPELL_PERIODIC_HEAL") note(cur, a, "heal_periodic", amount - over)   # R26: a tick
    # R2 (v44): the eaten part — healing all the same — capped at what the line
    # healed (a real line can log more absorbed than amount − overheal).
    eff = amount - over; if (eff < 0) eff = 0
    note(cur, a, "heal_absorbed", (habs < eff) ? habs : eff)
    next
}

# ---- R19 support attribution. A *_SUPPORT line is the underlying family's
# line with a 3-field spell block that is the BUFF (not the hit) and the
# supporter's bare guid appended as the LAST field ($NF) in place of the ST/AOE
# trailer. The amount is the buff's SHARE of the hit, read as logged — never
# computed from the hit. `support_given` lands on the supporter (raw guid — the
# ruling says it is a player; it needs no flags and no fold), `support_received`
# on the buffed SOURCE through the pet-owner map (a buffed pet's share is its
# owner's). A share whose buffed source is not ours (no player flags, no
# owner — an NPC ally) lands on NOBODY, given included: its damage is on no
# row, and R19's partition (Σ effective = Σ damage) must hold. Passive gate: a support line never opens, extends or splits a
# segment (it is not in pass 2's isCombat), so it records only into an open
# segment that is not past the trash gap, exactly like a miss.
#
# Every damage family is SPELL-shaped — 42 fields, amount at off31 = $32 —
# INCLUDING SWING_DAMAGE_LANDED_SUPPORT: the spell block pushes the suffix to
# the SPELL offsets, so reading it at the fixed swing offset ($29) yields the
# advanced block's ui_map_id (2287 in the fixture), and the parser's swing
# path (probing $10 for the advanced block, finding the buff's spell id) would
# yield that spell id, 395152 — never the share. The `absorbed` field ($38) is added
# as R1 does: the share of a hit the target's shield took whole is logged as amount 0
# + absorbed (support.txt l.38, the real-log shape), so the goldens depend on it.
ev == "SPELL_DAMAGE_SUPPORT" || ev == "SPELL_PERIODIC_DAMAGE_SUPPORT" ||
ev == "RANGE_DAMAGE_SUPPORT" || ev == "SWING_DAMAGE_LANDED_SUPPORT" {
    if (NF != 42) next
    if (passive_stale()) next
    sup = $NF; if (sup == "" || sup == "nil" || sup == "0000000000000000") next
    amt = $32 + $38
    # A buffed source that is not ours (an NPC ally) is no raid damage: the
    # share lands on nobody, the supporter included.
    a = actor($2, $4); if (a == "") next
    note(cur, sup, "support_given", amt)
    note(cur, a, "support_received", amt)
    next
}
# Heal support: 36 + 1 fields; the guid is appended AFTER `critical`, so the
# heal offsets do not move (amount $33, overheal $34). Effective share =
# amount - overheal, as R2 reads a heal.
ev == "SPELL_HEAL_SUPPORT" || ev == "SPELL_PERIODIC_HEAL_SUPPORT" {
    if (NF != 37) next
    if (passive_stale()) next
    sup = $NF; if (sup == "" || sup == "nil" || sup == "0000000000000000") next
    amt = ($33 + 0) - ($34 + 0)
    a = actor($2, $4); if (a == "") next
    note(cur, sup, "support_given_heal", amt)
    note(cur, a, "support_received_heal", amt)
    next
}
# SPELL_ABSORBED_SUPPORT (20 / 23 fields) is NOT a support family: its spell
# block is the buff, the underlying shield is unknowable, so the R2 exclusion
# cannot be applied. It stays Other and contributes to nothing — no arm here.

# ---- R2/R3 SPELL_ABSORBED credits the ABSORBER with healing (no overheal component)
#
# Arity is discriminated by FIELD COUNT (equivalently: presence of the damage-spell
# block), NOT by whether absorber == defender. spec.json claims the latter and is
# wrong: in the reference log 9960 of 11586 22-field lines have absorber == defender.
ev == "SPELL_ABSORBED" {
    if (NF == 22)      { ag = $13; af = $15; sp = $17 + 0; amt = $20 + 0 }
    else if (NF == 19) { ag = $10; af = $12; sp = $14 + 0; amt = $17 + 0 }
    else next
    if (sp in excl) {                  # stagger / cheat-death are not healing …
        # … but R17 reports the NON_HEALING_ABSORBS amount consumed on the
        # DEFENDER (fields 5-8 in both arities) as `stagger`. It is a subset of
        # the paired damage line's `absorbed` and is never added to `taken`.
        # This is the ONLY SPELL_ABSORBED reading on the destination side.
        # Not combat (see pass 2's isCombat): a shield line logged before the
        # pull's first hit is nobody's, exactly as in the meter.
        if (passive_stale()) next
        t = actor($6, $8); note(cur, t, "stagger", amt)
        next
    }
    a = actor(ag, af); if (a == "") next
    note(cur, a, "heal", amt); note(cur, a, "absorbheal", amt)
    shield_absorb($6, sp, ag, a, amt)  # R20: the defender is fields 5-8 in both arities
    next
}

# ---- R26 casts: a SPELL_CAST_SUCCESS by one of ours (a player by flag, a pet
# folded onto its owner) counts on the caster's row. Passive, like a miss: it
# never opens, extends or splits a segment (not in pass 2's isCombat), so a
# precast before the pull, a cast after the kill and one in the trash dead zone
# land nowhere. Kept apart from `note` so a player who only cast gets no row.
ev == "SPELL_CAST_SUCCESS" {
    if (passive_stale()) next
    a = actor($2, $4); if (a == "") next
    castv[cur SUBSEP a]++
    next
}

# ---- R27 (2026-10-08) resources: a SPELL_ENERGIZE / SPELL_PERIODIC_ENERGIZE
# landing on a PLAYER (a `Player-` destination by guid; a pet's pool is its
# own and counts for nobody) adds its amount to `energize_gained` and its
# overcap to `energize_wasted`, summed over power types. The suffix is the
# line's last four fields (`amount, overEnergize, powerType, maxPower`), with
# or without the advanced block before it. Passive, like a cast: never combat,
# so one before the pull, after its end or past the trash gap lands nowhere.
ev == "SPELL_ENERGIZE" || ev == "SPELL_PERIODIC_ENERGIZE" {
    if (passive_stale()) next
    if ($6 !~ /^Player-/) next
    engain[cur SUBSEP $6] += $(NF - 3)
    enwaste[cur SUBSEP $6] += $(NF - 2)
    next
}

# ---- R26 (2026-10-08) casts that BEGAN: a SPELL_CAST_START by one of ours,
# counted exactly like a cast (passive — never combat, so a start before the
# pull, after the kill or past the trash gap lands nowhere; an NPC's is
# nobody's). A start with no success after it is a cast that never went off.
ev == "SPELL_CAST_START" {
    if (passive_stale()) next
    a = actor($2, $4); if (a == "") next
    startv[cur SUBSEP a]++
    next
}

# ---- R26 (v44) empowered spells: a SPELL_EMPOWER_END by one of ours counts its
# release under the stage it trails (1–4; any other stage is none we name), a
# SPELL_EMPOWER_INTERRUPT counts a cancel whatever it trails. A START counts
# nothing. Passive like a cast: one before the pull, after the kill or past the
# trash gap lands nowhere; an NPC's is nobody's. No advanced block on either.
ev == "SPELL_EMPOWER_END" || ev == "SPELL_EMPOWER_INTERRUPT" {
    if (passive_stale()) next
    a = actor($2, $4); if (a == "") next
    if (ev == "SPELL_EMPOWER_INTERRUPT") { empc[cur SUBSEP a]++; next }
    if ($13 ~ /^[0-9]+$/ && $13 + 0 >= 1 && $13 + 0 <= 4) emps[cur SUBSEP a SUBSEP ($13 + 0)]++
    next
}

ev == "SPELL_INTERRUPT" { a = actor($2, $4); note(cur, a, "interrupts", 1); next }
ev == "SPELL_DISPEL"    { a = actor($2, $4); note(cur, a, "dispels", 1);    next }
# R15 (2026-10-08): a Spellsteal is a dispel too — counted alike (the index
# scanner counts SPELL_STOLEN as combat, like SPELL_DISPEL).
ev == "SPELL_STOLEN"    { a = actor($2, $4); note(cur, a, "dispels", 1);    next }

ev == "SPELL_AURA_APPLIED" {
    dot_aura(1)                                  # R26: a player's DoT on an enemy
    aura_apply(0)                                # R18: a BUFF on a player, in ROLE
    shield_aura(ev)                              # R20: a BUFF in SHIELD
    debuff_aura(ev)                              # R21: a hostile DEBUFF on a friendly
    if (strip($13) != "DEBUFF") next
    if (!(($10 + 0) in cc)) next
    a = actor($2, $4); note(cur, a, "cc", 1)
    next
}
ev == "SPELL_AURA_REFRESH" { dot_aura(1); aura_apply(1); shield_aura(ev); debuff_aura(ev); next }   # R18: APPLIED's 13-field shape
ev == "SPELL_AURA_REMOVED" { dot_aura(0); aura_remove(); shield_aura(ev); debuff_aura(ev); next }
ev == "SPELL_AURA_APPLIED_DOSE" || ev == "SPELL_AURA_REMOVED_DOSE" { debuff_aura(ev); next }   # R21: 14 fields, the trailer is the level

# Deaths: players only (a pet death is not a player death), and never a
# Hunter's Feign Death — the trailing unconsciousOnDeath field is 1 (R9)
ev == "UNIT_DIED" {
    if (!isPlayerFlags($8)) next
    if ($10 + 0 == 1) next
    note(cur, $6, "deaths", 1)
    next
}

END {
    print "segment", "kind", "name", "result", "dur_ms", "enc_id", "difficulty", "player", "metric", "value"
    # R18 read-time pass: a span still open closes at its segment's end
    # (min(end, now) − at; never negative), then the rollups: `am_uptime_ms` =
    # the per-second bitmap UNION of ActiveMitigation spans per target (exact
    # for whole-second spans, which every fixture uses — overlaps count once);
    # External ms by caster (given) and by target (received); SupportBuff ms
    # by caster, summed over its targets and spells.
    for (i = 1; i <= nspan; i++) {
        s = spanSeg[i]
        e = (spanEnd[i] != "") ? spanEnd[i] : segClose(s)
        if (e < spanAt[i]) e = spanAt[i]
        d = e - spanAt[i]
        if (spanKind[i] == "ActiveMitigation")
            for (sec = int((spanAt[i] - segStart[s]) / 1000); sec < int((e - segStart[s] + 999) / 1000); sec++)
                ambit[s SUBSEP spanTgt[i] SUBSEP sec] = 1
        if (spanKind[i] == "External") {
            val[s SUBSEP spanSrc[i] SUBSEP "externals_given_ms"] += d
            val[s SUBSEP spanTgt[i] SUBSEP "externals_received_ms"] += d
        }
        if (spanKind[i] == "SupportBuff") val[s SUBSEP spanSrc[i] SUBSEP "support_uptime_ms"] += d
    }
    for (k in ambit) { split(k, kk, SUBSEP); val[kk[1] SUBSEP kk[2] SUBSEP "am_uptime_ms"] += 1000 }
    # R26 (step 3) read-time close: a DoT union still open closes at its
    # segment's close; then each player's unions summed.
    for (k in dotN) {
        split(k, kk, SUBSEP)
        ms = dotMs[k] + (dotN[k] > 0 ? segClose(kk[1]) - dotSince[k] : 0)
        if (ms < 0) ms = 0
        dotUp[kk[1] SUBSEP kk[2]] += ms
    }
    # R20 segment-close fold: a shield still open folds with its consumed and
    # count only — no applied, no wasted, unknown += 1 (the key's segment is
    # its own, so this is the per-segment close for every segment at once).
    for (k in shOpen) if (shOpen[k]) { shKnown[k] = 0; shWasteKnown[k] = 0; sh_close(k) }
    # R28: per (segment, player) the seconds with a report, Σ of each second's
    # last report and Σ over the player's types of the largest max.
    for (k in powv) { split(k, kk, SUBSEP); powSec[kk[1] SUBSEP kk[2]]++; powSum[kk[1] SUBSEP kk[2]] += powv[k] }
    for (k in powm) { split(k, kk, SUBSEP); powMax[kk[1] SUBSEP kk[2]] += powm[k] }
    if (SHIELDS) for (r in shRowCount) { split(r, kk, SUBSEP); printf "shield row: seg %d owner %s spell %d count %d applied %d consumed %d wasted %d unknown %d\n", kk[1], kk[2], kk[3], shRowCount[r], shRowApplied[r], shRowConsumed[r], shRowWasted[r], shRowUnknown[r] > "/dev/stderr" }
    for (s = 1; s <= nseg; s++) {
        dur = (segKind[s] == "Encounter" && segEnd[s] != "") \
              ? segEnd[s] - segStart[s] \
              : (segLast[s] - segFirst[s])
        # R7 amendment: the combat clock `dps` runs on (see engage())
        comb = (segKind[s] == "Trash" && (s in engLast)) ? engMs[s] + 0 : dur
        # total damage for pct
        tot = 0
        for (k in seen) {
            split(k, kk, SUBSEP)
            if (kk[1] + 0 == s) tot += val[s SUBSEP kk[2] SUBSEP "damage"]
        }
        n = 0
        for (k in seen) {
            split(k, kk, SUBSEP)
            if (kk[1] + 0 != s) continue
            plist[++n] = kk[2]
        }
        # deterministic order: damage desc, then guid asc
        for (x = 1; x <= n; x++)
            for (y = x + 1; y <= n; y++) {
                dx = val[s SUBSEP plist[x] SUBSEP "damage"]
                dy = val[s SUBSEP plist[y] SUBSEP "damage"]
                if (dy > dx || (dy == dx && plist[y] < plist[x])) { t = plist[x]; plist[x] = plist[y]; plist[y] = t }
            }
        for (x = 1; x <= n; x++) {
            g = plist[x]
            d = val[s SUBSEP g SUBSEP "damage"] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tdamage\t%d\n",       s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, d
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\toverkill\t%d\n",     s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "overkill"] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tpetdamage\t%d\n",    s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "petdamage"] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tdps\t%.2f\n",        s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, (comb > 0 ? d / (comb / 1000.0) : 0)
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tcombat_ms\t%d\n",    s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, comb
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tpct\t%.2f\n",        s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, (tot > 0 ? 100.0 * d / tot : 0)
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\theal\t%d\n",         s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "heal"] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\toverheal\t%d\n",     s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "overheal"] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tabsorbheal\t%d\n",   s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "absorbheal"] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tinterrupts\t%d\n",   s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "interrupts"] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tcc\t%d\n",           s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "cc"] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tdispels\t%d\n",      s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "dispels"] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tdeaths\t%d\n",       s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "deaths"] + 0
            # R17 destination side — fixed shape, always emitted (zeros included)
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\ttaken\t%d\n",        s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "taken"] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tabsorbed\t%d\n",     s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "absorbed"] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tblocked\t%d\n",      s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "blocked"] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tprevented\t%d\n",    s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "prevented"] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tmisses\t%d\n",       s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "misses"] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tstagger\t%d\n",      s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "stagger"] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tstagger_ticked\t%d\n", s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "stagger_ticked"] + 0
            # R17 amendment: what armor and damage reduction took off (never taken)
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\treduced\t%d\n",      s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "reduced"] + 0
            # R22: what this actor (its pets folded in) dealt to ITSELF — held
            # off `damage`, so `damage` is what reached everyone else.
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tself_harm\t%d\n",     s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "self_harm"] + 0
            # R22 amendment: what it dealt to a teammate — held off `damage`,
            # still in the victim's `taken`.
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tfriendly_fire\t%d\n", s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "friendly_fire"] + 0
            # R19 support + the R2 amendment — fixed shape, always emitted after
            # the R17 metrics (zeros included). `effective` is DERIVED:
            # damage - support_received + support_given (never stored by the
            # meter; Σ effective over a segment = Σ damage).
            sg = val[s SUBSEP g SUBSEP "support_given"] + 0
            sr = val[s SUBSEP g SUBSEP "support_received"] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tsupport_given\t%d\n",         s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, sg
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tsupport_received\t%d\n",      s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, sr
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tsupport_given_heal\t%d\n",    s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "support_given_heal"] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tsupport_received_heal\t%d\n", s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "support_received_heal"] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\thealed_received\t%d\n",       s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "healed_received"] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tself_healed\t%d\n",           s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "self_healed"] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\teffective\t%d\n",             s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, d - sr + sg
            # R18 aura spans — fixed shape, always emitted after the R19
            # metrics (zeros included). `spans` counts role spans with the
            # player as TARGET (any kind); `taken10_0` is the first 10 s bucket
            # of the taken series (a spot check — the .md carries every bucket).
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tam_uptime_ms\t%d\n",          s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "am_uptime_ms"] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\texternals_given\t%d\n",       s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "externals_given"] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\texternals_given_ms\t%d\n",    s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "externals_given_ms"] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\texternals_received\t%d\n",    s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "externals_received"] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\texternals_received_ms\t%d\n", s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "externals_received_ms"] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tsupport_uptime_ms\t%d\n",     s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "support_uptime_ms"] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tspans\t%d\n",                 s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "spans"] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\ttaken10_0\t%d\n",             s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, tk10[s SUBSEP g SUBSEP 0] + 0
            # R20 shield ledger — fixed shape, always emitted after the R18
            # metrics. `absorb_wasted` is BLANK (not 0) when no closed shield
            # of the player's had a known waste — the meter's `None`.
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tabsorb_applied\t%d\n",        s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "absorb_applied"] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tabsorb_wasted\t%s\n",         s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, (wasteKnown[s SUBSEP g] ? val[s SUBSEP g SUBSEP "absorb_wasted"] + 0 : "")
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tshields_unknown\t%d\n",       s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "shields_unknown"] + 0
            # R21 stacked-debuff ledger — fixed shape, always emitted after the
            # R20 metrics (zeros included).
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tstack_hits\t%d\n",            s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "stack_hits"] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tstack_sum\t%d\n",             s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "stack_sum"] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tstack_max\t%d\n",             s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "stack_max"] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tstack_cells\t%d\n",           s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "stack_cells"] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tstack_auras\t%d\n",           s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "stack_auras"] + 0
            # R26 ability tree -- fixed shape, always emitted after the R21
            # metrics: casts (passive-gated), and the periodic halves of damage
            # and healing (ticks; the direct part is the total less these).
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tcasts\t%d\n",                 s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, castv[s SUBSEP g] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tcast_starts\t%d\n",           s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, startv[s SUBSEP g] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tdamage_periodic\t%d\n",       s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "damage_periodic"] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\theal_periodic\t%d\n",         s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "heal_periodic"] + 0
            # R26 (step 3): misses by the player and their pets, and the Σ of
            # their debuffs' unions up on enemies.
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tmisses_dealt\t%d\n",          s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, missv[s SUBSEP g] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tdot_uptime_ms\t%d\n",         s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, dotUp[s SUBSEP g] + 0
            # R27: power gained and lost to the cap, every power type summed.
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tenergize_gained\t%.4f\n",     s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, engain[s SUBSEP g] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tenergize_wasted\t%.4f\n",     s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, enwaste[s SUBSEP g] + 0
            # R2 (v44): the healing a heal-absorb ate (inside `heal`).
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\theal_absorbed\t%d\n",       s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, val[s SUBSEP g SUBSEP "heal_absorbed"] + 0
            # R26 (v44): empowered releases by stage, and the cancels.
            for (st = 1; st <= 4; st++)
                printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tempower_stage%d\t%d\n", s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, st, emps[s SUBSEP g SUBSEP st] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tempower_cancelled\t%d\n",   s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, empc[s SUBSEP g] + 0
            # R28 (v44): the power series — seconds reported, Σ of their
            # values, Σ of each type's largest max.
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tpower_seconds\t%d\n",       s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, powSec[s SUBSEP g] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tpower_sum\t%d\n",           s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, powSum[s SUBSEP g] + 0
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tpower_max\t%d\n",           s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, powMax[s SUBSEP g] + 0
        }
        delete plist
    }
    # R29: each closed boss pull's replay cut, then each finished keystone
    # run's ("K<n>", kind Key, its name and level, timed or over, its START to
    # its END) — `r_emit`'s rows.
    split("units posts floor hit cast_start cast_success pcast_start pcast_success pcast_failed interrupt debuff_applied debuff_removed debuff_dose death npc_died rez boss_engaged boss_killed boss_wiped placed_cast placed_summon placed_touch placed_gone markers", rnames, " ")
    for (s = 1; s <= nseg; s++) {
        if (!(s in rEnc) || segEnd[s] == "") continue
        dur = segEnd[s] - segStart[s]
        r_emit(s, s "\t" segKind[s] "\t" segName[s] "\t" segOk[s] "\t" dur "\t" segEnc[s] "\t" segDiff[s])
    }
    for (n = 1; n <= nk; n++) {
        s = "K" n
        if (kEnd[s] == "") continue
        r_emit(s, s "\tKey\t" kName[s] "\t" kOk[s] "\t" (kEnd[s] - kStart[s]) "\t\t")
    }
    if (shieldBad) exit 1
}
