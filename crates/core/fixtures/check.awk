#!/usr/bin/env gawk -f
#
# check.awk — independent expected-value computer for wowdps fixtures.
#
# Reads a WoW advanced combat log and emits per-segment / per-player totals as a
# stable TSV. This is the VALIDATOR's own implementation of the CONTRACT.md R1-R6
# and R17 semantics, written from the log grammar. It never calls, links, or
# consults the Rust implementation — that is the whole point: the Rust is graded
# against this, not the other way round.
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
}
# A *_MISSED line: count 1 on the friendly destination; BLOCK's amount and ABSORB's
# amountMissed are PREVENTED damage. A miss with no open segment (cur == 0) is
# dropped — R17: a miss never opens a segment.
function missed(dguid, dflags, kind, amt,   t) {
    if (cur == 0) return
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
ev == "SPELL_HEAL" || ev == "SPELL_PERIODIC_HEAL" {
    if (NF != 36) next
    a = actor($2, $4); if (a == "") next
    if ($10 + 0 in excl) next
    amount = $33 + 0                   # off32 amount (INCLUDES overheal)
    over   = $34 + 0                   # off33 overheal
    note(cur, a, "heal", amount - over); note(cur, a, "overheal", over)
    next
}

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
        t = actor($6, $8); note(cur, t, "stagger", amt)
        next
    }
    a = actor(ag, af); if (a == "") next
    note(cur, a, "heal", amt); note(cur, a, "absorbheal", amt)
    next
}

ev == "SPELL_INTERRUPT" { a = actor($2, $4); note(cur, a, "interrupts", 1); next }
ev == "SPELL_DISPEL"    { a = actor($2, $4); note(cur, a, "dispels", 1);    next }

ev == "SPELL_AURA_APPLIED" {
    if (strip($13) != "DEBUFF") next
    if (!(($10 + 0) in cc)) next
    a = actor($2, $4); note(cur, a, "cc", 1)
    next
}

# Deaths: players only (a pet death is not a player death)
ev == "UNIT_DIED" {
    if (!isPlayerFlags($8)) next
    note(cur, $6, "deaths", 1)
    next
}

END {
    print "segment", "kind", "name", "result", "dur_ms", "enc_id", "difficulty", "player", "metric", "value"
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
        }
        delete plist
    }
}
