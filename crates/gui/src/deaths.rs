//! The Deaths view's window-only pieces (R25, v35). On that view the meter
//! is a chronological table (the prototype's `.v-deaths`): every death in
//! the order it happened, from the snapshot's raid timeline — its time, who
//! died, the killing blow and who dealt it, when a rez raised them, the hit
//! and its overkill in the outcome red — where it used to be a count per
//! player in first-death order, six identical bars for six deaths. A row is
//! a press (or a j/k) away from that death's recap in the inspector: the
//! Deaths drill on that player, at that death window.
//!
//! Also here: what a death is called in words the ribbon and the recap
//! share — the time before a death a recap event happened.
//!
//! The live daemon sends the timeline with every snapshot and the history
//! store rebuilds one for a stored pull (v35); the count table stands only
//! where neither did — a store that kept a pull's card alone. An arena's
//! other team (R13) dies in the same list, tagged "enemy" and never counted
//! among the group's deaths.

use iced::widget::{column, container, mouse_area, row, scrollable, text};
use iced::{Element, Length, Theme};

use wowdps_model::fmt::{commas, duration};
use wowdps_model::{RaidDeath, RaidTimeline, Row, View};
use wowdps_proto::ClientState;

use crate::theme::{self, DensityPitch, Look, size};
use crate::view::{display_name, hover_style_in, row_style_in, scroll_clear};
use crate::window::{Gui, Message, RowHover};

/// What a death is, to open it: the player's meter key and label, and which
/// of their death windows (R9's index) — the Deaths drill's own question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Pick {
    pub key: String,
    pub label: String,
    pub index: u32,
}

/// What a death with no damage in its recap says in the killing blow's
/// place — a mechanic that removed the player without a hit. A note, not
/// an ability: set in the faint ink.
pub(crate) const NO_DAMAGE: &str = "No damage logged";

/// The table's columns (`.v-deaths{--cols:46px minmax(0,150px) minmax(0,1fr)
/// 74px 74px}`, and at 820 px and under `40px minmax(0,.8fr) minmax(0,1fr)`,
/// the hit and the overkill gone) and the gap between them
/// (`column-gap:12px`). The heads, rows and total start 4 px in from the
/// meter's edge, as the meter's own rank column does (`.thead,.trow,
/// .ttotal{padding:0 16px 0 4px}`), and end 16 px short of the border —
/// the list's scrollbar lane inside that only when the deaths overflow it.
/// (`minmax(0,150px)` beside a `1fr` resolves to the whole 150 px wherever
/// the row has it: a grid fills a track to its max before its `fr` ones.)
const TIME_W: f32 = 46.0;
const TIME_W_NARROW: f32 = 40.0;
const PLAYER_W: f32 = 150.0;
const NUM_W: f32 = 74.0;
const GAP: f32 = 12.0;
const LEAD: f32 = 4.0;
const TRAIL: f32 = 16.0;
/// The heading line's top and bottom (`.thead{padding:8px … 6px}`).
const HEADS_TOP: f32 = 8.0;
const HEADS_BOTTOM: f32 = 6.0;
/// A player's disc and the air after it (`.who2{gap:8px}`), the killing
/// blow's pieces (`.kb{gap:6px}`), the name (`.nm{font-size:15px}`), the
/// frame's 14 px the blow is set in, and its source's 13.5 (`.kb .src`).
const DISC: f32 = 20.0;
const WHO_GAP: f32 = 8.0;
const KB_GAP: f32 = 6.0;
const BLOW_PX: f32 = 14.0;
/// The enemy team's word after a name (R13), in the source's size.
const ENEMY: &str = "enemy";
/// An empty list's words, inset like the prototype's `.empty`.
const EMPTY_PAD: [f32; 2] = [20.0, 20.0];

/// One column, described once for the heads and every line alike: its
/// head, its width and whether its figures stand at its end.
struct Column {
    head: &'static str,
    width: Length,
    end: bool,
}

