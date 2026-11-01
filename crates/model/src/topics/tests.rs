use super::*;
use crate::session::SessionStatus;
use crate::{HarnessId, NativeSessionId};

const HOUR: i64 = 60 * 60 * 1000;
const WINDOWS: Windows = Windows {
    now_ms: 100 * HOUR,
    active_ms: 24 * HOUR,
    recent_ms: 72 * HOUR,
};

fn id(native: &str) -> SessionId {
    SessionId::new(HarnessId::new("claude-code"), NativeSessionId::new(native))
}

fn session(native: &str, hours_ago: i64, parent: Option<&str>) -> SessionSummary {
    SessionSummary {
        id: id(native),
        title: None,
        status: SessionStatus::Idle,
        model: None,
        effort: None,
        cwd: None,
        updated_at_ms: WINDOWS.now_ms - hours_ago * HOUR,
        parent: parent.map(id),
        task: None,
        node: None,
        blocked: false,
    }
}

fn topic(slug: &str, parent: Option<&str>) -> Topic {
    Topic {
        id: TopicId::new(slug),
        slug: slug.into(),
        name: slug.into(),
        parent: parent.map(TopicId::new),
        last_active_ms: None,
        updated_ms: 0,
        closed_ms: None,
        directory: None,
        owner: None,
        visibility: None,
    }
}

fn slugs<'a>(cards: &[Card<'a>]) -> Vec<&'a str> {
    cards.iter().map(|card| card.topic.slug.as_str()).collect()
}

fn natives(sessions: &[&SessionSummary]) -> Vec<String> {
    sessions
        .iter()
        .map(|session| session.id.native().as_str().to_owned())
        .collect()
}

#[test]
fn placed_sessions_file_under_their_root_topic_and_subagents_follow_their_parent() {
    let delegated = SessionSummary {
        task: Some(crate::TaskId::new("task-1")),
        ..session("delegated", 1, Some("lead"))
    };
    let topics = [
        topic("desktop", None),
        topic("desktop-chat", Some("desktop")),
        topic("mesh", None),
    ];
    let sessions = [
        session("lead", 1, None),
        session("helper", 1, Some("lead")),
        delegated,
        session("chat", 2, None),
        session("certs", 200, None),
    ];
    let placements = Placements::from([
        (id("lead"), TopicId::new("desktop")),
        (id("chat"), TopicId::new("desktop-chat")),
        (id("certs"), TopicId::new("mesh")),
    ]);
    let board = board(
        &topics,
        &sessions,
        &placements,
        &HistoryMarks::new(),
        &HashSet::new(),
        WINDOWS,
    );
    assert_eq!(board.recent.len(), 1);
    assert_eq!(board.recent[0].topic.slug, "desktop");
    // A delegated child is a session of its own in its parent's topic; a subagent goes with its parent.
    assert_eq!(natives(&board.recent[0].sessions), ["lead", "delegated", "chat"]);
    assert_eq!(
        board
            .past
            .iter()
            .map(|card| card.topic.slug.as_str())
            .collect::<Vec<_>>(),
        ["mesh"]
    );
    assert!(board.uncategorized.is_empty() && board.history.is_empty());
}

#[test]
fn a_topic_nests_anywhere_but_its_own_subtree_and_never_past_four_levels() {
    let topics = [
        topic("a", None),
        topic("b", Some("a")),
        topic("c", Some("b")),
        topic("d", Some("c")),
        topic("x", None),
        topic("y", Some("x")),
    ];
    let placements = Placements::new();
    let atlas = Atlas::new(&topics, &[], &placements);
    let id = TopicId::new;
    assert_eq!(
        (atlas.lineage(&topics[3].id).len(), atlas.height(&id("a"))),
        (4, 4)
    );
    assert!(atlas.can_nest(&id("y"), &topics[2].id));
    assert!(!atlas.can_nest(&id("y"), &topics[3].id));
    assert!(atlas.can_nest(&id("x"), &topics[1].id));
    assert!(!atlas.can_nest(&id("x"), &topics[2].id));
    assert!(!atlas.can_nest(&id("a"), &topics[2].id));
    assert!(!atlas.can_nest(&id("b"), &topics[1].id));
    assert_eq!(
        atlas.nesting(&id("b"), &topics[2].id),
        Err(Unnestable::OwnSubtree)
    );
    assert_eq!(atlas.nesting(&id("x"), &topics[2].id), Err(Unnestable::TooDeep));
}

