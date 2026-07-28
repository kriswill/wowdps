//! Contract-shaped fake implementation of `parser.rs` + `meter.rs`.
//!
//! Exists only so the TUI can be built and exercised before `core` merges.
//! Deleted at milestone 2 — see `model.rs`. Everything public here matches
//! CONTRACT.md exactly; the bodies are made-up data, not real analysis.

// ---------------------------------------------------------------- parser.rs

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    pub ts_ms: i64,
    pub event: Event,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    pub guid: String,
    pub name: String,
    pub flags: u32,
}

impl Unit {
    pub fn is_player(&self) -> bool {
        self.guid.starts_with("Player-")
    }
    pub fn is_pet_or_guardian(&self) -> bool {
        self.guid.starts_with("Pet-") || self.guid.starts_with("Creature-")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spell {
    pub id: u32,
    pub name: String,
    pub school: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuraType {
    Buff,
    Debuff,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Version {
        log_version: u32,
        advanced: bool,
    },
    EncounterStart {
        id: u32,
        name: String,
        difficulty: u32,
        group_size: u32,
    },
    EncounterEnd {
        id: u32,
        name: String,
        success: bool,
    },
    CombatantInfo {
        guid: String,
    },
    Damage {
        src: Unit,
        dst: Unit,
        spell: Option<Spell>,
        amount: u64,
        overkill: i64,
        absorbed: u64,
        critical: bool,
        periodic: bool,
    },
    Heal {
        src: Unit,
        dst: Unit,
        spell: Spell,
        amount: u64,
        overheal: u64,
        absorbed: u64,
        critical: bool,
    },
    Absorbed {
        src: Unit,
        dst: Unit,
        absorber: Unit,
        spell: Option<Spell>,
        amount: u64,
    },
    Interrupt {
        src: Unit,
        dst: Unit,
        spell: Spell,
        interrupted_spell: Spell,
    },
    AuraApplied {
        src: Unit,
        dst: Unit,
        spell: Spell,
        aura_type: AuraType,
    },
    Dispel {
        src: Unit,
        dst: Unit,
        spell: Spell,
        dispelled_spell: Spell,
    },
    Summon {
        owner: Unit,
        pet: Unit,
    },
    Death {
        unit: Unit,
    },
    Other,
}

/// Stub parser: recovers only the timestamp so replay drives a real clock;
/// every recognised line becomes `Event::Other`.
pub fn parse_line(line: &str) -> Option<LogLine> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    Some(LogLine {
        ts_ms: parse_ts_ms(line).unwrap_or(0),
        event: Event::Other,
    })
}

/// `7/26/2026 20:14:32.123-4  SPELL_DAMAGE,...` -> ms since midnight.
fn parse_ts_ms(line: &str) -> Option<i64> {
    let (_, rest) = line.split_once(' ')?;
    let clock = rest.split(['-', ' ']).next()?;
    let mut parts = clock.split(':');
    let h: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next()?.parse().ok()?;
    let (s, ms) = parts.next()?.split_once('.')?;
    let s: i64 = s.parse().ok()?;
    let ms: i64 = ms.parse().ok()?;
    Some(((h * 60 + m) * 60 + s) * 1000 + ms)
}

// ----------------------------------------------------------------- meter.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Damage,
    Healing,
    Interrupts,
    CrowdControl,
    Dispels,
    Deaths,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentKind {
    Encounter,
    Trash,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub key: String,
    pub label: String,
    pub amount: u64,
    pub extra: u64,
    pub per_sec: f64,
    pub pct: f64,
}

/// One actor's contribution to one view, plus its drilldowns.
#[derive(Debug, Clone)]
struct Entry {
    key: String,
    label: String,
    amount: u64,
    extra: u64,
    by_spell: Vec<(String, u64, u64)>,
    by_target: Vec<(String, u64, u64)>,
}

#[derive(Debug, Clone)]
pub struct Segment {
    pub kind: SegmentKind,
    pub name: String,
    pub start_ms: i64,
    pub end_ms: Option<i64>,
    pub success: Option<bool>,
    /// Per-view entries, indexed by `view_index`.
    views: [Vec<Entry>; 6],
    /// Latest timestamp seen, so a live segment can report a duration.
    now_ms: i64,
}

fn view_index(view: View) -> usize {
    match view {
        View::Damage => 0,
        View::Healing => 1,
        View::Interrupts => 2,
        View::CrowdControl => 3,
        View::Dispels => 4,
        View::Deaths => 5,
    }
}

fn is_rate_view(view: View) -> bool {
    matches!(view, View::Damage | View::Healing)
}

impl Segment {
    pub fn duration_ms(&self, now_ms: i64) -> i64 {
        let end = self.end_ms.unwrap_or_else(|| now_ms.max(self.now_ms));
        (end - self.start_ms).max(0)
    }

    pub fn rows(&self, view: View) -> Vec<Row> {
        let entries = &self.views[view_index(view)];
        let total: u64 = entries.iter().map(|e| e.amount).sum();
        let secs = self.duration_ms(self.now_ms) as f64 / 1000.0;
        let mut rows: Vec<Row> = entries
            .iter()
            .map(|e| Row {
                key: e.key.clone(),
                label: e.label.clone(),
                amount: e.amount,
                extra: e.extra,
                per_sec: rate(e.amount, secs, view),
                pct: pct(e.amount, total),
            })
            .collect();
        sort_rows(&mut rows);
        rows
    }

    pub fn breakdown(&self, player_guid: &str, view: View) -> (Vec<Row>, Vec<Row>) {
        let entries = &self.views[view_index(view)];
        let Some(entry) = entries.iter().find(|e| e.key == player_guid) else {
            return (Vec::new(), Vec::new());
        };
        let secs = self.duration_ms(self.now_ms) as f64 / 1000.0;
        let build = |src: &[(String, u64, u64)]| {
            let total: u64 = src.iter().map(|(_, a, _)| *a).sum();
            let mut rows: Vec<Row> = src
                .iter()
                .map(|(label, amount, extra)| Row {
                    key: label.clone(),
                    label: label.clone(),
                    amount: *amount,
                    extra: *extra,
                    per_sec: rate(*amount, secs, view),
                    pct: pct(*amount, total),
                })
                .collect();
            sort_rows(&mut rows);
            rows
        };
        (build(&entry.by_spell), (build(&entry.by_target)))
    }
}

fn rate(amount: u64, secs: f64, view: View) -> f64 {
    if !is_rate_view(view) || secs <= 0.0 {
        0.0
    } else {
        amount as f64 / secs
    }
}

fn pct(amount: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        amount as f64 * 100.0 / total as f64
    }
}

/// Descending by amount; label breaks ties so ordering is stable across frames.
fn sort_rows(rows: &mut [Row]) {
    rows.sort_by(|a, b| b.amount.cmp(&a.amount).then_with(|| a.label.cmp(&b.label)));
}

pub struct Meter {
    segments: Vec<Segment>,
    fed: u64,
}

impl Default for Meter {
    fn default() -> Self {
        Self::new()
    }
}

impl Meter {
    pub fn new() -> Self {
        Self {
            segments: demo_segments(),
            fed: 0,
        }
    }

