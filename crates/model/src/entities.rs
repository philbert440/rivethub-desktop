//! What the wiki knows about: people, hosts, models, software and the rest, each an entity filed
//! under one type, the types nesting like topics do.

use crate::TopicId;
use crate::topics::{Atlas, Placements, Topic, outline};

/// An entity and the other names it goes by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entity {
    pub topic: Topic,
    pub aliases: Vec<String>,
}

/// The entity types and the entities filed under them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalog {
    pub types: Vec<Topic>,
    pub entities: Vec<Entity>,
}

impl Catalog {
    /// The types as a tree, each with its depth.
    pub fn outline(&self) -> Vec<(usize, &Topic)> {
        outline(&self.types)
    }

    pub fn entity(&self, id: &TopicId) -> Option<&Entity> {
        self.entities.iter().find(|entity| &entity.topic.id == id)
    }

    pub fn kind(&self, id: &TopicId) -> Option<&Topic> {
        self.types.iter().find(|kind| &kind.id == id)
    }

    /// The entities under the type or any of its subtypes, by name.
    pub fn within(&self, kind: &TopicId) -> Vec<&Entity> {
        let mut within: Vec<&Entity> = self
            .entities
            .iter()
            .filter(|entity| {
                entity
                    .topic
                    .parent
                    .as_ref()
                    .is_some_and(|parent| self.lineage(parent).contains(&kind))
            })
            .collect();
        within.sort_by_key(|entity| entity.topic.name.to_lowercase());
        within
    }

    /// The type and the types above it, the top-level type last.
    pub fn lineage<'a>(&'a self, kind: &'a TopicId) -> Vec<&'a TopicId> {
        self.atlas().lineage(kind)
    }

    /// The type path an entity or type sits on, from the top-level type down.
    pub fn path(&self, id: &TopicId) -> Vec<&Topic> {
        let start = self
            .entity(id)
            .and_then(|entity| entity.topic.parent.as_ref())
            .unwrap_or(id);
        let mut path: Vec<&Topic> = self
            .lineage(start)
            .into_iter()
            .filter_map(|kind| self.kind(kind))
            .collect();
        path.reverse();
        path
    }

    /// Whether the type can move under `parent`: a type there, not inside itself, and within the
    /// depth rh allows.
    pub fn can_nest(&self, kind: &TopicId, parent: &TopicId) -> bool {
        self.kind(parent)
            .is_some_and(|under| self.atlas().can_nest(kind, &under.id))
    }

    /// The entities whose name or one of their aliases holds the query, the name matches first.
    pub fn find(&self, query: &str) -> Vec<&Entity> {
        let wanted = query.trim().to_lowercase();
        if wanted.is_empty() {
            return Vec::new();
        }
        let mut found: Vec<(bool, &Entity)> = self
            .entities
            .iter()
            .filter_map(|entity| {
                let by_name = entity.topic.name.to_lowercase().contains(&wanted);
                let by_alias = entity
                    .aliases
                    .iter()
                    .any(|alias| alias.to_lowercase().contains(&wanted));
                (by_name || by_alias).then_some((!by_name, entity))
            })
            .collect();
        found.sort_by(|(a_alias, a), (b_alias, b)| {
            a_alias
                .cmp(b_alias)
                .then_with(|| a.topic.name.to_lowercase().cmp(&b.topic.name.to_lowercase()))
        });
        found.into_iter().map(|(_, entity)| entity).collect()
    }

    fn atlas(&self) -> Atlas<'_> {
        Atlas::new(&self.types, &[], &NO_PLACEMENTS)
    }
}

static NO_PLACEMENTS: std::sync::LazyLock<Placements> = std::sync::LazyLock::new(Placements::new);

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(id: &str, parent: Option<&str>) -> Topic {
        Topic {
            id: TopicId::new(id),
            slug: id.to_owned(),
            name: id.to_owned(),
            parent: parent.map(TopicId::new),
            last_active_ms: None,
            updated_ms: 0,
            closed_ms: None,
            directory: None,
            owner: None,
            visibility: None,
        }
    }

    fn entity(id: &str, parent: &str, aliases: &[&str]) -> Entity {
        Entity {
            topic: kind(id, Some(parent)),
            aliases: aliases.iter().map(|alias| (*alias).to_owned()).collect(),
        }
    }

    #[test]
    fn a_type_holds_the_entities_of_its_subtypes_and_paths_run_from_the_top() {
        let catalog = Catalog {
            types: vec![kind("Software", None), kind("Libraries", Some("Software"))],
            entities: vec![
                entity("tokio", "Libraries", &[]),
                entity("adb", "Software", &[]),
                entity("gerty", "Hosts", &["10.4.20.12"]),
            ],
        };
        let names = |found: Vec<&Entity>| -> Vec<String> {
            found
                .into_iter()
                .map(|entity| entity.topic.name.clone())
                .collect()
        };
        assert_eq!(names(catalog.within(&TopicId::new("Software"))), ["adb", "tokio"]);
        assert_eq!(names(catalog.within(&TopicId::new("Libraries"))), ["tokio"]);
        let path: Vec<&str> = catalog
            .path(&TopicId::new("tokio"))
            .into_iter()
            .map(|kind| kind.name.as_str())
            .collect();
        assert_eq!(path, ["Software", "Libraries"]);
        assert_eq!(names(catalog.find("10.4")), ["gerty"]);
        assert!(!catalog.can_nest(&TopicId::new("Software"), &TopicId::new("Libraries")));
    }
}