#[test]
fn a_typed_directory_is_a_full_path_on_one_node_or_every_node() {
    assert_eq!(
        Directory::typed(" /srv/work/ ", Some("ct115".into())),
        Some(Directory::Local {
            node: "ct115".into(),
            path: "/srv/work".into()
        })
    );
    assert_eq!(
        Directory::typed("/", None),
        Some(Directory::Shared { path: "/".into() })
    );
    assert_eq!(Directory::typed("~/work", None), None);
    assert_eq!(Directory::typed("", Some("ct115".into())), None);
}

#[test]
fn names_fold_to_the_slugs_rh_files_them_under() {
    assert_eq!(slug("Alpha Sub"), "alpha-sub");
    assert_eq!(slug("  alpha--SUB! "), "alpha-sub");
    assert_eq!(slug("Café 2.0"), "café-2-0");
    assert_eq!(slug("?!"), "");
}

#[test]
fn a_live_session_is_active_however_quiet_and_a_card_counts_its_history() {
    let topics = [topic("desk", None)];
    let sessions = [
        session("lead", 2, None),
        session("old", 30, None),
        session("live", 30, None),
        session("loose", 40, None),
    ];
    let placements = Placements::from([
        (id("lead"), TopicId::new("desk")),
        (id("old"), TopicId::new("desk")),
        (id("live"), TopicId::new("desk")),
    ]);
    let live = HashSet::from([id("live"), id("loose")]);
    let board = board(
        &topics,
        &sessions,
        &placements,
        &HistoryMarks::new(),
        &live,
        WINDOWS,
    );
    assert_eq!(natives(&board.recent[0].sessions), ["lead", "live"]);
    assert_eq!(board.recent[0].history, 1);
    assert_eq!(natives(&board.uncategorized), ["loose"]);
}

#[test]
fn a_subtopic_works_in_its_own_directory_or_the_nearest_one_above() {
    let mut root = topic("root", None);
    root.directory = Some(Directory::Shared {
        path: "/shared/root".into(),
    });
    let mut own = topic("own", Some("root"));
    own.directory = Some(Directory::Local {
        node: "ct115".into(),
        path: "/work/own".into(),
    });
    let topics = [root, own, topic("plain", Some("root")), topic("loose", None)];
    let placements = Placements::new();
    let atlas = Atlas::new(&topics, &[], &placements);
    let path = |index: usize| match atlas.directory(&topics[index].id) {
        Some(Directory::Local { path, .. } | Directory::Shared { path }) => Some(path.as_str()),
        None => None,
    };
    assert_eq!(
        [path(1), path(2), path(3)],
        [Some("/work/own"), Some("/shared/root"), None]
    );
}

#[test]
fn a_topic_counts_as_active_from_its_last_change() {
    let mut fresh = topic("fresh", None);
    fresh.updated_ms = WINDOWS.now_ms - HOUR;
    let topics = [fresh, topic("old", None)];
    let placements = Placements::new();
    let board = board(
        &topics,
        &[],
        &placements,
        &HistoryMarks::new(),
        &HashSet::new(),
        WINDOWS,
    );
    assert_eq!(slugs(&board.recent), ["fresh"]);
    assert_eq!(slugs(&board.past), ["old"]);
}

#[test]
fn a_closed_topic_is_past_until_one_of_its_sessions_is_active_again() {
    let mut desktop = topic("desktop", None);
    desktop.closed_ms = Some(WINDOWS.now_ms - 2 * HOUR);
    let topics = [desktop];
    let placements = Placements::from([(id("lead"), TopicId::new("desktop"))]);
    let marks = HistoryMarks::new();
    let quiet = [session("lead", 3, None)];
    let closed = board(&topics, &quiet, &placements, &marks, &HashSet::new(), WINDOWS);
    assert_eq!(slugs(&closed.recent), Vec::<&str>::new());
    assert_eq!(slugs(&closed.past), ["desktop"]);
    let busy = [session("lead", 1, None)];
    let reopened = board(&topics, &busy, &placements, &marks, &HashSet::new(), WINDOWS);
    assert_eq!(slugs(&reopened.recent), ["desktop"]);
}