    /// Stub behaviour: advance the live segment's clock, and grow its numbers
    /// occasionally so the meter visibly moves during a replay or live tail.
    pub fn feed(&mut self, line: LogLine) {
        self.fed += 1;
        let Some(last) = self.segments.last_mut() else {
            return;
        };
        if last.end_ms.is_some() {
            return;
        }
        if line.ts_ms > 0 {
            last.now_ms = last.now_ms.max(last.start_ms + (self.fed as i64 * 120));
        }
        if self.fed.is_multiple_of(16) {
            let mut rng = Rng::new(self.fed);
            for view in 0..6 {
                for entry in last.views[view].iter_mut() {
                    let bump = if view < 2 {
                        rng.next_range(20_000, 90_000)
                    } else {
                        u64::from(rng.next_range(0, 8) == 0)
                    };
                    entry.amount += bump;
                    if let Some(first) = entry.by_spell.first_mut() {
                        first.1 += bump;
                    }
                    if let Some(first) = entry.by_target.first_mut() {
                        first.1 += bump;
                    }
                }
            }
        }
    }

    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    pub fn current_index(&self) -> usize {
        self.segments.len().saturating_sub(1)
    }
}

// ------------------------------------------------------------- demo dataset

/// xorshift64* — deterministic, so the stub renders identically every run and
/// TestBackend assertions stay stable. No `rand` dependency.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn next_range(&mut self, lo: u64, hi: u64) -> u64 {
        if hi <= lo {
            return lo;
        }
        lo + self.next_u64() % (hi - lo)
    }
}

