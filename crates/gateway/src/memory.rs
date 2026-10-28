use model::TopicId;
use model::memory::{Found, Period, TagProposal, TermProposal, Verdict};
use model::wiki::{Listing, Page};
use wire::gateway::memory::{
    MemorySearchResponse, MemoryTagsDecideRequest, PendingTagListResult, TagDecisionResult, TagEntryPair,
    TagTaxonomyDecideRequest, TagTaxonomyListResult,
};
use wire::gateway::topics::TopicPage;
use wire::gateway::wiki::{WikiPageResponse, WikiTopicList};

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
                .append_pair("limit", SEARCH_LIMIT);
            if let Some(window) = translate::browse_window(period) {
                params.append_pair("window", &window.to_string());
            }
        }
        let found: MemorySearchResponse = Self::read_json(self.http.get(url).send().await)?.await?;
        Ok(model::memory::said(translate::found(found)))
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