/// The columns at the window's breakpoint.
fn columns(narrow: bool) -> Vec<Column> {
    let col = |head, width, end| Column { head, width, end };
    let mut cols = vec![
        col(
            "Time",
            Length::Fixed(if narrow { TIME_W_NARROW } else { TIME_W }),
            false,
        ),
        // `minmax(0,150px)`, `minmax(0,.8fr)` narrow, beside the killing
        // blow's `1fr`.
        col(
            "Player",
            if narrow {
                Length::FillPortion(4)
            } else {
                Length::Fixed(PLAYER_W)
            },
            false,
        ),
        col("Killing blow", Length::FillPortion(5), false),
    ];
    if !narrow {
        cols.push(col("Hit", Length::Fixed(NUM_W), true));
        cols.push(col("Overkill", Length::Fixed(NUM_W), true));
    }
    cols
}

/// `cells` laid out in `cols`, one each, in order.
fn cells(cols: &[Column], cells: Vec<Element<'static, Message>>) -> Element<'static, Message> {
    let mut line = row![].spacing(GAP).align_y(iced::Alignment::Center);
    for (c, cell) in cols.iter().zip(cells) {
        line = line.push(container(cell).width(c.width).clip(true).align_x(if c.end {
            iced::Alignment::End
        } else {
            iced::Alignment::Start
        }));
    }
    line.into()
}

/// The deaths the table draws, as owned data the layout lays out at the
/// width the window gives it.
#[derive(Debug, Clone)]
pub(crate) struct Table {
    /// Every death, oldest first (the raid timeline's order).
    all: Vec<RaidDeath>,
    /// What is drawn: places in `all` the filter keeps.
    drawn: Vec<usize>,
    /// The death the inspector recaps, by its place in `all`.
    selected: Option<usize>,
    hover: Option<usize>,
    /// The keys are in the inspector: the selection steps back to the
    /// hover's weight, as the meter's does.
    keys_away: bool,
    row_h: f32,
    hide_realms: bool,
    /// What the filter says, when it hides every death.
    filter: String,
    /// The reader's own character's guid when the daemon marked none of
    /// the deaths ([`owner`]): their deaths wear "you" all the same.
    owner: Option<String>,
}

impl Table {
    /// The table for the stage — `None` off the Deaths view and without a
    /// raid timeline (a stored pull whose store kept its card alone), where
    /// the count table stands.
    pub(crate) fn of(state: &Gui) -> Option<Self> {
        let app = state.fight();
        if app.view != View::Deaths {
            return None;
        }
        let raid = app.raid()?;
        Some(Table {
            drawn: drawn(raid, &state.filter),
            selected: selected(app, raid),
            hover: match state.row_hover {
                Some(RowHover::Death(i)) => Some(i),
                _ => None,
            },
            keys_away: app.inspecting(),
            row_h: state.cfg.density().row_h(),
            hide_realms: state.cfg.hide_realms,
            filter: state.filter.clone(),
            owner: owner(state, raid),
            all: raid.deaths.clone(),
        })
    }

    /// Is death `d` the reader's own ("you")?
    fn mine(&self, d: &RaidDeath) -> bool {
        is_mine(d, self.owner.as_deref())
    }

    /// Where death `i` stands in the list's content, top and bottom — what
    /// keeps a stepped selection in sight. `None` when it is not drawn.
    pub(crate) fn extent(&self, i: usize) -> Option<(f32, f32)> {
        let at = self.drawn.iter().position(|d| *d == i)?;
        let top = at as f32 * self.row_h;
        Some((top, top + self.row_h))
    }

    /// Where the player `key`'s death stands — the selected one when it is
    /// theirs, else their last.
    pub(crate) fn extent_of(&self, key: &str) -> Option<(f32, f32)> {
        let theirs = |i: &usize| self.all.get(*i).is_some_and(|d| d.guid == key);
        let at = self
            .selected
            .filter(theirs)
            .or_else(|| (0..self.all.len()).rev().find(theirs))?;
        self.extent(at)
    }

    /// The table laid out: `narrow` is the window's breakpoint, and in a
    /// narrow window a press pushes the inspector over it too (the
    /// window's `OpenDeath` decides that).
    pub(crate) fn view(self, narrow: bool) -> Element<'static, Message> {
        iced::widget::responsive(move |bounds| self.layout(narrow, bounds.height)).into()
    }

    /// The total's words: the group's deaths, the battle rezzes that undid
    /// some (a self-rez is none), and an arena's enemy deaths apart.
    fn total_words(&self) -> String {
        let ours: Vec<&RaidDeath> = self.all.iter().filter(|d| !d.enemy).collect();
        let rezzes = ours.iter().filter(|d| d.battle_rezzed()).count();
        let enemies = self.all.len() - ours.len();
        let mut label = crate::nav::plural(ours.len(), "death");
        if rezzes > 0 {
            label = format!("{label}, {}", rez_words(rezzes));
        }
        if enemies > 0 {
            label = format!("{label}, {}", crate::nav::plural(enemies, "enemy death"));
        }
        label
    }

    fn layout(&self, narrow: bool, height: f32) -> Element<'static, Message> {
        let cols = columns(narrow);
        let lead = LEAD;
        let rows_h = self.drawn.len() as f32 * self.row_h;
        // A short list takes its own height and the total follows its last
        // row; a long one scrolls with the total pinned under it — the
        // meter's own rule (`view::total_follows`). Only a list that scrolls
        // keeps its scrollbar's lane clear; a short one's rows run to the
        // border, as the prototype's do.
        let follows = crate::view::total_follows(rows_h, height);
        let trail = if follows {
            TRAIL
        } else {
            TRAIL - theme::pitch::SCROLL_LANE
        };
        let clear = |el: Element<'static, Message>| -> Element<'static, Message> {
            if follows { el } else { scroll_clear(el).into() }
        };
        let head = |words: &'static str| -> Element<'static, Message> {
            text(words)
                .size(size::LABEL)
                .color(theme::GOLD_DIM)
                .wrapping(text::Wrapping::None)
                .into()
        };
        let heads = container(cells(&cols, cols.iter().map(|c| head(c.head)).collect())).padding(
            iced::Padding {
                top: HEADS_TOP,
                right: trail,
                bottom: HEADS_BOTTOM,
                left: lead,
            },
        );

        let mut list = column![];
        if self.drawn.is_empty() {
            let words = if self.all.is_empty() {
                "Nobody died in this pull.".to_string()
            } else {
                format!(
                    "No player matches \u{201c}{}\u{201d}. Filter by name, class, spec or role, \
                     or press Esc to clear it.",
                    self.filter
                )
            };
            list = list.push(
                container(text(words).size(size::BODY).color(theme::INK_2)).padding(EMPTY_PAD),
            );
        }
        for &i in &self.drawn {
            let Some(d) = self.all.get(i) else {
                continue;
            };
            list = list.push(self.line(i, d, narrow, &cols, (lead, trail)));
        }
        let rows = scrollable(clear(list.into()))
            .id(crate::view::meter_list_id())
            .height(if follows {
                Length::Shrink
            } else {
                Length::Fill
            })
            .width(Length::Fill);
        // The label stands over the Player column: the total's own inset
        // and gap are inside the lead it is given.
        let time_w = if narrow { TIME_W_NARROW } else { TIME_W };
        let total = crate::table::total::<Message>(
            &[],
            crate::table::Grid::Meter {
                view: View::Deaths,
                narrow,
            },
            &[],
            self.total_words(),
            (lead + time_w + GAP - crate::table::TOTAL_INSET - crate::table::GAP).max(0.0),
            false,
        );
        column![
            clear(heads.into()),
            crate::nav::hairline::<Message>(),
            rows,
            total
        ]
        .height(Length::Fill)
        .into()
    }

    /// One death (`.trow`): its time, who (their disc, their name and "you"
    /// after it — no role glyph, as the prototype's `.who2` has none here —
    /// or the enemy team's word), the killing blow with its source and rez
    /// after it, the hit and the overkill.
    fn line(
        &self,
        i: usize,
        d: &RaidDeath,
        narrow: bool,
        cols: &[Column],
        (lead, trail): (f32, f32),
    ) -> Element<'static, Message> {
        let selected = self.selected == Some(i);
        let hovered = self.hover == Some(i);
        let name = if self.hide_realms {
            display_name(&d.name).to_string()
        } else {
            d.name.clone()
        };
        let mut label = crate::ellipsis::ellipsis(name)
            .size(size::NAME)
            .font(if selected {
                theme::UI_MEDIUM
            } else {
                theme::UI
            })
            .color(crate::view::name_ink(selected));
        // The owner's tag, as the meter's names wear it — and the name gives
        // way to it.
        let mine = self.mine(d);
        let tags = crate::view::name_tags(None, mine.then_some(d.class), None);
        let mut room: Vec<(String, f32, iced::Font)> = Vec::new();
        let mut px = tags.as_ref().map_or(0.0, |(_, w)| WHO_GAP + w);
        if d.enemy {
            room.push((ENEMY.to_string(), size::SMALL, theme::UI));
            px += WHO_GAP;
        }
        if px > 0.0 {
            label = label.leaving(room, px);
        }
        let mut who = row![
            crate::compare::class_icon::<Message>(d.class, d.spec, None, DISC),
            label,
        ]
        .spacing(WHO_GAP)
        .align_y(iced::Alignment::Center);
        if let Some((tags, _)) = tags {
            who = who.push(tags);
        }
        if d.enemy {
            who = who.push(
                text(ENEMY)
                    .size(size::SMALL)
                    .color(theme::INK_3_TEXT)
                    .wrapping(text::Wrapping::None),
            );
        }
        let w = words(d, self.hide_realms);
        // The killing blow, then ONE quieter run after it — the source and
        // the rez as the prototype's `.src` span joins them ("Zul'jan,
        // rezzed 2:03") — which gives every letter, down to its "…",
        // before the blow gives one: the blow is what the column is about.
        let mut kb = crate::ellipsis::ellipsis(w.blow.clone())
            .size(BLOW_PX)
            .color(if w.note {
                theme::INK_3_TEXT
            } else {
                theme::INK
            });
        if let Some(after) = w.after() {
            kb = kb
                .tail(after, size::SMALL, theme::INK_3_TEXT, KB_GAP)
                .giving(crate::ellipsis::Give::First);
        }
        let blow: Element<'static, Message> = kb.into();
        let mut line: Vec<Element<'static, Message>> = vec![
            text(duration(d.at_ms))
                .size(size::BODY)
                .color(theme::INK_2)
                .wrapping(text::Wrapping::None)
                .into(),
            who.into(),
            blow,
        ];
        if !narrow {
            let num = |s: String, px: f32, ink| -> Element<'static, Message> {
                text(s)
                    .size(px)
                    .color(ink)
                    .wrapping(text::Wrapping::None)
                    .into()
            };
            // A cheat death's "hit" of 1 is the log's bookkeeping, not a
            // blow: its cell is left empty, as a death without damage is.
            let hit = if d.hit > 0 && !w.cheat {
                commas(d.hit)
            } else {
                String::new()
            };
            let over = d.overkill.map(commas).unwrap_or_default();
            line.push(num(hit, size::NUM, theme::INK));
            line.push(num(over, size::SMALL, theme::BAD));
        }
        let quiet = self.keys_away;
        let line = container(cells(cols, line))
            .height(Length::Fixed(self.row_h))
            .align_y(iced::Alignment::Center)
            .padding(iced::Padding {
                top: 0.0,
                right: trail,
                bottom: 0.0,
                left: lead,
            })
            .width(Length::Fill)
            .style(move |_: &Theme| {
                if selected && !quiet {
                    row_style_in(&Look::WINDOW, true)
                } else {
                    hover_style_in(&Look::WINDOW, hovered || selected)
                }
            });
        mouse_area(line)
            .on_press(Message::OpenDeath(Pick {
                key: d.guid.clone(),
                label: d.name.clone(),
                index: d.index,
            }))
            .on_enter(Message::HoverRow(Some(RowHover::Death(i))))
            .on_exit(Message::HoverRow(None))
            .into()
    }
}

