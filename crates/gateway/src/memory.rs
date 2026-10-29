use model::memory::{Found, Period, SessionMemory, Stats, TagProposal, TermProposal, Verdict};
use model::wiki::{Listing, Page, PageEdit};
use model::{SessionId, TopicId};
use wire::gateway::memory::{
    MemorySearchResponse, MemoryStatsResponse, MemorySummaryDagResponse, MemoryTagsDecideRequest,
    PendingTagListResult, TagDecisionResult, TagEntryPair, TagTaxonomyDecideRequest, TagTaxonomyListResult,
};
use wire::gateway::topics::TopicPage;
use wire::gateway::wiki::{WikiPageEdit, WikiPageResponse, WikiTopicList};

use crate::{Gateway, GatewayError, translate};

const SEARCH_LIMIT: &str = "50";
const LISTING_LIMIT: &str = "100";

impl Gateway {
    pub async fn recall(&self, query: &str, period: Period) -> Result<Vec<Found>, GatewayError> {
        let mut url = self.api_url(&["api", "memory", "search"])?;
        {
            let mut params = url.query_pairs_mut();
            params
                .append_pair("q", query)
                .append_pair("scope", "both")
                .append_pair("limit", SEARCH_LIMIT)
                .append_pair("exclude_roles", crate::SAID_ONLY);
            if let Some(window) = translate::browse_window(period) {
                params.append_pair("window", &window.to_string());
            }
        }
        let found: MemorySearchResponse = Self::read_json(self.http.get(url).send().await)?.await?;
        Ok(model::memory::said(translate::found(found)))
    }

    pub async fn session_memory(&self, session: &SessionId) -> Result<SessionMemory, GatewayError> {
        let id = translate::wire_session(session)?.to_string();
        let url = self.api_url(&["api", "memory", "summaries", &id])?;
        let dag: MemorySummaryDagResponse = Self::read_json(self.http.get(url).send().await)?.await?;
        Ok(translate::session_memory(dag))
    }

    pub async fn topic_page(&self, topic: &TopicId) -> Result<Option<Page>, GatewayError> {
        let url = self.api_url(&["api", "topics", topic.as_str(), "page"])?;
        let bound: TopicPage = Self::read_json(self.http.get(url).send().await)?.await?;
        Ok(bound.page.map(translate::page))
    }

    pub async fn wiki_page(&self, slug: &str) -> Result<Page, GatewayError> {
        let url = self.api_url(&["api", "wiki", slug])?;
        let page: WikiPageResponse = Self::read_json(self.http.get(url).send().await)?.await?;
        Ok(translate::page(page))
    }

    /// Replaces what the edit names on a topic or entity page, and answers the page as it is now.
    pub async fn edit_page(&self, slug: &str, edit: PageEdit) -> Result<Page, GatewayError> {
        let url = self.api_url(&["api", "wiki", slug])?;
        let body = WikiPageEdit {
            current_state: edit.current_state,
            article: edit.article,
        };
        let page: WikiPageResponse = Self::read_json(self.http.put(url).json(&body).send().await)?.await?;
        Ok(translate::page(page))
    }

    pub async fn memory_stats(&self) -> Result<Stats, GatewayError> {
        let url = self.api_url(&["api", "memory", "stats"])?;
        let stats: MemoryStatsResponse = Self::read_json(self.http.get(url).send().await)?.await?;
        Ok(Stats {
            conversations: stats.conversations,
            messages: stats.messages,
            tool_calls: stats.tool_calls,
            summaries: stats.summaries,
            embedded_messages: stats.embedded_messages,
            embed_queue: stats.embed_queue_depth,
            failed_embeddings: stats.failed_embeddings,
        })
    }

    pub async fn wiki_pages(&self, query: &str) -> Result<Vec<Listing>, GatewayError> {
        let mut url = self.api_url(&["api", "wiki", "topics"])?;
        url.query_pairs_mut().append_pair("limit", LISTING_LIMIT);
        if !query.trim().is_empty() {
            url.query_pairs_mut().append_pair("q", query.trim());
        }
        let list: WikiTopicList = Self::read_json(self.http.get(url).send().await)?.await?;
        Ok(list.topics.into_iter().map(translate::listing).collect())
    }

    pub async fn tag_proposals(&self) -> Result<Vec<TagProposal>, GatewayError> {
        let url = self.api_url(&["api", "memory", "tags", "pending"])?;
        let pending: PendingTagListResult = Self::read_json(self.http.get(url).send().await)?.await?;
        Ok(pending.tags.into_iter().map(translate::tag_proposal).collect())
    }

    pub async fn term_proposals(&self) -> Result<Vec<TermProposal>, GatewayError> {
        let mut url = self.api_url(&["api", "memory", "tags", "taxonomy"])?;
        url.query_pairs_mut().append_pair("state", "suggested");
        let listed: TagTaxonomyListResult = Self::read_json(self.http.get(url).send().await)?.await?;
        Ok(listed.entries.into_iter().map(translate::term_proposal).collect())
    }

    pub async fn decide_tag(&self, id: &str, verdict: Verdict) -> Result<(), GatewayError> {
        let url = self.api_url(&["api", "memory", "tags", "decide"])?;
        let body = MemoryTagsDecideRequest {
            ids: Some(vec![id.to_owned()]),
            state: Some(translate::verdict(verdict)),
            decided_by: None,
        };
        let _: TagDecisionResult = Self::read_json(self.http.post(url).json(&body).send().await)?.await?;
        Ok(())
    }

    pub async fn decide_term(&self, key: &str, value: &str, verdict: Verdict) -> Result<(), GatewayError> {
        let url = self.api_url(&["api", "memory", "tags", "taxonomy", "decide"])?;
        let body = TagTaxonomyDecideRequest {
            entries: Some(vec![TagEntryPair {
                key: key.to_owned(),
                value: value.to_owned(),
            }]),
            state: Some(translate::verdict(verdict)),
            decided_by: None,
        };
        let _: TagDecisionResult = Self::read_json(self.http.post(url).json(&body).send().await)?.await?;
        Ok(())
    }
}
