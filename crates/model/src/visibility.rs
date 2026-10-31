const PRIVATE: &str = "private";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Visibility {
    Private,
    Space(String),
}

impl Visibility {
    pub fn parse(text: &str) -> Self {
        if text == PRIVATE {
            Self::Private
        } else {
            Self::Space(text.to_owned())
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Private => PRIVATE,
            Self::Space(id) => id,
        }
    }

    pub fn is_private(&self) -> bool {
        matches!(self, Self::Private)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionVisibility {
    pub effective: Visibility,
    pub marked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Space {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Me {
    pub user: String,
    pub space: Option<Space>,
    pub default: Visibility,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub to: Visibility,
    pub label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Private,
    Shared,
}

impl Me {
    pub fn owns(&self, owner: Option<&str>) -> bool {
        owner == Some(self.user.as_str())
    }

    pub fn offer(&self, known: Option<&Visibility>) -> Option<Offer> {
        let space = self.space.as_ref()?;
        Some(if known.unwrap_or(&self.default).is_private() {
            Offer {
                to: Visibility::Space(space.id.clone()),
                label: format!("Share with {}", space.name.to_lowercase()),
            }
        } else {
            Offer {
                to: Visibility::Private,
                label: "Mark private".to_owned(),
            }
        })
    }

    pub fn glyph(&self, known: Option<&Visibility>) -> Option<Glyph> {
        self.space.as_ref()?;
        match (self.default.is_private(), known?.is_private()) {
            (false, true) => Some(Glyph::Private),
            (true, false) => Some(Glyph::Shared),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Glyph, Me, Space, Visibility};

    fn me(default: &str, space: bool) -> Me {
        Me {
            user: "phil".into(),
            space: space.then(|| Space {
                id: "household".into(),
                name: "Household".into(),
            }),
            default: Visibility::parse(default),
        }
    }

    fn label(me: &Me, known: Option<&str>) -> Option<String> {
        let known = known.map(Visibility::parse);
        me.offer(known.as_ref()).map(|offer| offer.label)
    }

    #[test]
    fn the_menu_offers_the_other_side_of_what_applies() {
        let sharing = me("household", true);
        let private = me("private", true);
        assert_eq!(
            label(&sharing, Some("private")).as_deref(),
            Some("Share with household")
        );
        assert_eq!(
            label(&sharing, Some("household")).as_deref(),
            Some("Mark private")
        );
        assert_eq!(label(&sharing, None).as_deref(), Some("Mark private"));
        assert_eq!(label(&private, None).as_deref(), Some("Share with household"));
        assert_eq!(
            private.offer(None).map(|offer| offer.to),
            Some(Visibility::Space("household".into()))
        );
        assert_eq!(label(&me("private", false), Some("private")), None);
        assert!(sharing.owns(Some("phil")));
        assert!(!sharing.owns(Some("coco")));
        assert!(!sharing.owns(None));
    }

    #[test]
    fn rows_mark_only_what_differs_from_the_default() {
        let private = Visibility::Private;
        let shared = Visibility::Space("household".into());
        let sharing = me("household", true);
        assert_eq!(sharing.glyph(Some(&private)), Some(Glyph::Private));
        assert_eq!(sharing.glyph(Some(&shared)), None);
        assert_eq!(sharing.glyph(None), None);
        let keeping = me("private", true);
        assert_eq!(keeping.glyph(Some(&shared)), Some(Glyph::Shared));
        assert_eq!(keeping.glyph(Some(&private)), None);
        assert_eq!(me("household", false).glyph(Some(&private)), None);
    }
}