/// "2 battle rezzes", "1 battle rez".
fn rez_words(n: usize) -> String {
    if n == 1 {
        "1 battle rez".to_string()
    } else {
        format!("{n} battle rezzes")
    }
}

/// A death in words: the killing blow and what follows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Words {
    /// The killing blow ("Venom Rupture"), a cheat death's ("Purgatory ran
    /// out") or the note that there was none ([`NO_DAMAGE`]).
    pub blow: String,
    /// `blow` is the note, not an ability.
    pub note: bool,
    /// Who dealt it: "Zul'jan", "self" for their own, "cheat death" for a
    /// cheat death running out, nothing for a nil source or no damage.
    pub source: String,
    /// "rezzed 2:03", when a rez raised them.
    pub rez: Option<String>,
    /// A cheat death ran out (their own blow of 1 health or less): the
    /// hit is the log's bookkeeping, and no figure is shown for it.
    pub cheat: bool,
}

impl Words {
    /// What follows the blow, as one run (the prototype's `.src`: `${src},
    /// rezzed ${t}`): "Zul'jan, rezzed 2:03", "Zul'jan", "rezzed 4:25" —
    /// `None` when neither is there.
    pub(crate) fn after(&self) -> Option<String> {
        match (self.source.is_empty(), &self.rez) {
            (true, None) => None,
            (false, None) => Some(self.source.clone()),
            (true, Some(rez)) => Some(rez.clone()),
            (false, Some(rez)) => Some(format!("{}, {rez}", self.source)),
        }
    }
}

