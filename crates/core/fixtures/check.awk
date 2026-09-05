#!/usr/bin/env gawk -f
#
# check.awk — independent expected-value computer for wowdps fixtures.
#
# Reads a WoW advanced combat log and emits per-segment / per-player totals as a
# stable TSV. This is the VALIDATOR's own implementation of the CONTRACT.md R1-R6,
# R17, R18, R19 (+ the R2 amendment) and R20 semantics, written from the log grammar.
# It never calls, links, or consults the Rust implementation — that is the whole
# point: the Rust is graded against this, not the other way round. R18 (aura
# spans with caster and target) runs over a hard-coded copy of the FIXTURES'
# role-spell ids (ROLE, in BEGIN), never the generated Rust table; R20 (the
# shield ledger) likewise over the fixtures' absorb-spell ids (SHIELD).
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
# parts riding the damage event; the full-miss amounts go to `prevented`. The
# destination is attributed exactly like a source (players by flag, pets folded
# onto their owner; NPC destinations are nobody's).
function taken(dguid, dflags, amt, absorbed, blocked,   t) {
    t = actor(dguid, dflags); if (t == "") return
    note(cur, t, "taken", amt + absorbed)
    note(cur, t, "absorbed", absorbed)
    note(cur, t, "blocked", blocked)
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

function missed(dguid, dflags, kind, amt,   t) {
    if (passive_stale()) return
    t = actor(dguid, dflags); if (t == "") return
    note(cur, t, "misses", 1)
    if (kind == "BLOCK" || kind == "ABSORB") note(cur, t, "prevented", amt + 0)
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
    # R18 role-spell table — the FIXTURES' ids only, hard-coded (aura id =
    # the buff the log applies, never the cast id). The Rust table is
    # crates/core/src/role_spells.rs (generated, curated membership); these
    # must be a SUBSET of it. The gate is on the meter's numbers, not the table.
    ROLE[132404] = "ActiveMitigation"   # Shield Block
    ROLE[871]    = "Defensive"          # Shield Wall
    ROLE[342246] = "Defensive"          # Alter Time
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
}

ev == "ENCOUNTER_START" {
    if (cur && segEnd[cur] == "") segEnd[cur] = now
    newSeg("Encounter", strip($3), now, $2 + 0, $4 + 0)
    encStart = now
    next
}

ev == "ENCOUNTER_END" {
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

# ---- R1 damage: amount = base_amount + absorbed-field; extra = overkill clamped >=0
# SWING_DAMAGE only (LANDED is the same swing); *_SUPPORT and DAMAGE_SPLIT excluded.
# R17: the same event is recorded a second time on its DESTINATION (`taken`).
ev == "SWING_DAMAGE" {
    taken($6, $8, $29 + 0, $35 + 0, $34 + 0)   # R17: off28 base, off34 absorbed, off33 blocked
    a = actor($2, $4); if (a == "") next
    amt = $29 + $35                    # off28 base_amount + off34 absorbed
    ok  = ($31 + 0 > 0) ? $31 + 0 : 0  # off30 overkill
    note(cur, a, "damage", amt); note(cur, a, "overkill", ok)
    if ($2 != a) note(cur, a, "petdamage", amt)
    pname[a] = pname[a]
    next
}

ev == "SPELL_DAMAGE" || ev == "SPELL_PERIODIC_DAMAGE" || ev == "RANGE_DAMAGE" {
    if (NF != 42) next                 # truncated/malformed
    # R17: a self-sourced Stagger tick (124255, src == dst) re-deals damage the
    # staggered hit already had Taken in full: excluded from `taken`, tallied as
    # `stagger_ticked`. It stays damage DEALT by the monk — R1 has no self-damage
    # exclusion.
    if ($10 + 0 == 124255 && $2 == $6) { t = actor($6, $8); note(cur, t, "stagger_ticked", $32 + 0) }
    else taken($6, $8, $32 + 0, $38 + 0, $37 + 0)   # off31 base, off37 absorbed, off36 blocked
    a = actor($2, $4); if (a == "") next
    amt = $32 + $38                    # off31 base_amount + off37 absorbed
    ok  = ($34 + 0 > 0) ? $34 + 0 : 0  # off33 overkill
    note(cur, a, "damage", amt); note(cur, a, "overkill", ok)
    if ($2 != a) note(cur, a, "petdamage", amt)
    next
}

# R17: ENVIRONMENTAL_DAMAGE — no spell block; envType sits at off28 AFTER the
# (target) advanced block, then the usual 10-field damage suffix: base off29,
# blocked off34, absorbed off35 (39 fields). The source is the nil unit, so it
# deals nothing; the destination takes it.
ev == "ENVIRONMENTAL_DAMAGE" {
    if (NF != 39) next
    taken($6, $8, $30 + 0, $36 + 0, $35 + 0)
    next
}

# ---- R17 misses: no damage twin. Index FORWARD from missType — the ST/AOE trailer
# on SPELL_* / SPELL_PERIODIC_* makes end-relative offsets wrong. A miss against an
# NPC (a player's spell EVADEd, a swing DODGEd by the boss…) has no friendly
# destination and is taken by nobody.
ev == "SWING_MISSED" {                       # missType off9, isOffHand off10, amount off11
    missed($6, $8, $10, $12)
    next
}
ev == "SPELL_MISSED" || ev == "SPELL_PERIODIC_MISSED" || ev == "RANGE_MISSED" ||
ev == "DAMAGE_SHIELD_MISSED" {               # missType off12, isOffHand off13, amount off14
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
    t = actor($6, $8)
    if (t != "") {
        note(cur, t, "healed_received", amount - over)
        if ($2 == $6) note(cur, t, "self_healed", amount - over)
    }
    a = actor($2, $4); if (a == "") next
    note(cur, a, "heal", amount - over); note(cur, a, "overheal", over)
    next
}

# ---- R19 support attribution. A *_SUPPORT line is the underlying family's
# line with a 3-field spell block that is the BUFF (not the hit) and the
# supporter's bare guid appended as the LAST field ($NF) in place of the ST/AOE
# trailer. The amount is the buff's SHARE of the hit, read as logged — never
# computed from the hit. `support_given` lands on the supporter (raw guid — the
# ruling says it is a player; it needs no flags and no fold), `support_received`
# on the buffed SOURCE through the pet-owner map (a buffed pet's share is its
# owner's). Passive gate: a support line never opens, extends or splits a
# segment (it is not in pass 2's isCombat), so it records only into an open
# segment that is not past the trash gap, exactly like a miss.
#
# Every damage family is SPELL-shaped — 42 fields, amount at off31 = $32 —
# INCLUDING SWING_DAMAGE_LANDED_SUPPORT: the spell block pushes the suffix to
# the SPELL offsets, so reading it at the fixed swing offset ($29) yields the
# advanced block's ui_map_id (2287 in the fixture), and the parser's swing
# path (probing $10 for the advanced block, finding the buff's spell id) would
# yield that spell id, 395152 — never the share. The `absorbed` field ($38) is added
# as R1 does; every fixture support line carries absorbed 0, so the goldens do
# not depend on that choice.
ev == "SPELL_DAMAGE_SUPPORT" || ev == "SPELL_PERIODIC_DAMAGE_SUPPORT" ||
ev == "RANGE_DAMAGE_SUPPORT" || ev == "SWING_DAMAGE_LANDED_SUPPORT" {
    if (NF != 42) next
    if (passive_stale()) next
    sup = $NF; if (sup == "" || sup == "nil" || sup == "0000000000000000") next
    amt = $32 + $38
    note(cur, sup, "support_given", amt)
    a = actor($2, $4); if (a == "") next
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
    note(cur, sup, "support_given_heal", amt)
    a = actor($2, $4); if (a == "") next
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

ev == "SPELL_INTERRUPT" { a = actor($2, $4); note(cur, a, "interrupts", 1); next }
ev == "SPELL_DISPEL"    { a = actor($2, $4); note(cur, a, "dispels", 1);    next }

ev == "SPELL_AURA_APPLIED" {
    aura_apply(0)                                # R18: a BUFF on a player, in ROLE
    shield_aura(ev)                              # R20: a BUFF in SHIELD
    if (strip($13) != "DEBUFF") next
    if (!(($10 + 0) in cc)) next
    a = actor($2, $4); note(cur, a, "cc", 1)
    next
}
ev == "SPELL_AURA_REFRESH" { aura_apply(1); shield_aura(ev); next }   # R18: APPLIED's 13-field shape
ev == "SPELL_AURA_REMOVED" { aura_remove(); shield_aura(ev); next }

# Deaths: players only (a pet death is not a player death)
ev == "UNIT_DIED" {
    if (!isPlayerFlags($8)) next
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
    # R20 segment-close fold: a shield still open folds with its consumed and
    # count only — no applied, no wasted, unknown += 1 (the key's segment is
    # its own, so this is the per-segment close for every segment at once).
    for (k in shOpen) if (shOpen[k]) { shKnown[k] = 0; shWasteKnown[k] = 0; sh_close(k) }
    if (SHIELDS) for (r in shRowCount) { split(r, kk, SUBSEP); printf "shield row: seg %d owner %s spell %d count %d applied %d consumed %d wasted %d unknown %d\n", kk[1], kk[2], kk[3], shRowCount[r], shRowApplied[r], shRowConsumed[r], shRowWasted[r], shRowUnknown[r] > "/dev/stderr" }
    for (s = 1; s <= nseg; s++) {
        dur = (segKind[s] == "Encounter" && segEnd[s] != "") \
              ? segEnd[s] - segStart[s] \
              : (segLast[s] - segFirst[s])
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
            printf "%d\t%s\t%s\t%s\t%d\t%s\t%s\t%s\tdps\t%.2f\n",        s, segKind[s], segName[s], segOk[s], dur, segEnc[s], segDiff[s], g, (dur > 0 ? d / (dur / 1000.0) : 0)
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
        }
        delete plist
    }
    if (shieldBad) exit 1
}
