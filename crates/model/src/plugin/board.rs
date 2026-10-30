use std::collections::VecDeque;

use crate::plugin::canvas::{End, Event, Item, Op, Placed, Point, Rect, Scene, Viewport};

pub const MIN_ZOOM: f32 = 0.1;
pub const MAX_ZOOM: f32 = 4.;
/// The room a fitted view leaves around the content, in screen pixels.
const FIT_MARGIN: f32 = 32.;

/// What is under a point of the canvas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hit {
    Anchor { node: String, anchor: String },
    Node(String),
    Connector(String),
}

impl Hit {
    pub fn item(&self) -> &str {
        match self {
            Self::Anchor { node, .. } => node,
            Self::Node(id) | Self::Connector(id) => id,
        }
    }
}

/// A canvas as the person works on it: the plug-in's scene with its changes, what is selected,
/// where the view is, and what the plug-in hasn't answered yet: connectors drawn and items
/// deleted.
#[derive(Debug, Clone)]
pub struct Board {
    scene: Scene,
    selection: Vec<String>,
    viewport: Option<Viewport>,
    provisional: Vec<(End, End)>,
    hidden: Vec<String>,
}

impl Board {
    pub fn new(scene: Scene) -> Self {
        Self {
            viewport: scene.viewport,
            scene,
            selection: Vec::new(),
            provisional: Vec::new(),
            hidden: Vec::new(),
        }
    }

    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    /// Takes the scene a fetch of the view brought, keeping the view and the selection of the
    /// items still in it.
    pub fn renew(&mut self, scene: Scene) {
        self.scene = scene;
        let scene = &self.scene;
        self.selection.retain(|id| scene.item(id).is_some());
    }

    pub fn viewport(&self) -> Option<Viewport> {
        self.viewport
    }

    /// The view, fitting the content into a view of `size` the first time it is asked for.
    pub fn view_in(&mut self, size: Point) -> Viewport {
        *self.viewport.get_or_insert_with(|| fitted(&self.scene, size))
    }

    pub fn fit(&mut self, size: Point) {
        self.viewport = Some(fitted(&self.scene, size));
    }

    /// Moves the view by `by` screen pixels, the content following the pointer.
    pub fn pan(&mut self, by: Point) {
        if let Some(view) = self.viewport.as_mut() {
            view.x -= by.x / view.zoom;
            view.y -= by.y / view.zoom;
        }
    }