/// The reader's own character's guid among `raid`'s deaths when the daemon
/// marked none of them (`RaidDeath.mine`): its store off, or not yet
/// published — found as [`Gui::owner_of`] finds the meter's "you" (the
/// locked character, then the configured `history_characters` and the
/// accent's name), so the skull, the table, the header and the meter agree
/// on who the reader is. `None` when the daemon's marks stand.
pub(crate) fn owner(state: &Gui, raid: &RaidTimeline) -> Option<String> {
    if raid.deaths.iter().any(|d| d.mine) {
        return None;
    }
    let rows: Vec<Row> = raid
        .deaths
        .iter()
        .map(|d| Row {
            key: d.guid.clone(),
            label: d.name.clone(),
            enemy: d.enemy,
            ..Row::default()
        })
        .collect();
    state
        .owner_of(&rows)
        .and_then(|i| rows.get(i))
        .map(|r| r.key.clone())
}

/// Is `d` the reader's own death — the daemon's mark, or the guid
/// [`owner`] found — and never an arena enemy's?
pub(crate) fn is_mine(d: &RaidDeath, owner: Option<&str>) -> bool {
    !d.enemy && (d.mine || owner == Some(d.guid.as_str()))
}

/// A death's words: its killing blow, its source and its rez — "Venom
/// Rupture" / "Zul'jan" / "rezzed 2:03". A blow the player dealt themselves
/// for 1 health or less is a cheat death running out (Purgatory: the log
/// writes its end as a self-kill), worded as the prototype words it —
/// "Purgatory ran out", "cheat death"; any other of their own is "self"; a
/// player source loses its realm when the options say so.
pub(crate) fn words(d: &RaidDeath, hide_realms: bool) -> Words {
    let own = !d.source.is_empty() && d.source == d.name;
    let cheat = own && !d.blow.is_empty() && d.hit <= 1;
    let (blow, note) = if d.blow.is_empty() {
        (NO_DAMAGE.to_string(), true)
    } else if cheat {
        (format!("{} ran out", d.blow), false)
    } else {
        (d.blow.clone(), false)
    };
    let source = if cheat {
        "cheat death".to_string()
    } else if own {
        "self".to_string()
    } else if hide_realms {
        display_name(&d.source).to_string()
    } else {
        d.source.clone()
    };
    Words {
        blow,
        note,
        source,
        rez: d
            .rez
            .as_ref()
            .map(|r| format!("rezzed {}", duration(r.at_ms))),
        cheat,
    }
}

