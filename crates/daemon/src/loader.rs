//! Historical-segment parsing off the hub thread. One client browsing a
//! night of history must never freeze the live overlay, so loads run on a
//! small worker pool and come back as `HubMsg::Loaded`.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread;

use wowdps_core::index::{SegmentMeta, load_segment};
use wowdps_core::meter::meter_from_lines;
use wowdps_core::model::SegmentId;

use crate::hub::HubMsg;

pub struct LoadReq {
    pub id: SegmentId,
    pub path: PathBuf,
    pub meta: SegmentMeta,
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
                    let guard = rx.lock().expect("loader queue poisoned");
                    guard.recv()
                };
                let Ok(req) = req else { return };
                let result = load_segment(&req.path, &req.meta)
                    .map(|lines| meter_from_lines(lines.iter().map(String::as_str)))
                    .map_err(|e| format!("{}: {e}", req.path.display()));
                if hub.send(HubMsg::Loaded { id: req.id, result }).is_err() {
                    return;
                }
            }
        });
    }
    tx
}
