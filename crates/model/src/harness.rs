use crate::HarnessId;

/// A harness a node offers, as `GET /api/harnesses` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessOffer {
    pub id: HarnessId,
    pub chat: bool,
    pub lists_sessions: bool,
    pub takes_images: bool,
    pub models: Vec<ModelChoice>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelChoice {
    pub id: String,
    pub label: String,
    pub default: bool,
}

impl HarnessOffer {
    /// Up to `limit` of its models matching every word of `query`, those with a word starting
    /// with the first one first, and how many more match.
    pub fn suggest_models(&self, query: &str, limit: usize) -> (Vec<&ModelChoice>, usize) {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let mut ranked: Vec<(bool, &ModelChoice)> = self
            .models
            .iter()
            .filter_map(|model| {
                let text = format!("{} {}", model.label, model.id).to_lowercase();
                if !words.iter().all(|word| text.contains(word.as_str())) {
                    return None;
                }
                let leads = words.first().is_some_and(|first| {
                    text.split(['/', ' ', '-'])
                        .any(|part| part.starts_with(first.as_str()))
                });
                Some((!leads, model))
            })
            .collect();
        ranked.sort_by_key(|(later, _)| *later);
        let more = ranked.len().saturating_sub(limit);
        let shown = ranked.into_iter().take(limit).map(|(_, model)| model).collect();
        (shown, more)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_suggestions_match_every_word_and_lead_with_word_starts() {
        let models = [
            "google/Gemini Superflash",
            "nvidia/DeepSeek V4.1 Flash",
            "gpubox-ds41/DeepSeek-V4.1-Flash-1017",
            "opencode/MiniMax Free",
        ];
        let offer = HarnessOffer {
            id: HarnessId::new("opencode"),
            chat: true,
            lists_sessions: true,
            takes_images: false,
            models: models
                .map(|label| ModelChoice {
                    id: label.to_owned(),
                    label: label.to_owned(),
                    default: false,
                })
                .to_vec(),
        };
        let labels = |query, limit| {
            let (shown, more) = offer.suggest_models(query, limit);
            (
                shown.iter().map(|model| model.label.as_str()).collect::<Vec<_>>(),
                more,
            )
        };
        assert_eq!(
            labels("flash", 2),
            (
                vec![
                    "nvidia/DeepSeek V4.1 Flash",
                    "gpubox-ds41/DeepSeek-V4.1-Flash-1017"
                ],
                1
            )
        );
        assert_eq!(
            labels("ds41 FLASH", 5),
            (vec!["gpubox-ds41/DeepSeek-V4.1-Flash-1017"], 0)
        );
    }
}
