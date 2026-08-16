//! Parser robustness: "never panics" is a contract claim (`parse_line` doc,
//! CONTRACT.md), so it gets a hand-rolled fuzz suite. All mutation is driven
//! by a fixed-seed xorshift64 PRNG — the crates are stdlib-only, so no
//! proptest/quickcheck — which makes every run identical and every failure
//! reproducible. `parse_line` takes `&str`, so raw-byte mutations enter the
//! parser the way real corrupt bytes would: through `from_utf8_lossy`.

use wowdps_core::parser::{Event, parse_line};

const FIXTURES: &[&str] = &[
    concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/sample.txt"),
    concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/arena.txt"),
    concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/instance.txt"),
];

/// xorshift64 with a fixed seed: deterministic, stdlib-only, good enough to
/// scatter mutations. Never seeded from time or the environment.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        assert!(seed != 0, "xorshift64 degenerates at 0");
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Uniform-ish in `0..n` (modulo bias is irrelevant for fuzzing).
    fn below(&mut self, n: usize) -> usize {
        assert!(n > 0);
        (self.next() % n as u64) as usize
    }

    fn byte(&mut self) -> u8 {
        (self.next() & 0xFF) as u8
    }
}

fn corpus() -> Vec<String> {
    let mut lines = Vec::new();
    for path in FIXTURES {
        let text = std::fs::read_to_string(path);
        assert!(text.is_ok(), "{path}: unreadable fixture");
        lines.extend(text.unwrap_or_default().lines().map(str::to_string));
    }
    assert!(
        lines.len() > 100,
        "corpus suspiciously small ({} lines) — fixture moved?",
        lines.len()
    );
    lines
}

/// The single assertion of this suite: feeding `bytes` (lossily decoded, as
/// a real corrupt log would arrive) must return, not unwind. The `Option`
/// itself is unconstrained — malformed lines are `None`, salvageable ones
/// parse — because the contract's promise is about panics, not acceptance.
fn assert_parses_without_panic(bytes: &[u8], ctx: &str) {
    let text = String::from_utf8_lossy(bytes).into_owned();
    let outcome = std::panic::catch_unwind(|| {
        let _ = parse_line(&text);
    });
    assert!(outcome.is_ok(), "parse_line panicked ({ctx}) on {text:?}");
}

/// Every fixture line cut at every byte boundary — the exhaustive version of
/// "a write got interrupted mid-line", which tailing a live log makes an
/// everyday event rather than a corruption scenario.
#[test]
fn every_truncation_of_every_fixture_line_parses_without_panic() {
    for (n, line) in corpus().iter().enumerate() {
        let bytes = line.as_bytes();
        for cut in 0..=bytes.len() {
            assert_parses_without_panic(&bytes[..cut], &format!("line {n} cut at {cut}"));
        }
    }
}

/// Random single-byte substitutions, insertions and deletions over the whole
/// corpus. Substituted bytes cover the full 0..=255 range, so invalid UTF-8
/// lands mid-line too (and reaches the parser through the lossy decode).
#[test]
fn random_byte_mutations_parse_without_panic() {
    let lines = corpus();
    let mut rng = Rng::new(0x5EED_CAFE_F00D_0001);
    for i in 0..10_000 {
        let mut bytes = lines[rng.below(lines.len())].as_bytes().to_vec();
        match rng.below(3) {
            0 if !bytes.is_empty() => {
                let at = rng.below(bytes.len());
                bytes[at] = rng.byte();
            }
            1 => {
                let at = rng.below(bytes.len() + 1);
                bytes.insert(at, rng.byte());
            }
            _ if !bytes.is_empty() => {
                bytes.remove(rng.below(bytes.len()));
            }
            _ => {}
        }
        assert_parses_without_panic(&bytes, &format!("mutation iteration {i}"));
    }
}

/// Splices — the head of one line grafted onto the tail of another — model a
/// log rotated or flushed mid-write, where two records collide in one line.
#[test]
fn random_splices_of_two_lines_parse_without_panic() {
    let lines = corpus();
    let mut rng = Rng::new(0x5EED_CAFE_F00D_0002);
    for i in 0..3_000 {
        let a = lines[rng.below(lines.len())].as_bytes();
        let b = lines[rng.below(lines.len())].as_bytes();
        let cut_a = rng.below(a.len() + 1);
        let cut_b = rng.below(b.len() + 1);
        let mut spliced = a[..cut_a].to_vec();
        spliced.extend_from_slice(&b[cut_b..]);
        assert_parses_without_panic(&spliced, &format!("splice iteration {i}"));
    }
}

/// Structural-character injection: quotes, commas and brackets are exactly
/// the bytes the CSV splitter keys on, so extra ones probe its state machine
/// harder than uniform noise does.
#[test]
fn quote_comma_and_bracket_injection_parses_without_panic() {
    let lines = corpus();
    let mut rng = Rng::new(0x5EED_CAFE_F00D_0003);
    for (n, line) in lines.iter().enumerate() {
        for &ch in b"\",[]()" {
            for _ in 0..8 {
                let mut bytes = line.as_bytes().to_vec();
                bytes.insert(rng.below(bytes.len() + 1), ch);
                // A second injection of the same character catches unbalanced
                // open/close handling (e.g. a quote opened and never closed).
                bytes.insert(rng.below(bytes.len() + 1), ch);
                assert_parses_without_panic(&bytes, &format!("injection into line {n}"));
            }
        }
    }
}

/// Invalid UTF-8 (lone continuation bytes, overlong starts, 0xFF) inserted
/// into valid lines. The lossy decode turns each broken sequence into
/// U+FFFD, which is multi-byte — so this also checks the parser's slicing
/// stays on char boundaries when byte offsets shift under it.
#[test]
fn invalid_utf8_insertions_parse_via_lossy_without_panic() {
    let lines = corpus();
    let mut rng = Rng::new(0x5EED_CAFE_F00D_0004);
    let bad: &[u8] = &[0x80, 0xBF, 0xC0, 0xC1, 0xE0, 0xF5, 0xFF];
    for i in 0..3_000 {
        let mut bytes = lines[rng.below(lines.len())].as_bytes().to_vec();
        for _ in 0..1 + rng.below(3) {
            let b = bad[rng.below(bad.len())];
            bytes.insert(rng.below(bytes.len() + 1), b);
        }
        assert_parses_without_panic(&bytes, &format!("utf8 iteration {i}"));
    }
}

/// CONTRACT.md: unknown events are `Some(LogLine { event: Other, .. })`,
/// never `None` and never an error. Rewriting every fixture line's event
/// name to an unknown one must therefore still parse — the timestamp is
/// valid, only the event is foreign.
#[test]
fn unknown_events_become_event_other_never_an_error() {
    let mut checked = 0;
    for line in corpus() {
        // Timestamp and CSV are separated by two spaces in the fixtures.
        let Some(sep) = line.find("  ") else { continue };
        let (ts, rest) = line.split_at(sep + 2);
        let fields_after_event = rest.split_once(',').map_or("", |(_, tail)| tail);
        let mutated = format!("{ts}WOWDPS_FUZZ_UNKNOWN_EVENT,{fields_after_event}");
        let parsed = parse_line(&mutated);
        assert!(
            parsed.is_some(),
            "unknown event dropped instead of becoming Other: {mutated:?}"
        );
        assert_eq!(
            parsed.map(|l| l.event),
            Some(Event::Other),
            "unknown event not Event::Other: {mutated:?}"
        );
        checked += 1;
    }
    assert!(
        checked > 100,
        "only {checked} lines exercised — separator convention changed?"
    );
}