const PLAYERS: [(&str, &str); 3] = [
    ("Player-1-AAAA", "Thrallmar"),
    ("Player-1-BBBB", "Bigheals"),
    ("Player-1-CCCC", "Stabbyboi"),
];

const DAMAGE_SPELLS: [&[&str]; 3] = [
    &[
        "Chaos Bolt",
        "Immolate",
        "Incinerate",
        "Thrallmar (Pet): Melee",
    ],
    &["Holy Fire", "Smite", "Shadow Word: Pain"],
    &["Backstab", "Envenom", "Poisoned Knife"],
];
const HEAL_SPELLS: [&[&str]; 3] = [
    &["Health Funnel"],
    &["Flash Heal", "Renew", "Prayer of Mending"],
    &["Crimson Vial"],
];
const UTILITY_SPELLS: [&[&str]; 3] = [
    &["Axe Toss", "Fear"],
    &["Silence", "Psychic Scream"],
    &["Kick", "Kidney Shot"],
];
const TARGETS: [&str; 3] = ["Boss", "Add: Shambler", "Add: Acolyte"];

fn demo_segments() -> Vec<Segment> {
    vec![
        demo_segment(
            SegmentKind::Encounter,
            "Kel'Thuzad",
            0,
            Some(245_000),
            Some(true),
            1,
        ),
        demo_segment(SegmentKind::Trash, "Trash", 260_000, Some(390_000), None, 2),
        demo_segment(SegmentKind::Encounter, "Sludgefist", 420_000, None, None, 3),
    ]
}

fn demo_segment(
    kind: SegmentKind,
    name: &str,
    start_ms: i64,
    end_ms: Option<i64>,
    success: Option<bool>,
    seed: u64,
) -> Segment {
    let mut rng = Rng::new(seed);
    let duration_s = (end_ms.unwrap_or(start_ms + 134_000) - start_ms) / 1000;
    let mut views: [Vec<Entry>; 6] = Default::default();

    for (i, (guid, name)) in PLAYERS.iter().enumerate() {
        // Damage / healing: scale with segment length so DPS stays plausible.
        let dps = rng.next_range(45_000, 120_000);
        let hps = if i == 1 {
            rng.next_range(60_000, 110_000)
        } else {
            rng.next_range(2_000, 12_000)
        };
        views[view_index(View::Damage)].push(spread(
            guid,
            name,
            dps * duration_s as u64,
            DAMAGE_SPELLS[i],
            &mut rng,
            8,
        ));
        views[view_index(View::Healing)].push(spread(
            guid,
            name,
            hps * duration_s as u64,
            HEAL_SPELLS[i],
            &mut rng,
            22,
        ));
        // Count views: small integers, and not every player scores on each.
        for (view, cap) in [
            (View::Interrupts, 6),
            (View::CrowdControl, 9),
            (View::Dispels, 5),
        ] {
            let n = rng.next_range(0, cap);
            if n > 0 {
                views[view_index(view)].push(spread(guid, name, n, UTILITY_SPELLS[i], &mut rng, 0));
            }
        }
    }

    // Deaths: one per dead player, and only on the wipe/trash segments.
    if success != Some(true) {
        for (guid, name) in PLAYERS
            .iter()
            .skip(1)
            .take(if end_ms.is_some() { 2 } else { 1 })
        {
            views[view_index(View::Deaths)].push(Entry {
                key: (*guid).to_string(),
                label: (*name).to_string(),
                amount: 1,
                extra: 0,
                by_spell: vec![("Thunderous Stomp".to_string(), 1, 0)],
                by_target: vec![("Sludgefist".to_string(), 1, 0)],
            });
        }
    }

    Segment {
        kind,
        name: name.to_string(),
        start_ms,
        end_ms,
        success,
        views,
        now_ms: end_ms.unwrap_or(start_ms + 134_000),
    }
}

/// Split `total` across `labels` and across targets, with `extra_pct` of the
/// amount reported as overheal/overkill.
fn spread(
    guid: &str,
    name: &str,
    total: u64,
    labels: &[&str],
    rng: &mut Rng,
    extra_pct: u64,
) -> Entry {
    let by_spell = split(total, labels, rng, extra_pct);
    let by_target = split(total, &TARGETS, rng, extra_pct);
    Entry {
        key: guid.to_string(),
        label: name.to_string(),
        amount: total,
        extra: total * extra_pct / 100,
        by_spell,
        by_target,
    }
}

