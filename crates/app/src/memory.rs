use gateway::GatewayError;
use std::collections::{HashMap, HashSet};

use model::flagged::{FlaggedAction, FlaggedGroup};
use model::memory::{Found, Period, SessionMemory, Stats, Summary, TagProposal, TermProposal, Verdict};
use model::tidy::{Batch, Upkeep};
use model::topics::Topic;
use model::wiki::{EntityMention, Listing, Owned, Page, PageEdit};
use model::{SessionId, TopicId};

use crate::Hub;

/// How many wiki pages are asked about at once.
const WIKI_CHECKS: usize = 8;

/// The topics, and as far as they have been read, the wiki pages their root topics own and the
/// entities each root topic's sessions mention. A root's page is read once it's wanted.
#[derive(Debug, Default)]
pub struct Library {
    pub topics: Vec<Topic>,
    pub pages: Vec<Page>,
    pub mentions: HashMap<TopicId, Vec<EntityMention>>,
    /// The root topics whose page and mentions have been read, whether rh had a page or not.
    pub read: HashSet<TopicId>,
}

impl Library {
    /// Keeps what was read for a root topic, saying whether its page changed.
    pub fn keep(&mut self, root: TopicId, owned: Owned) -> bool {
        let before = self
            .pages
            .iter()
            .position(|page| page.topic.as_ref() == Some(&root))
            .map(|at| self.pages.remove(at));
        let changed = before != owned.page;
        self.pages.extend(owned.page);
        self.mentions.insert(root.clone(), owned.mentions);
        self.read.insert(root);
        changed
    }
}

/// What memory proposed and is waiting on a person for: tree changes in batches, then the
/// tagger's tags and tag values.
#[derive(Debug, Default)]
pub struct Tidy {
    pub batches: Vec<Batch>,
    pub tags: Vec<TagProposal>,
    pub terms: Vec<TermProposal>,
}

impl Tidy {
    /// How many proposals wait, as the Stats tab counts them.
    pub fn waiting(&self) -> usize {
        self.batches
            .iter()
            .map(|batch| batch.proposals.len())
            .sum::<usize>()
            + self.tags.len()
            + self.terms.len()
    }
}

impl Hub {
    pub async fn recall(&self, query: String, period: Period) -> Result<Vec<Found>, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.recall(&query, period).await })
            .await
    }

    /// One of a session's summaries, whole, for one its outline cut short.
    pub async fn session_summary(
        &self,
        session: SessionId,
        summary: String,
    ) -> Result<Summary, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.session_summary(&session, &summary).await })
            .await
    }

    pub async fn session_memory(&self, session: SessionId) -> Result<SessionMemory, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.session_memory(&session).await })
            .await
    }

    /// Several sessions' memory, read together and in order; a session whose memory couldn't be
    /// read comes back as `None`.
    pub async fn session_memories(
        &self,
        sessions: Vec<SessionId>,
    ) -> Result<Vec<Option<SessionMemory>>, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move {
            let read =
                futures::future::join_all(sessions.iter().map(|session| gateway.session_memory(session)))
                    .await;
            Ok(read.into_iter().map(Result::ok).collect())
        })
        .await
    }

    /// The topics alone; each root topic's page is read when it's wanted, with `topic_page`.
    pub async fn library(&self) -> Result<Library, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move {
            Ok(Library {
                topics: gateway.topics().await?,
                ..Library::default()
            })
        })
        .await
    }

    /// Edits the page a topic or entity owns, writing it first when the wiki hasn't.
    pub async fn edit_page(&self, topic: TopicId, edit: PageEdit) -> Result<Owned, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.edit_page(&topic, edit).await })
            .await
    }

    /// A filed conversation's summaries, for one that is no harness session.
    pub async fn conversation_memory(&self, conversation: String) -> Result<SessionMemory, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.conversation_memory(&conversation).await })
            .await
    }

    pub async fn upkeep(&self) -> Result<Upkeep, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.upkeep().await }).await
    }

    /// The jobs memory stopped retrying, grouped by cause; `None` when this node keeps none for
    /// this client.
    pub async fn flagged(&self) -> Result<Option<Vec<FlaggedGroup>>, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.flagged().await }).await
    }

    pub async fn act_on_flagged(&self, id: i64, action: FlaggedAction) -> Result<(), GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.act_on_flagged(id, action).await })
            .await
    }

    pub async fn decide_proposal(&self, proposal: String, verdict: Verdict) -> Result<(), GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.decide_proposal(&proposal, verdict).await })
            .await
    }

    /// Decides a whole batch; answers each proposal it couldn't decide with rh's words.
    pub async fn decide_batch(
        &self,
        batch: String,
        verdict: Verdict,
    ) -> Result<Vec<(String, String)>, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.decide_batch(&batch, verdict).await })
            .await
    }

    pub async fn memory_stats(&self) -> Result<Stats, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.memory_stats().await }).await
    }

    /// The page a topic or entity owns, if the wiki has written one, and what its sessions mention.
    pub async fn topic_page(&self, topic: TopicId) -> Result<Owned, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.topic_page(&topic).await })
            .await
    }

    pub async fn wiki_pages(&self, query: String) -> Result<Vec<Listing>, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.wiki_pages(&query).await })
            .await
    }

    /// Which of these slugs name a wiki page, a few at a time; a slug rh couldn't be asked about
    /// is left out, to be asked again.
    pub async fn wiki_pages_exist(&self, slugs: Vec<String>) -> Vec<(String, bool)> {
        use futures::StreamExt as _;
        let gateway = self.inner.local.clone();
        let checked = self.on_runtime(async move {
            Ok(futures::stream::iter(slugs)
                .map(|slug| {
                    let gateway = gateway.clone();
                    async move {
                        let found = gateway.wiki_page_exists(&slug).await;
                        found.ok().map(|found| (slug, found))
                    }
                })
                .buffer_unordered(WIKI_CHECKS)
                .filter_map(std::future::ready)
                .collect()
                .await)
        });
        checked.await.unwrap_or_default()
    }

    pub async fn wiki_page(&self, slug: String) -> Result<Page, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.wiki_page(&slug).await })
            .await
    }

    pub async fn tidy(&self) -> Result<Tidy, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move {
            let (batches, tags, terms) = futures::future::try_join3(
                gateway.proposals(),
                gateway.tag_proposals(),
                gateway.term_proposals(),
            )
            .await?;
            Ok(Tidy { batches, tags, terms })
        })
        .await
    }

    pub async fn decide_tag(&self, id: String, verdict: Verdict) -> Result<(), GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.decide_tag(&id, verdict).await })
            .await
    }

    pub async fn decide_term(&self, term: TermProposal, verdict: Verdict) -> Result<(), GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.decide_term(&term.key, &term.value, verdict).await })
            .await
    }
}