#[test]
fn a_session_marked_as_history_leaves_its_lane_until_it_is_active_again() {
    let topics = [topic("desktop", None)];
    let placements = Placements::from([
        (id("done"), TopicId::new("desktop")),
        (id("lead"), TopicId::new("desktop")),
    ]);
    let marks = HistoryMarks::from([
        (id("done"), WINDOWS.now_ms - HOUR),
        (id("loose"), WINDOWS.now_ms - HOUR),
        (id("back"), WINDOWS.now_ms - 3 * HOUR),
    ]);
    let sessions = [
        session("done", 2, None),
        session("lead", 2, None),
        session("loose", 2, None),
        session("back", 2, None),
    ];
    let board = board(&topics, &sessions, &placements, &marks, &HashSet::new(), WINDOWS);
    assert_eq!(natives(&board.recent[0].sessions), ["lead"]);
    assert_eq!(natives(&board.uncategorized), ["back"]);
    assert_eq!(natives(&board.history), ["loose"]);
}

#[test]
fn a_side_track_placed_elsewhere_files_under_its_own_topic() {
    let topics = [topic("desktop", None), topic("mesh", None)];
    let sessions = [session("lead", 1, None), session("detour", 1, Some("lead"))];
    let placements = Placements::from([
        (id("lead"), TopicId::new("desktop")),
        (id("detour"), TopicId::new("mesh")),
    ]);
    let board = board(
        &topics,
        &sessions,
        &placements,
        &HistoryMarks::new(),
        &HashSet::new(),
        WINDOWS,
    );
    let mesh = board
        .recent
        .iter()
        .find(|card| card.topic.slug == "mesh")
        .map(|card| natives(&card.sessions));
    assert_eq!(mesh, Some(vec!["detour".to_owned()]));
}

#[test]
fn unplaced_sessions_are_uncategorized_while_young_then_history() {
    let sessions = [
        session("fresh", 3, None),
        session("stale", 30, None),
        session("child", 3, Some("fresh")),
        session("old-child", 30, Some("stale")),
    ];
    let placements = Placements::new();
    let board = board(
        &[],
        &sessions,
        &placements,
        &HistoryMarks::new(),
        &HashSet::new(),
        WINDOWS,
    );
    assert_eq!(natives(&board.uncategorized), ["fresh"]);
    assert_eq!(natives(&board.history), ["stale", "old-child"]);
}

#[test]
fn the_outline_puts_each_subtopic_under_its_root_and_the_busiest_root_first() {
    let mut quiet = topic("mesh", None);
    quiet.last_active_ms = Some(1);
    let mut busy = topic("desktop", None);
    busy.last_active_ms = Some(9);
    let topics = [
        topic("desktop-chat", Some("desktop")),
        quiet,
        topic("orphan", Some("gone")),
        busy,
    ];
    let rows: Vec<(usize, &str)> = outline(&topics)
        .into_iter()
        .map(|(depth, topic)| (depth, topic.slug.as_str()))
        .collect();
    assert_eq!(
        rows,
        [(0, "desktop"), (1, "desktop-chat"), (0, "mesh"), (0, "orphan")]
    );
}

#[test]
fn suggestions_rank_prefix_matches_before_substrings_then_recency() {
    let mut quiet = topic("desktop-chat", None);
    quiet.last_active_ms = Some(1);
    let mut busy = topic("desktop", None);
    busy.last_active_ms = Some(9);
    let topics = [topic("rh-desktop", None), quiet, busy, topic("mesh", None)];
    let names = |query| {
        suggest(&topics, query, 5)
            .iter()
            .map(|topic| topic.slug.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(names("desk"), ["desktop", "desktop-chat", "rh-desktop"]);
    assert_eq!(names("zzz"), Vec::<String>::new());
}
