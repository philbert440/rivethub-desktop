use gpui::Context;
use model::TopicId;
use model::topics::{Atlas, Placements, Topic, outline};
use model::wiki::Owned;

use super::{MemoryPage, Opened};

/// How many root topics' pages are read as soon as the topics list: those the list opens on.
const READ_FIRST: usize = 4;
/// The topic index's children before its first topic row: the finder and the heading.
const INDEX_HEAD: usize = 2;

impl MemoryPage {
    /// Lists the topics, then reads the pages of the root topics the list opens on; any other
    /// root's page is read once its topic is opened or scrolled into view. Pages read before stay
    /// on screen until they are read again.
    pub(super) fn load_library(&self, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let loaded = hub.library().await;
            this.update(cx, |this, cx| match loaded {
                Ok(mut library) => {
                    if let Some(before) = this.library.take() {
                        library.pages = before.pages;
                        library.mentions = before.mentions;
                    }
                    let first = first_roots(&library.topics);
                    this.library = Some(library);
                    this.read_pages(first, cx);
                    if let Some(Opened::Topic(topic)) = this.opened.clone() {
                        this.open_topic(topic, cx);
                    }
                    cx.notify();
                }
                Err(err) if err.is_missing_route() => {
                    this.problem = Some("Topic pages need a newer rh on this node.".into());
                    cx.notify();
                }
                Err(err) => this.failed(&err, cx),
            })
            .ok();
        })
        .detach();
    }

    /// Reads these root topics' pages, each but once at a time.
    pub(super) fn read_pages(&mut self, roots: Vec<TopicId>, cx: &mut Context<Self>) {
        for root in roots {
            if !self.reading_roots.insert(root.clone()) {
                continue;
            }
            let hub = self.hub.clone();
            cx.spawn(async move |this, cx| {
                let read = hub.topic_page(root.clone()).await;
                this.update(cx, |this, cx| {
                    this.reading_roots.remove(&root);
                    match read {
                        Ok(owned) => this.page_read(root, owned, cx),
                        Err(err) if err.is_missing_route() => {
                            this.problem = Some("Topic pages need a newer rh on this node.".into());
                            cx.notify();
                        }
                        Err(err) => this.failed(&err, cx),
                    }
                })
                .ok();
            })
            .detach();
        }
    }

    /// Keeps a root topic's page, and shows it when its topic is open and it changed.
    fn page_read(&mut self, root: TopicId, owned: Owned, cx: &mut Context<Self>) {
        let open = self.opened_root().as_ref() == Some(&root);
        let Some(library) = self.library.as_mut() else {
            return;
        };
        let changed = library.keep(root, owned);
        if open && (changed || self.reading.is_none()) {
            self.show_topic(cx);
        }
        cx.notify();
    }

    /// Whether the open topic's root page has been read, written or not.
    pub(super) fn root_read(&self) -> bool {
        self.opened_root()
            .zip(self.library.as_ref())
            .is_some_and(|(root, library)| library.read.contains(&root))
    }

    /// Reads the pages of the root topics whose rows the index shows, once this frame is drawn.
    pub(super) fn read_in_view(&self, rows: &[(usize, &Topic)], cx: &mut Context<Self>) {
        let Some(library) = self.library.as_ref() else {
            return;
        };
        let shown = self.index_scroll.top_item()..=self.index_scroll.bottom_item();
        let placements = Placements::new();
        let atlas = Atlas::new(&library.topics, &[], &placements);
        let mut wanted: Vec<TopicId> = Vec::new();
        for (_, topic) in rows
            .iter()
            .enumerate()
            .filter(|(row, _)| shown.contains(&(row + INDEX_HEAD)))
            .map(|(_, row)| *row)
        {
            let root = atlas.root(&topic.id);
            if !library.read.contains(root) && !self.reading_roots.contains(root) && !wanted.contains(root) {
                wanted.push(root.clone());
            }
        }
        if wanted.is_empty() {
            return;
        }
        let this = cx.weak_entity();
        cx.defer(move |cx| {
            this.update(cx, |this, cx| this.read_pages(wanted, cx)).ok();
        });
    }
}

/// The root topics the outline opens with.
fn first_roots(topics: &[Topic]) -> Vec<TopicId> {
    outline(topics)
        .into_iter()
        .filter(|(depth, _)| *depth == 0)
        .take(READ_FIRST)
        .map(|(_, topic)| topic.id.clone())
        .collect()
}
