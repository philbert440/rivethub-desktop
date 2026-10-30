use gateway::GatewayError;
use std::collections::HashMap;

use model::memory::{Found, Period, SessionMemory, Stats, TagProposal, TermProposal, Verdict};
use model::topics::Topic;
use model::wiki::{EntityMention, Listing, Owned, Page, PageEdit};
use model::{SessionId, TopicId};

use crate::Hub;

/// The topics, the wiki pages their root topics own, and the entities each root topic's sessions
/// mention.
#[derive(Debug, Default)]
pub struct Library {
    pub topics: Vec<Topic>,
    pub pages: Vec<Page>,
    pub mentions: HashMap<TopicId, Vec<EntityMention>>,
}

/// What the tagger proposed and is waiting on a person for.
#[derive(Debug, Default)]
pub struct Tidy {
    pub tags: Vec<TagProposal>,
    pub terms: Vec<TermProposal>,
}

impl Hub {
    pub async fn recall(&self, query: String, period: Period) -> Result<Vec<Found>, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.recall(&query, period).await })
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

    pub async fn library(&self) -> Result<Library, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move {
            let topics = gateway.topics().await?;
            let roots: Vec<&Topic> = topics.iter().filter(|topic| topic.parent.is_none()).collect();
            let owned =
                futures::future::try_join_all(roots.iter().map(|topic| gateway.topic_page(&topic.id)))
                    .await?;
            let mut pages = Vec::new();
            let mut mentions = HashMap::new();
            for (
                root,
                Owned {
                    page,
                    mentions: named,
                },
            ) in roots.iter().zip(owned)
            {
                pages.extend(page);
                mentions.insert(root.id.clone(), named);
            }
            Ok(Library {
                topics,
                pages,
                mentions,
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

    pub async fn wiki_page(&self, slug: String) -> Result<Page, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.wiki_page(&slug).await })
            .await
    }

    pub async fn tidy(&self) -> Result<Tidy, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move {
            let (tags, terms) =
                futures::future::try_join(gateway.tag_proposals(), gateway.term_proposals()).await?;
            Ok(Tidy { tags, terms })
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