/// Deal `total` out over `labels`, largest first, remainder to the last label
/// so the parts always sum back to `total`.
fn split(total: u64, labels: &[&str], rng: &mut Rng, extra_pct: u64) -> Vec<(String, u64, u64)> {
    let mut out = Vec::with_capacity(labels.len());
    let mut left = total;
    for (i, label) in labels.iter().enumerate() {
        let amount = if i + 1 == labels.len() {
            left
        } else {
            let share = left / (labels.len() - i) as u64;
            let jitter = rng.next_range(share / 2, share.max(1) * 3 / 2);
            jitter.min(left)
        };
        left -= amount;
        out.push(((*label).to_string(), amount, amount * extra_pct / 100));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_timestamp_off_a_log_line() {
        let l = parse_line("7/26/2026 20:14:32.123-4  SPELL_DAMAGE,Player-1,\"x\"").unwrap();
        assert_eq!(l.ts_ms, ((20 * 60 + 14) * 60 + 32) * 1000 + 123);
        assert_eq!(l.event, Event::Other);
    }

    #[test]
    fn blank_lines_are_not_log_lines() {
        assert!(parse_line("").is_none());
        assert!(parse_line("   ").is_none());
    }

    #[test]
    fn unparsable_lines_still_yield_other() {
        let l = parse_line("garbage without a timestamp").unwrap();
        assert_eq!(l.ts_ms, 0);
    }

    #[test]
    fn demo_meter_has_history_and_a_live_segment() {
        let m = Meter::new();
        assert_eq!(m.segments().len(), 3);
        assert_eq!(m.current_index(), 2);
        assert!(m.segments()[0].end_ms.is_some());
        assert!(m.segments()[2].end_ms.is_none(), "last segment is live");
    }

    #[test]
    fn rows_are_sorted_desc_and_pct_sums_to_100() {
        let m = Meter::new();
        let rows = m.segments()[0].rows(View::Damage);
        assert_eq!(rows.len(), 3);
        for w in rows.windows(2) {
            assert!(w[0].amount >= w[1].amount, "not sorted: {rows:?}");
        }
        let sum: f64 = rows.iter().map(|r| r.pct).sum();
        assert!((sum - 100.0).abs() < 0.01, "pct sum was {sum}");
        assert!(rows[0].per_sec > 0.0, "damage rows carry DPS");
    }

    #[test]
    fn count_views_have_no_rate() {
        let m = Meter::new();
        for seg in m.segments() {
            for row in seg.rows(View::Interrupts) {
                assert_eq!(row.per_sec, 0.0);
            }
        }
    }

    #[test]
    fn breakdown_splits_by_spell_and_target() {
        let m = Meter::new();
        let seg = &m.segments()[0];
        let top = &seg.rows(View::Damage)[0];
        let (by_spell, by_target) = seg.breakdown(&top.key, View::Damage);
        assert!(!by_spell.is_empty() && !by_target.is_empty());
        assert_eq!(by_spell.iter().map(|r| r.amount).sum::<u64>(), top.amount);
        assert_eq!(by_target.iter().map(|r| r.amount).sum::<u64>(), top.amount);
    }

    #[test]
    fn breakdown_of_an_unknown_player_is_empty() {
        let m = Meter::new();
        let (a, b) = m.segments()[0].breakdown("Player-nope", View::Damage);
        assert!(a.is_empty() && b.is_empty());
    }

    #[test]
    fn feeding_lines_advances_the_live_segment_only() {
        let mut m = Meter::new();
        let before_live = m.segments()[2].rows(View::Damage)[0].amount;
        let before_done = m.segments()[0].rows(View::Damage)[0].amount;
        for i in 0..64 {
            m.feed(LogLine {
                ts_ms: 1000 + i,
                event: Event::Other,
            });
        }
        assert!(m.segments()[2].rows(View::Damage)[0].amount > before_live);
        assert_eq!(m.segments()[0].rows(View::Damage)[0].amount, before_done);
    }

    #[test]
    fn live_duration_grows_with_now_ms() {
        let m = Meter::new();
        let seg = &m.segments()[2];
        let early = seg.duration_ms(seg.start_ms + 1_000);
        let later = seg.duration_ms(seg.start_ms + 999_000);
        assert!(later > early);
        // A closed segment ignores `now_ms` entirely.
        let done = &m.segments()[0];
        assert_eq!(done.duration_ms(i64::MAX), 245_000);
    }
}
