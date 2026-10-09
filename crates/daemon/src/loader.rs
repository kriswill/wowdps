//! Historical-segment parsing off the hub thread. One client browsing a
//! night of history must never freeze the live overlay, so loads run on a
//! small worker pool and come back as `HubMsg::Loaded`.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread;

use wowdps_core::index::{SegmentMeta, SegmentText, load_segment_text};
use wowdps_core::model::SegmentId;

use crate::history::{CutJob, HistoryLink, HistoryReq, ImportJob, Loaded};
use crate::hub::HubMsg;

pub struct LoadReq {
    pub id: SegmentId,
    pub path: PathBuf,
    pub meta: SegmentMeta,
    pub reply: LoadReply,
}

/// Who gets the parsed meter. The hub installs it into the engine's LRU;
/// the history store's import path consumes it and never touches the LRU.
pub enum LoadReply {
    Hub,
    History {
        link: HistoryLink,
        job: Box<ImportJob>,
    },
}

/// Start `workers` loader threads; they exit when the returned sender drops.
pub fn spawn(hub: Sender<HubMsg>, workers: usize) -> Sender<LoadReq> {
    let (tx, rx) = channel::<LoadReq>();
    let rx = Arc::new(Mutex::new(rx));
    for _ in 0..workers.max(1) {
        let rx: Arc<Mutex<Receiver<LoadReq>>> = Arc::clone(&rx);
        let hub = hub.clone();
        thread::spawn(move || {
            loop {
                let req = {
                    let guard = rx.lock().unwrap_or_else(|e| e.into_inner());
                    guard.recv()
                };
                let Ok(req) = req else { return };
                let text = load_segment_text(&req.path, &req.meta)
                    .map_err(|e| format!("{}: {e}", req.path.display()));
                match req.reply {
                    LoadReply::Hub => {
                        let result = text.map(|text| Box::new(text.meter()));
                        if hub.send(HubMsg::Loaded { id: req.id, result }).is_err() {
                            return;
                        }
                    }
                    LoadReply::History { link, job } => {
                        let result = text.map(|text| load_for(&text, job.cut));
                        link.reply(HistoryReq::Loaded { job, result });
                    }
                }
            }
        });
    }
    tx
}

/// v45 (R29): what an import job asked of its segment's text — the meter,
/// the replay cut, or both from one parse of its lines.
fn load_for(text: &SegmentText, cut: CutJob) -> Loaded {
    match cut {
        CutJob::No => Loaded {
            meter: Some(Box::new(text.meter())),
            cut: None,
        },
        CutJob::Also => {
            let (meter, cut) = crate::replay::meter_and_cut(text, None);
            Loaded {
                meter: Some(Box::new(meter)),
                cut: Some(Box::new(cut)),
            }
        }
        CutJob::Only => Loaded {
            meter: None,
            cut: Some(Box::new(crate::replay::cut_text(text, None))),
        },
    }
}