    /// Zooms by `factor`, keeping the canvas point under `at` (in screen pixels from the view's
    /// top-left) where it is.
    pub fn zoom_at(&mut self, at: Point, factor: f32) {
        let Some(view) = self.viewport.as_mut() else {
            return;
        };
        let held = Point::new(view.x + at.x / view.zoom, view.y + at.y / view.zoom);
        view.zoom = (view.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        view.x = held.x - at.x / view.zoom;
        view.y = held.y - at.y / view.zoom;
    }

    /// The canvas point at `at` screen pixels from the view's top-left.
    pub fn to_canvas(&self, at: Point) -> Point {
        self.viewport.map_or(at, |view| {
            Point::new(view.x + at.x / view.zoom, view.y + at.y / view.zoom)
        })
    }

    /// The topmost item under `at`, or one of its anchors when `anchors`; `slack` is how near
    /// counts as on, in canvas units. Decorative items are never hit.
    pub fn hit(&self, at: Point, slack: f32, anchors: bool) -> Option<Hit> {
        if anchors {
            let anchor = self.shown().rev().find_map(|item| match item {
                Item::Node(node) if !node.decorative => node
                    .anchors
                    .iter()
                    .find(|anchor| node.anchor_point(anchor).distance(at) <= slack)
                    .map(|anchor| Hit::Anchor {
                        node: node.id.clone(),
                        anchor: anchor.id.clone(),
                    }),
                _ => None,
            });
            if anchor.is_some() {
                return anchor;
            }
        }
        self.scene.items.iter().rev().find_map(|item| match item {
            Item::Node(node) if !node.decorative && node.bounds().contains(at) => {
                Some(Hit::Node(node.id.clone()))
            }
            Item::Connector(connector)
                if !connector.decorative
                    && self
                        .scene
                        .line(connector)
                        .is_some_and(|line| line.distance_to(at) <= slack) =>
            {
                Some(Hit::Connector(connector.id.clone()))
            }
            _ => None,
        })
    }

    /// The items on screen: the scene less what was deleted and not yet answered, and the
    /// connectors of deleted nodes.
    pub fn shown(&self) -> impl DoubleEndedIterator<Item = &Item> {
        self.scene.items.iter().filter(|item| !self.is_hidden(item))
    }

    fn is_hidden(&self, item: &Item) -> bool {
        let hidden = |id: &str| self.hidden.iter().any(|gone| gone == id);
        match item {
            Item::Node(node) => hidden(&node.id),
            Item::Connector(connector) => {
                hidden(&connector.id) || hidden(&connector.from.node) || hidden(&connector.to.node)
            }
        }
    }

    pub fn selection(&self) -> &[String] {
        &self.selection
    }

    pub fn is_selected(&self, id: &str) -> bool {
        self.selection.iter().any(|selected| selected == id)
    }

    /// Selects `id` alone, or adds it to the selection or takes it out when `toggle`. Whether the
    /// selection changed.
    pub fn select(&mut self, id: &str, toggle: bool) -> bool {
        let before = self.selection.clone();
        if !toggle {
            self.selection.clear();
            self.selection.push(id.to_owned());
        } else if let Some(at) = self.selection.iter().position(|selected| selected == id) {
            self.selection.remove(at);
        } else {
            self.selection.push(id.to_owned());
        }
        self.selection != before
    }

    pub fn clear_selection(&mut self) -> bool {
        let changed = !self.selection.is_empty();
        self.selection.clear();
        changed
    }

    /// Selects the nodes the box touches, adding them to the selection when `adding`.
    pub fn select_within(&mut self, area: Rect, adding: bool) -> bool {
        let before = self.selection.clone();
        if !adding {
            self.selection.clear();
        }
        let touched: Vec<String> = self
            .shown()
            .filter_map(|item| match item {
                Item::Node(node) if !node.decorative && node.bounds().intersects(area) => {
                    Some(node.id.clone())
                }
                _ => None,
            })
            .collect();
        for id in touched {
            if !self.selection.contains(&id) {
                self.selection.push(id);
            }
        }
        self.selection != before
    }

    /// The selected nodes a drag moves: not locked and not decorative, with where each starts.
    pub fn draggable(&self) -> Vec<Placed> {
        self.scene
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Node(node) if !node.locked && !node.decorative && self.is_selected(&node.id) => {
                    Some(Placed {
                        id: node.id.clone(),
                        at: Point::new(node.x, node.y),
                    })
                }
                _ => None,
            })
            .collect()
    }

    /// Where nodes that started at `from` land when dragged by `by` canvas units, their top-left
    /// on the grid when it snaps.
    pub fn dragged(&self, from: &[Placed], by: Point) -> Vec<Placed> {
        let snap = self.scene.grid.filter(|grid| grid.snap && grid.size > 0.);
        from.iter()
            .map(|start| {
                let at = start.at + by;
                Placed {
                    id: start.id.clone(),
                    at: snap.map_or(at, |grid| {
                        Point::new(
                            (at.x / grid.size).round() * grid.size,
                            (at.y / grid.size).round() * grid.size,
                        )
                    }),
                }
            })
            .collect()
    }

    /// Puts nodes where they were dropped; they stay there unless the plug-in places them.
    pub fn place(&mut self, placed: &[Placed]) {
        for item in &mut self.scene.items {
            if let Item::Node(node) = item
                && let Some(spot) = placed.iter().find(|spot| spot.id == node.id)
            {
                node.x = spot.at.x;
                node.y = spot.at.y;
            }
        }
    }

    /// Whether a connector can be drawn out of `node`'s `anchor`.
    pub fn starts_connectors(&self, node: &str, anchor: &str) -> bool {
        self.scene
            .node(node)
            .and_then(|from| from.anchor(anchor))
            .is_some_and(|anchor| anchor.direction.starts())
    }

    /// The end a connector from `node`'s `anchor` would have if dropped on `target`: an anchor
    /// that takes connectors in, or the box of another node. `None` when it can't connect there.
    pub fn connection(&self, (node, anchor): (&str, &str), target: Option<&Hit>) -> Option<End> {
        if !self.starts_connectors(node, anchor) {
            return None;
        }
        match target? {
            Hit::Anchor { node: to, anchor } if to != node => {
                let ends = self.scene.node(to)?.anchor(anchor)?.direction.ends();
                ends.then(|| End {
                    node: to.clone(),
                    anchor: Some(anchor.clone()),
                })
            }
            Hit::Node(to) if to != node => Some(End {
                node: to.clone(),
                anchor: None,
            }),
            _ => None,
        }
    }

    /// Deletes what is selected: it is hidden until the plug-in answers, and comes back unless the
    /// answer removes it. The deleted items, or none.
    pub fn delete_selection(&mut self) -> Vec<String> {
        let deleted = std::mem::take(&mut self.selection);
        self.hidden.extend(deleted.iter().cloned());
        deleted
    }

    /// Draws a connector the plug-in hasn't answered for yet.
    pub fn propose(&mut self, from: End, to: End) {
        self.provisional.push((from, to));
    }

    pub fn provisional(&self) -> &[(End, End)] {
        &self.provisional
    }

    /// The plug-in answered `event`: its connector drawn so far goes, the plug-in's own upsert
    /// adding the real one, and what it deleted shows again unless the answer removed it.
    pub fn settle(&mut self, event: &Event) {
        match event {
            Event::Connected { from, to } => self
                .provisional
                .retain(|(drawn_from, drawn_to)| drawn_from != from || drawn_to != to),
            Event::Deleted(items) => self.hidden.retain(|id| !items.contains(id)),
            Event::Selected(_) | Event::Moved(_) | Event::Activated(_) | Event::Menu { .. } => {}
        }
    }

    pub fn apply(&mut self, ops: Vec<Op>) {
        for op in ops {
            match op {
                Op::Upsert(item) => match self.scene.items.iter_mut().find(|known| known.id() == item.id()) {
                    Some(known) => *known = item,
                    None => self.scene.items.push(item),
                },
                Op::Remove(ids) => {
                    self.scene
                        .items
                        .retain(|item| !ids.iter().any(|id| id == item.id()));
                    self.selection.retain(|id| !ids.contains(id));
                }
            }
        }
    }
}

