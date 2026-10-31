use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use gpui::{Context, Window};
use model::TopicId;
use model::markdown::page_links;

use super::{MemoryPage, Tab};
use crate::markdown_view::PageLinks;

/// Which pages the open pages' `[[links]]` name, as rh answered, and the ones it's being asked
/// about.
#[derive(Debug, Default)]
pub(super) struct Links {
    found: HashMap<String, bool>,
    asking: HashSet<String>,
}

impl MemoryPage {
    /// Asks rh which of the open pages' links name a page it has, for those not asked about yet.
    pub(super) fn check_links(&mut self, cx: &mut Context<Self>) {
        let open = [
            self.reading.as_ref().map(|reading| &reading.blocks),
            self.browse.reading.as_ref().map(|reading| &reading.blocks),
        ];
        let mut slugs: Vec<String> = Vec::new();
        for slug in open.into_iter().flatten().flat_map(|blocks| page_links(blocks)) {
            let asked = self.links.found.contains_key(&slug) || self.links.asking.contains(&slug);
            if !asked && !slugs.contains(&slug) {
                slugs.push(slug);
            }
        }
        if slugs.is_empty() {
            return;
        }
        self.links.asking.extend(slugs.iter().cloned());
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let found = hub.wiki_pages_exist(slugs.clone()).await;
            this.update(cx, |this, cx| {
                for slug in &slugs {
                    this.links.asking.remove(slug);
                }
                this.links.found.extend(found);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The links a page shows: to the pages rh has, each opening here.
    pub(super) fn page_links(&self, cx: &mut Context<Self>) -> PageLinks {
        let known = self
            .links
            .found
            .iter()
            .filter(|(_, found)| **found)
            .map(|(slug, _)| slug.clone())
            .collect();
        let page = cx.entity().downgrade();
        PageLinks {
            known: Arc::new(known),
            open: Rc::new(move |slug: &str, _: &mut Window, cx| {
                let slug = slug.to_owned();
                page.update(cx, |this, cx| this.follow_link(slug, cx)).ok();
            }),
        }
    }

    /// Opens the page a link names where Memory shows it: on its topic's or entity's page, else on
    /// its own.
    fn follow_link(&mut self, slug: String, cx: &mut Context<Self>) {
        self.problem = None;
        let hub = self.hub.clone();
        let catalog = self.catalog.is_none().then(|| hub.clone());
        cx.spawn(async move |this, cx| {
            let fetched = hub.wiki_page(slug.clone()).await;
            let catalog = match catalog {
                Some(hub) => hub.catalog().await.ok(),
                None => None,
            };
            this.update(cx, |this, cx| {
                if let Some(catalog) = catalog {
                    this.catalog = Some(catalog);
                }
                match fetched {
                    Ok(page) => this.open_linked(slug, page.topic, cx),
                    Err(err) => this.failed(&err, cx),
                }
            })
            .ok();
        })
        .detach();
    }

    fn open_linked(&mut self, slug: String, owner: Option<TopicId>, cx: &mut Context<Self>) {
        let entity = owner.as_ref().is_some_and(|owner| {
            self.catalog
                .as_ref()
                .is_some_and(|catalog| catalog.entity(owner).is_some())
        });
        match owner {
            Some(owner) if entity => {
                self.select(Tab::Entities, cx);
                self.open_entity(owner, cx);
            }
            Some(owner) => {
                self.select(Tab::Topics, cx);
                self.open_topic(owner, cx);
            }
            None => {
                self.select(Tab::Topics, cx);
                self.open_page(slug, cx);
            }
        }
    }
}