/// The deaths the filter keeps, by place in `raid.deaths` — matched as the
/// meter's rows are, by name, class, spec or role.
pub(crate) fn drawn(raid: &RaidTimeline, filter: &str) -> Vec<usize> {
    let rows: Vec<Row> = raid
        .deaths
        .iter()
        .map(|d| Row {
            label: d.name.clone(),
            class: d.class,
            spec: d.spec,
            ..Row::default()
        })
        .collect();
    crate::view::filtered_indexed(rows, filter)
        .into_iter()
        .map(|(i, _)| i)
        .collect()
}

/// The death the inspector recaps, by its place in `raid.deaths`: the
/// drilled player's window the Deaths drill asked for, or answered with —
/// their last, when it names none.
pub(crate) fn selected(app: &ClientState, raid: &RaidTimeline) -> Option<usize> {
    let key = &app.drill.as_ref()?.key;
    let want = app.death_request().or_else(|| app.deaths().1);
    match want {
        Some(index) => raid
            .deaths
            .iter()
            .position(|d| d.guid == *key && d.index == index),
        None => raid.deaths.iter().rposition(|d| d.guid == *key),
    }
}

/// "−4.25s": a recap event's time before the death (R9, v35 `offset_ms`) —
/// in hundredths under ten seconds, where a raid's last events crowd (a
/// ring of 32 can span half a second of heals and hits), in tenths under a
/// minute ("−12.4s"), and "−1:05" a minute or more before; "0.00s" on the
/// death's own moment.
pub(crate) fn before(ms: i64) -> String {
    let back = ms.unsigned_abs();
    let secs = back as f64 / 1000.0;
    // Under 5 ms it rounds to the death's own moment: no sign on a zero.
    if back < 5 {
        "0.00s".to_string()
    } else if back < 10_000 {
        format!("\u{2212}{secs:.2}s")
    } else if back < 60_000 {
        format!("\u{2212}{secs:.1}s")
    } else {
        format!("\u{2212}{}", duration(back as i64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::testkit as tk;
    use wowdps_model::{Class, Rez};

    fn death(guid: &str, at_ms: i64, index: u32) -> RaidDeath {
        RaidDeath {
            guid: guid.to_string(),
            name: format!("{guid}-Realm-US"),
            class: Some(Class::Hunter),
            spec: None,
            index,
            at_ms,
            blow: "Venom Rupture".to_string(),
            source: "Zul'jan".to_string(),
            hit: 203_042,
            overkill: Some(13_485),
            rez: None,
            mine: false,
            enemy: false,
        }
    }

    fn pair(w: &Words) -> (&str, &str) {
        (w.blow.as_str(), w.source.as_str())
    }

    /// The killing blow, then its source, then the rez apart: every
    /// combination reads as one phrase, a death with no hit says so in a
    /// note's ink, and a cheat death running out is worded as one.
    #[test]
    fn a_death_reads_as_its_blow_its_source_and_its_rez() {
        let mut d = death("Tueur", 70_000, 0);
        let w = words(&d, true);
        assert_eq!(pair(&w), ("Venom Rupture", "Zul'jan"));
        assert!(!w.note && !w.cheat && w.rez.is_none());
        // Their own: "self" — and for 1 health or less, a cheat death ran
        // out (Purgatory's end is logged as a self-kill).
        let own = RaidDeath {
            source: d.name.clone(),
            ..d.clone()
        };
        assert_eq!(words(&own, true).source, "self");
        let purgatory = RaidDeath {
            blow: "Purgatory".into(),
            source: d.name.clone(),
            hit: 1,
            overkill: None,
            ..d.clone()
        };
        let w = words(&purgatory, true);
        assert_eq!(pair(&w), ("Purgatory ran out", "cheat death"));
        assert!(w.cheat);
        // A player's blow loses its realm when the options say so.
        let pvp = RaidDeath {
            source: "Bo-Realm-US".into(),
            ..d.clone()
        };
        assert_eq!(words(&pvp, true).source, "Bo");
        assert_eq!(words(&pvp, false).source, "Bo-Realm-US");
        d.rez = Some(Rez {
            at_ms: 123_200,
            by: "Player-2".into(),
            by_name: "Soundscape-Realm-US".into(),
            spell: "Intercession".into(),
        });
        let w = words(&d, true);
        assert_eq!(pair(&w), ("Venom Rupture", "Zul'jan"));
        assert_eq!(w.rez.as_deref(), Some("rezzed 2:03"));
        d.blow.clear();
        d.source.clear();
        d.rez = None;
        let w = words(&d, true);
        assert_eq!(pair(&w), (NO_DAMAGE, ""));
        assert!(w.note, "a note, not an ability");
    }

    /// The time before a death, as the recap's column reads it.
    #[test]
    fn a_recap_event_reads_its_time_before_the_death() {
        assert_eq!(before(0), "0.00s");
        assert_eq!(before(-3), "0.00s", "a zero wears no sign");
        assert_eq!(before(-346), "\u{2212}0.35s");
        assert_eq!(before(-1_234), "\u{2212}1.23s");
        assert_eq!(before(-12_450), "\u{2212}12.4s");
        assert_eq!(before(-65_000), "\u{2212}1:05");
    }

    /// The filter narrows the deaths as it narrows the meter, keeping
    /// their order.
    #[test]
    fn the_filter_keeps_the_order_of_the_deaths_it_draws() {
        let raid = RaidTimeline {
            view: View::Taken,
            bucket_ms: 1000,
            series: Vec::new(),
            deaths: vec![
                death("Tueur", 1_000, 0),
                death("Mehna", 2_000, 0),
                death("Tueur", 3_000, 1),
            ],
            lust: Vec::new(),
        };
        assert_eq!(drawn(&raid, ""), [0, 1, 2]);
        assert_eq!(drawn(&raid, "tue"), [0, 2]);
        assert_eq!(drawn(&raid, "hunter"), [0, 1, 2], "by class too");
        assert!(drawn(&raid, "zzz").is_empty());
    }

    /// The raid kill's window on its Deaths view, with the raid timeline in
    /// hand (as the live daemon sends it).
    fn on_deaths() -> (Gui, std::os::unix::net::UnixStream) {
        let mut state = tk::raided(25);
        state.view = View::Deaths;
        tk::gui_over(state)
    }

    /// The table lists the deaths in the order they happened — not the
    /// Deaths meter's per-player count — with the prototype's heads, each
    /// death's time, killing blow, source and rez, a cheat death worded as
    /// one with no hit, and a total that counts the deaths and the battle
    /// rezzes; narrow, the hit and the overkill go.
    #[test]
    fn the_table_lists_the_deaths_in_the_order_they_happened() {
        let (gui, _peer) = on_deaths();
        let table = Table::of(&gui).expect("the Deaths view with a raid timeline");
        let mut ui = tk::wide(table.clone().view(false));
        for head in ["Time", "Player", "Killing blow", "Hit", "Overkill"] {
            assert!(ui.find(head).is_ok(), "{head}");
        }
        let ys: Vec<f32> = [
            "Raider3", "Raider5", "Raider7", "Raider9", "Raider16", "Raider20",
        ]
        .iter()
        .map(|n| {
            ui.find(format!("{n}-Realm-US").as_str())
                .map(|t| t.bounds().y)
                .unwrap_or(f32::NAN)
        })
        .collect();
        assert!(ys.windows(2).all(|w| w[0] < w[1]), "in order: {ys:?}");
        for words in [
            "1:10",
            "Venom Rupture",
            "Zul'jan",
            "Zul'jan, rezzed 2:03",
            "rezzed 4:25",
            NO_DAMAGE,
            "Purgatory ran out",
            "cheat death",
            "150,016",
            "10,016",
            "6 deaths, 2 battle rezzes",
            "you",
        ] {
            assert!(ui.find(words).is_ok(), "{words}");
        }
        // A cheat death's "hit" of 1 is no figure.
        assert!(ui.find("1").is_err(), "no hit of 1");
        let mut narrow = tk::simulator(table.view(true));
        assert!(narrow.find("Killing blow").is_ok());
        assert!(narrow.find("Hit").is_err() && narrow.find("Overkill").is_err());
    }

    /// The heads, the rows and the total start 4 px in from the meter's
    /// edge, as the meter's rank column does (`.thead,.trow{padding:0 16px
    /// 0 4px}`) — wide and narrow alike.
    #[test]
    fn the_table_starts_at_the_meters_edge() {
        let (gui, _peer) = on_deaths();
        let table = Table::of(&gui).expect("a table");
        let mut ui = tk::wide(table.clone().view(false));
        let time = ui.find("Time").expect("the head").bounds();
        assert_eq!(time.x, LEAD);
        let first = ui.find("1:10").expect("the first death").bounds();
        assert_eq!(first.x, LEAD);
        let mut ui = tk::simulator(table.view(true));
        let time = ui.find("Time").expect("the head").bounds();
        assert_eq!(time.x, LEAD);
    }

    /// The killing blow is the last thing to give way: at the tile's width
    /// (a 960 px window's stage beside its 410 px inspector, ~549 px of
    /// table) the blow stands whole while its source and rez — one run —
    /// give letters, and are still drawn, cut, rather than dropped.
    #[test]
    fn the_blow_is_never_cut_while_its_tail_has_room_to_give() {
        let (gui, _peer) = on_deaths();
        let table = Table::of(&gui).expect("a table");
        let mut ui = tk::simulator_as(
            crate::window::settings(),
            iced::Size::new(549.0, 400.0),
            table.view(false),
        );
        let full = crate::ellipsis::width_of::<
            <iced::Renderer as iced::advanced::text::Renderer>::Paragraph,
        >("Venom Rupture", BLOW_PX, theme::UI);
        let blow = ui.find("Venom Rupture").expect("the blow").bounds();
        assert!(
            blow.width >= full - 0.5,
            "the blow is whole: {} of {full}",
            blow.width
        );
        assert!(
            ui.find("Zul'jan, rezzed 2:03").is_ok(),
            "the source and rez give way, drawn cut"
        );
    }

    /// A daemon that marks nobody (its store off, not yet published): the
    /// window's own owner — its locked character, or a configured name —
    /// is "you" on the table as it is on the meter; with neither, nobody.
    #[test]
    fn the_owners_death_is_you_when_the_daemon_marks_nobody() {
        let (mut gui, _peer) = tk::gui_over(tk::raided_unmarked(25, View::Deaths));
        let table = Table::of(&gui).expect("a table");
        assert_eq!(table.owner, None, "nothing marked, nothing locked");
        assert!(tk::wide(table.view(false)).find("you").is_err());
        gui.owner_guid = Some("Player-1-16".to_string());
        let table = Table::of(&gui).expect("a table");
        assert_eq!(table.owner.as_deref(), Some("Player-1-16"), "the lock");
        assert!(tk::wide(table.view(false)).find("you").is_ok());
        gui.owner_guid = None;
        gui.cfg.extra.insert(
            "history_characters".to_string(),
            toml::Value::String("Raider16".to_string()),
        );
        let table = Table::of(&gui).expect("a table");
        assert_eq!(table.owner.as_deref(), Some("Player-1-16"), "the name");
        // And the ribbon agrees.
        let r = crate::ribbon::Ribbon::of(&gui).expect("a ribbon");
        assert_eq!(
            r.skulls.iter().filter(|s| s.mine).count(),
            1,
            "the owner's skull says you"
        );
        // A daemon that marked a death is believed over the window's hints.
        let (gui, _peer) = on_deaths();
        assert_eq!(Table::of(&gui).and_then(|t| t.owner), None);
    }

    /// A press on a death opens it: that player's recap, at that window.
    #[test]
    fn a_press_on_a_death_opens_its_recap() {
        let (gui, _peer) = on_deaths();
        let table = Table::of(&gui).expect("a table");
        let mut ui = tk::wide(table.view(false));
        let _ = ui.click("Raider16-Realm-US");
        let got: Vec<Message> = ui.into_messages().collect();
        assert!(
            got.iter().any(|m| matches!(
                m,
                Message::OpenDeath(Pick { key, index: 0, .. }) if key == "Player-1-16"
            )),
            "{got:?}"
        );
    }

    /// An arena's other team dies in the same list — tagged "enemy" and
    /// counted apart, never among the group's deaths — and a self-rez is
    /// no battle rez.
    #[test]
    fn an_enemy_death_is_tagged_and_a_self_rez_is_no_battle_rez() {
        let mut state = tk::raided(25);
        state.view = View::Deaths;
        let (gui, _peer) = tk::gui_over(state);
        let mut table = Table::of(&gui).expect("a table");
        let mut foe = death("Player-2-X", 400_000, 0);
        foe.enemy = true;
        foe.name = "Xar-Realm-US".into();
        table.all.push(foe);
        table.drawn.push(table.all.len() - 1);
        // Raider3's rez becomes their own (an Ankh): no battle rez.
        let own = table.all[0].guid.clone();
        if let Some(r) = table.all[0].rez.as_mut() {
            r.by = own;
        }
        assert_eq!(table.total_words(), "6 deaths, 1 battle rez, 1 enemy death");
        let mut ui = tk::wide(table.view(false));
        assert!(ui.find("enemy").is_ok());
    }

    /// Off the Deaths view, or with no raid timeline (a store that kept a
    /// pull's card alone), the count table stands.
    #[test]
    fn the_table_needs_the_deaths_view_and_a_timeline() {
        let (gui, _peer) = tk::gui_over(tk::raided(25));
        assert!(Table::of(&gui).is_none(), "the Damage view");
        let mut state = tk::raid(25);
        state.view = View::Deaths;
        let (gui, _peer) = tk::gui_over(state);
        assert!(Table::of(&gui).is_none(), "no timeline");
    }
}
