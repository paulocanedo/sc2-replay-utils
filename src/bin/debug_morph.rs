/// Debug: rastreia ciclo de vida de CC/Orbital/PF para entender como morphs
/// aparecem nos tracker events.
use std::collections::HashMap;
use s2protocol::tracker_events::{unit_tag, ReplayTrackerEvent};

fn is_interesting(name: &str) -> bool {
    let n = name.to_lowercase();
    n.contains("command") || n.contains("orbital") || n.contains("planetary")
        || n.contains("nexus") || name == "SCV" || name == "Probe"
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Uso: debug_morph <arquivo.SC2Replay>");
        std::process::exit(1);
    }
    let path = &args[1];

    let (mpq, fc) = s2protocol::read_mpq(path).expect("read_mpq");
    let events = s2protocol::read_tracker_events(path, &mpq, &fc).expect("tracker");

    // tag → unit_type_name (para resolver UnitDone/UnitDied)
    let mut tag_map: HashMap<i64, String> = HashMap::new();
    // tags que nos interessam
    let mut interesting_tags: std::collections::HashSet<i64> = std::collections::HashSet::new();

    let mut game_loop: u32 = 0;
    for ev in events {
        game_loop += ev.delta;

        match &ev.event {
            ReplayTrackerEvent::UnitBorn(e) => {
                let tag = unit_tag(e.unit_tag_index, e.unit_tag_recycle);
                tag_map.insert(tag, e.unit_type_name.clone());
                if is_interesting(&e.unit_type_name) {
                    interesting_tags.insert(tag);
                    let ability = e.creator_ability_name.as_deref().unwrap_or("<none>");
                    println!(
                        "loop={:>6}  UnitBorn     tag={:<12}  type={:<28}  player={}  ability={}",
                        game_loop, tag, e.unit_type_name, e.control_player_id, ability
                    );
                }
            }
            ReplayTrackerEvent::UnitInit(e) => {
                let tag = unit_tag(e.unit_tag_index, e.unit_tag_recycle);
                tag_map.insert(tag, e.unit_type_name.clone());
                if is_interesting(&e.unit_type_name) {
                    interesting_tags.insert(tag);
                    println!(
                        "loop={:>6}  UnitInit     tag={:<12}  type={:<28}  player={}",
                        game_loop, tag, e.unit_type_name, e.control_player_id
                    );
                }
            }
            ReplayTrackerEvent::UnitDone(e) => {
                let tag = unit_tag(e.unit_tag_index, e.unit_tag_recycle);
                if interesting_tags.contains(&tag) {
                    let unit_type = tag_map.get(&tag).map(|s| s.as_str()).unwrap_or("???");
                    println!(
                        "loop={:>6}  UnitDone     tag={:<12}  type={:<28}",
                        game_loop, tag, unit_type
                    );
                }
            }
            ReplayTrackerEvent::UnitDied(e) => {
                let tag = unit_tag(e.unit_tag_index, e.unit_tag_recycle);
                if interesting_tags.contains(&tag) {
                    let unit_type = tag_map.get(&tag).map(|s| s.as_str()).unwrap_or("???");
                    println!(
                        "loop={:>6}  UnitDied     tag={:<12}  type={:<28}  killer={:?}",
                        game_loop, tag, unit_type, e.killer_player_id
                    );
                }
            }
            ReplayTrackerEvent::UnitTypeChange(e) => {
                let tag = unit_tag(e.unit_tag_index, e.unit_tag_recycle);
                let old_type = tag_map.get(&tag).map(|s| s.as_str()).unwrap_or("???");
                if interesting_tags.contains(&tag) || is_interesting(&e.unit_type_name) {
                    interesting_tags.insert(tag);
                    println!(
                        "loop={:>6}  UnitTypeChange tag={:<12}  old={:<20} -> new={:<20}",
                        game_loop, tag, old_type, e.unit_type_name
                    );
                    tag_map.insert(tag, e.unit_type_name.clone());
                }
            }
            _ => {}
        }
    }
}