/// The view that fits the content into a view of `size` screen pixels, never zooming in past 1.
fn fitted(scene: &Scene, size: Point) -> Viewport {
    let Some(extent) = scene.extent() else {
        return Viewport {
            x: 0.,
            y: 0.,
            zoom: 1.,
        };
    };
    let room = Point::new(
        (size.x - 2. * FIT_MARGIN).max(1.),
        (size.y - 2. * FIT_MARGIN).max(1.),
    );
    let zoom = (room.x / extent.width.max(1.))
        .min(room.y / extent.height.max(1.))
        .clamp(MIN_ZOOM, 1.);
    let center = extent.center();
    Viewport {
        x: center.x - size.x / zoom / 2.,
        y: center.y - size.y / zoom / 2.,
        zoom,
    }
}

/// The events waiting for a canvas's plug-in, sent one at a time and in order. A drag's moves and
/// a run of selections that haven't gone yet merge into one.
#[derive(Debug, Default)]
pub struct Outbox {
    waiting: VecDeque<Event>,
    sending: bool,
}

impl Outbox {
    pub fn push(&mut self, event: Event) {
        match (self.waiting.back_mut(), event) {
            (Some(Event::Moved(queued)), Event::Moved(moved)) => {
                for spot in moved {
                    match queued.iter_mut().find(|known| known.id == spot.id) {
                        Some(known) => known.at = spot.at,
                        None => queued.push(spot),
                    }
                }
            }
            (Some(Event::Selected(queued)), Event::Selected(selected)) => *queued = selected,
            (_, event) => self.waiting.push_back(event),
        }
    }

    /// The next event to send, unless one is still waiting for its answer.
    pub fn take(&mut self) -> Option<Event> {
        if self.sending {
            return None;
        }
        let next = self.waiting.pop_front();
        self.sending = next.is_some();
        next
    }

    /// The answer to the event sent last arrived, or it failed.
    pub fn answered(&mut self) {
        self.sending = false;
    }
}

#[cfg(test)]
mod tests {
    use super::{Board, Hit, Outbox};
    use crate::plugin::canvas::{
        Anchor, Arrow, Connector, Direction, End, Event, Grid, Item, Node, Op, Placed, Point, Route, Scene,
        Side,
    };

    fn node(id: &str, x: f32, anchors: &[(&str, Direction)]) -> Item {
        Item::Node(Node {
            id: id.to_owned(),
            x,
            y: 0.,
            width: 200.,
            height: 72.,
            shapes: Vec::new(),
            anchors: anchors
                .iter()
                .map(|(id, direction)| Anchor {
                    id: (*id).to_owned(),
                    x: 0.,
                    y: 36.,
                    side: Side::Left,
                    direction: *direction,
                })
                .collect(),
            tooltip: None,
            menu: Vec::new(),
            locked: id == "locked",
            decorative: false,
        })
    }

