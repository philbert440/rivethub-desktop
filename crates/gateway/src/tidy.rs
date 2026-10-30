use model::TopicId;
use model::memory::Verdict;
use model::tidy::{Batch, Change, Filing, Proposal, Proposer, TypeCount, Upkeep, Wiki, Worker, Workers};
use wire::gateway::tidy::{
    MemoryUpkeep, TidyBatch, TidyBatchDecision, TidyChange, TidyPending, TidyProposal, WorkerActivity,
};

use crate::{Gateway, GatewayError, translate};

fn change(raw: TidyChange) -> Change {
    let id = TopicId::new;
    match raw {
        TidyChange::Merge { source, into } => Change::Merge {
            source: id(source),
            into: id(into),
        },
        TidyChange::Move { topic, parent } => Change::Move {
            topic: id(topic),
            parent: parent.map(id),
        },
        TidyChange::Rename { topic, name } => Change::Rename {
            topic: id(topic),
            name,
        },
        TidyChange::Close { topic } => Change::Close { topic: id(topic) },
        TidyChange::Delete { topic } => Change::Delete { topic: id(topic) },
        TidyChange::CreateTopic {
            topic,
            name,
            parent,
            sessions,
            conversations,
        } => Change::CreateTopic {
            topic: id(topic),
            name,
            parent: parent.map(id),
            sessions: sessions.iter().map(translate::session).collect(),
            conversations,
        },
        TidyChange::AddType { topic, name, parent } => Change::AddType {
            topic: id(topic),
            name,
            parent: parent.map(id),
        },
    }
}

fn proposal(raw: TidyProposal) -> Proposal {
    Proposal {
        id: raw.id,
        change: change(raw.change),
        reason: raw.reason,
        sessions: raw.evidence.sessions,
        samples: raw
            .evidence
            .samples
            .into_iter()
            .filter_map(|sample| sample.title)
            .filter(|title| !title.trim().is_empty())
            .collect(),
    }
}

fn batch(raw: TidyBatch) -> Batch {
    Batch {
        id: raw.batch_id,
        by: match raw.proposed_by {
            wire::gateway::tidy::Proposer::Categorizer => Proposer::Categorizer,
            wire::gateway::tidy::Proposer::Backfill => Proposer::Backfill,
        },
        at_ms: raw.proposed_at.timestamp_millis(),
        proposals: raw.proposals.into_iter().map(proposal).collect(),
    }
}

fn worker(raw: &WorkerActivity) -> Worker {
    Worker {
        last_finished_ms: raw.last_finished_at.map(|at| at.timestamp_millis()),
        failing: raw.failing,
    }
}

fn decision(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Accept => "approve",
        Verdict::Reject => "reject",
    }
}

impl Gateway {
    /// How memory is keeping up: filing, the wiki, the workers and the proposals waiting.
    pub async fn upkeep(&self) -> Result<Upkeep, GatewayError> {
        let url = self.api_url(&["api", "memory", "upkeep"])?;
        let raw: MemoryUpkeep = Self::read_json(self.http.get(url).send().await)?.await?;
        Ok(Upkeep {
            filing: Filing {
                categorizer: raw.filing.categorizer,
                you: raw.filing.you,
                uncategorized: raw.filing.uncategorized,
                left_out: raw.filing.left_out,
                corrections_last_week: raw.filing.corrections_last_week,
            },
            wiki: Wiki {
                topic_pages: raw.wiki.topic_pages,
                entity_types: raw
                    .wiki
                    .entity_types
                    .into_iter()
                    .map(|counted| TypeCount {
                        kind: TopicId::new(counted.type_id),
                        name: counted.name,
                        entities: counted.entities,
                    })
                    .collect(),
                loose_pages: raw.wiki.loose_pages,
            },
            workers: Workers {
                embedder: worker(&raw.workers.embedder),
                summarizer: worker(&raw.workers.summarizer),
                categorizer: worker(&raw.workers.categorizer),
                wiki_builder: worker(&raw.workers.wiki_builder),
            },
            pending: raw.pending_proposals,
        })
    }

    /// The pending tree changes, newest batch first.
    pub async fn proposals(&self) -> Result<Vec<Batch>, GatewayError> {
        let url = self.api_url(&["api", "tidy", "proposals"])?;
        let pending: TidyPending = Self::read_json(self.http.get(url).send().await)?.await?;
        Ok(pending.batches.into_iter().map(batch).collect())
    }

    /// Approves or rejects one proposal; rh applies an approved change through the topic routes.
    pub async fn decide_proposal(&self, proposal: &str, verdict: Verdict) -> Result<(), GatewayError> {
        let url = self.api_url(&["api", "tidy", "proposals", proposal, decision(verdict)])?;
        let _: TidyProposal = Self::read_json(self.http.post(url).send().await)?.await?;
        Ok(())
    }

    /// Approves or rejects every pending proposal of a batch, and answers what failed and why.
    pub async fn decide_batch(&self, batch: &str, verdict: Verdict) -> Result<Vec<String>, GatewayError> {
        let url = self.api_url(&["api", "tidy", "batches", batch, decision(verdict)])?;
        let decided: TidyBatchDecision = Self::read_json(self.http.post(url).send().await)?.await?;
        Ok(decided.failed.into_iter().map(|failed| failed.error).collect())
    }
}
