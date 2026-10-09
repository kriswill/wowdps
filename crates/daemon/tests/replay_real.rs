//! v45 (R29): the replay cut over real logs, exported as a replay's seven
//! files — what a pull cut by the extractor's old text cutter is compared
//! against, file by file (the replay tier's validation). Ignored: it needs
//! the logs, and the pulls' list holds real names.
//!
//! ```sh
//! WOWDPS_REPLAY_PULLS=pulls.tsv WOWDPS_REPLAY_LOGS=~/…/Logs WOWDPS_REPLAY_OUT=out \
//!   cargo test --release -p wowdps-daemon --test replay_real -- --ignored --nocapture
//! ```
//!
//! `pulls.tsv`: `name  log  start  encounter  owner` a line (tab-separated;
//! `start` the pull's ENCOUNTER_START in UTC ms, as a cut pull's `pull.txt`
//! says it). Each pull is found in its log's index (its encounter, starting
//! within 3 s), cut from its seeds and slice as the store cuts it, written
//! to the tier, read back, and written as files into `OUT/<name>/` with the
//! tier beside them (`replay.bin`); a line per pull says the sizes.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

use wowdps_core::index::{self, SegmentMeta, load_segment_text};
use wowdps_core::meter::SegmentKind;
use wowdps_core::model::View;
use wowdps_core::parser::tz_offset_min;
use wowdps_daemon::replay::cut_text;
use wowdps_proto::replay::{self, csv};

struct Pull {
    name: String,
    log: String,
    start_utc: i64,
    encounter: u32,
    owner: String,
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// The log's timezone offset in minutes, from its first line.
fn tz_of(path: &Path) -> i64 {
    let mut first = String::new();
    if let Ok(f) = File::open(path) {
        let _ = BufReader::new(f).read_line(&mut first);
    }
    tz_offset_min(&first).map_or(0, i64::from)
}

/// The rubric's journal: an encounter's instance name and its order.
fn journal(encounter: u32) -> (Option<String>, Option<u32>) {
    let Ok(rubric) = wowdps_encounter_rubric::Rubric::embedded() else {
        return (None, None);
    };
    let instance = rubric
        .encounters()
        .into_iter()
        .find(|(id, _, _)| *id == encounter)
        .map(|(_, _, i)| i.name.clone());
    let order = rubric
        .encounter(encounter, None)
        .and_then(Result::ok)
        .map(|e| e.order);
    (instance, order)
}

#[test]
#[ignore = "needs real logs: WOWDPS_REPLAY_PULLS, WOWDPS_REPLAY_LOGS, WOWDPS_REPLAY_OUT"]
fn real_pulls_cut_into_the_replay_tier() {
    let (Some(list), Some(logs), Some(out)) = (
        env("WOWDPS_REPLAY_PULLS"),
        env("WOWDPS_REPLAY_LOGS"),
        env("WOWDPS_REPLAY_OUT"),
    ) else {
        eprintln!("skipped: set WOWDPS_REPLAY_PULLS, WOWDPS_REPLAY_LOGS and WOWDPS_REPLAY_OUT");
        return;
    };
    let mut text = String::new();
    File::open(&list)
        .and_then(|mut f| f.read_to_string(&mut text))
        .expect("pulls list");
    let pulls: Vec<Pull> = text
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            let [name, log, start, encounter, owner, ..] = f[..] else {
                return None;
            };
            if log.starts_with("wcl:") {
                return None;
            }
            Some(Pull {
                name: name.to_string(),
                log: log.to_string(),
                start_utc: start.parse().ok()?,
                encounter: encounter.parse().ok()?,
                owner: owner.to_string(),
            })
        })
        .collect();
    // One scan per log.
    let mut by_log: BTreeMap<&str, Vec<&Pull>> = BTreeMap::new();
    for p in &pulls {
        by_log.entry(&p.log).or_default().push(p);
    }
    let mut failed = Vec::new();
    for (log, pulls) in by_log {
        let path = PathBuf::from(&logs).join(log);
        let Ok(mut file) = File::open(&path) else {
            failed.push(format!("{log}: missing"));
            continue;
        };
        let idx = index::scan(&mut file);
        let tz = tz_of(&path);
        // With WOWDPS_REPLAY_KEYS set, each keystone run the log holds whole
        // (a keyed Σ, what the store cuts for a key) and its tier's size.
        if env("WOWDPS_REPLAY_KEYS").is_some() {
            for meta in idx.overalls.iter().filter(|m| m.pars_ms.is_some()) {
                let text = load_segment_text(&path, meta).expect("run text");
                let cut = cut_text(&text, None);
                let bytes = replay::encode(&cut);
                println!(
                    "key\t{}\t{} s\t{} units\t{} posts\t{} events\ttier {} B\tfloor {}",
                    meta.name,
                    meta.duration_ms / 1000,
                    cut.units.len(),
                    cut.posts(),
                    cut.events.len(),
                    bytes.len(),
                    cut.floor
                );
            }
        }
        for p in pulls {
            let local = p.start_utc + tz * 60_000;
            let meta: Option<&SegmentMeta> = idx.segments.iter().find(|m| {
                m.kind == SegmentKind::Encounter
                    && m.encounter.is_some_and(|e| e.id == p.encounter)
                    && (m.start_ms - local).abs() < 3000
            });
            let Some(meta) = meta else {
                failed.push(format!("{}: no segment", p.name));
                continue;
            };
            let text = load_segment_text(&path, meta).expect("segment text");
            let owner = (!p.owner.is_empty()).then_some(p.owner.as_str());
            let cut = cut_text(&text, owner);
            let bytes = replay::encode(&cut);
            let back = replay::decode(&bytes).expect("decodes");
            assert!(back == cut, "{}: the tier round-trips", p.name);
            let meter = text.meter();
            let raid = meter
                .segments()
                .iter()
                .find(|s| s.kind == SegmentKind::Encounter)
                .map(|s| s.raid_timeline(View::Damage));
            let (instance, order) = journal(p.encounter);
            let dir = PathBuf::from(&out).join(&p.name);
            csv::write_dir(
                &dir,
                &back,
                raid.as_ref()
                    .map(|r| (r.series.as_slice(), u64::from(r.bucket_ms))),
                &csv::PullFacts {
                    instance: instance.as_deref(),
                    log,
                    owner,
                    order,
                },
            )
            .expect("files");
            std::fs::write(dir.join("replay.bin"), &bytes).expect("tier");
            let csv_bytes: u64 = csv::FILES
                .iter()
                .filter_map(|f| std::fs::metadata(dir.join(f)).ok())
                .map(|m| m.len())
                .sum();
            println!(
                "{}\t{} units\t{} posts\t{} events\t{} placed\t{} markers\ttier {} B\tfiles {} B\tlines {}\tfloor {}",
                p.name,
                back.units.len(),
                back.posts(),
                back.events.len(),
                back.placed.len(),
                back.markers.len(),
                bytes.len(),
                csv_bytes,
                text.slice().count(),
                back.floor,
            );
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}