    fn board() -> Board {
        Board::new(Scene {
            items: vec![
                node("fetch", 0., &[("out", Direction::Out)]),
                node("draft", 272., &[("in", Direction::In), ("out", Direction::Out)]),
                node("locked", 544., &[("in", Direction::In)]),
            ],
            grid: Some(Grid {
                size: 16.,
                snap: true,
            }),
            viewport: None,
        })
    }

    fn placed(id: &str, x: f32, y: f32) -> Placed {
        Placed {
            id: id.to_owned(),
            at: Point::new(x, y),
        }
    }

    #[test]
    fn a_drag_snaps_to_the_grid_and_leaves_locked_nodes_where_they_are() {
        let mut board = board();
        board.select("draft", false);
        board.select("locked", true);
        let start = board.draggable();
        assert_eq!(start, [placed("draft", 272., 0.)]);
        assert_eq!(
            board.dragged(&start, Point::new(9., 7.)),
            [placed("draft", 288., 0.)]
        );
    }

    #[test]
    fn connectors_go_from_an_out_anchor_to_an_in_anchor_or_the_box_of_another_node() {
        let board = board();
        let into = |node: &str, anchor: &str| Hit::Anchor {
            node: node.to_owned(),
            anchor: anchor.to_owned(),
        };
        assert_eq!(
            board.connection(("fetch", "out"), Some(&into("draft", "in"))),
            Some(End {
                node: "draft".into(),
                anchor: Some("in".into())
            })
        );
        assert_eq!(
            board.connection(("fetch", "out"), Some(&into("draft", "out"))),
            None
        );
        assert_eq!(
            board.connection(("draft", "in"), Some(&Hit::Node("fetch".into()))),
            None
        );
        assert_eq!(
            board.connection(("draft", "out"), Some(&Hit::Node("draft".into()))),
            None
        );
        assert_eq!(
            board.connection(("draft", "out"), Some(&Hit::Node("fetch".into()))),
            Some(End {
                node: "fetch".into(),
                anchor: None
            })
        );
    }

    #[test]
    fn upserts_replace_in_place_and_removals_leave_the_selection() {
        let mut board = board();
        board.select("draft", false);
        board.apply(vec![
            Op::Upsert(node("fetch", 64., &[])),
            Op::Upsert(node("step-4", 0., &[])),
            Op::Remove(vec!["draft".into()]),
        ]);
        let ids: Vec<&str> = board.scene().items.iter().map(Item::id).collect();
        assert_eq!(ids, ["fetch", "locked", "step-4"]);
        assert_eq!(board.scene().node("fetch").map(|node| node.x), Some(64.));
        assert_eq!(board.selection(), [] as [String; 0]);
    }

    #[test]
    fn deleted_items_hide_with_their_connectors_until_the_answer() {
        let mut board = board();
        board.apply(vec![Op::Upsert(Item::Connector(Connector {
            id: "e1".into(),
            from: End {
                node: "fetch".into(),
                anchor: Some("out".into()),
            },
            to: End {
                node: "draft".into(),
                anchor: Some("in".into()),
            },
            route: Route::Curved,
            stroke: None,
            stroke_width: None,
            dashed: false,
            arrow: Arrow::End,
            label: None,
            tooltip: None,
            menu: Vec::new(),
            decorative: false,
        }))]);
        board.select("draft", false);
        let deleted = board.delete_selection();
        assert_eq!(deleted, ["draft"]);
        let shown: Vec<&str> = board.shown().map(Item::id).collect();
        assert_eq!(shown, ["fetch", "locked"]);
        board.settle(&Event::Deleted(deleted));
        assert_eq!(board.shown().count(), 4);
    }

    #[test]
    fn waiting_moves_and_selections_merge_but_go_one_at_a_time() {
        let mut outbox = Outbox::default();
        outbox.push(Event::Selected(vec!["fetch".into()]));
        outbox.push(Event::Selected(vec!["draft".into()]));
        assert_eq!(outbox.take(), Some(Event::Selected(vec!["draft".into()])));
        outbox.push(Event::Moved(vec![placed("draft", 16., 0.)]));
        outbox.push(Event::Moved(vec![
            placed("draft", 32., 0.),
            placed("fetch", 0., 16.),
        ]));
        assert_eq!(outbox.take(), None);
        outbox.answered();
        assert_eq!(
            outbox.take(),
            Some(Event::Moved(vec![
                placed("draft", 32., 0.),
                placed("fetch", 0., 16.)
            ]))
        );
        outbox.answered();
        assert_eq!(outbox.take(), None);
    }
}
