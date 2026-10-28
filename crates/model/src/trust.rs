const DAY_MS: i64 = 86_400_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Certificate {
    pub subject: String,
    pub issuer: String,
    pub not_after_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Validity {
    Valid { days_left: i64 },
    Expired { days_ago: i64 },
}

/// The holder a mesh device certificate names: `spiffe://<trust domain>/device/<user>/<device>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Holder<'a> {
    pub user: &'a str,
    pub device: &'a str,
}

impl Certificate {
    pub fn validity(&self, now_ms: i64) -> Validity {
        let left = self.not_after_ms - now_ms;
        if left > 0 {
            Validity::Valid {
                days_left: left / DAY_MS,
            }
        } else {
            Validity::Expired {
                days_ago: -left / DAY_MS,
            }
        }
    }

    pub fn holder(&self) -> Option<Holder<'_>> {
        let path = self.subject.strip_prefix("spiffe://")?;
        let (_, path) = path.split_once('/')?;
        let mut parts = path.strip_prefix("device/")?.split('/');
        match (parts.next(), parts.next(), parts.next()) {
            (Some(user), Some(device), None) if !user.is_empty() && !device.is_empty() => {
                Some(Holder { user, device })
            }
            _ => None,
        }
    }
}

/// What this client presents to the mesh and the authority it trusts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trust {
    pub device: Option<Certificate>,
    pub authority: Certificate,
}

#[cfg(test)]
mod tests {
    use super::{Certificate, DAY_MS, Holder, Validity};

    fn cert(subject: &str, not_after_ms: i64) -> Certificate {
        Certificate {
            subject: subject.to_owned(),
            issuer: "Rivet Intermediate CA".to_owned(),
            not_after_ms,
        }
    }

    #[test]
    fn mesh_device_ids_name_their_holder() {
        let device = cert("spiffe://rivet.mesh/device/phil/phildesk-hub", 0);
        assert_eq!(
            device.holder(),
            Some(Holder {
                user: "phil",
                device: "phildesk-hub"
            })
        );
        assert_eq!(cert("spiffe://rivet.mesh/node/phildesk", 0).holder(), None);
        assert_eq!(cert("spiffe://rivet.mesh/device/phil", 0).holder(), None);
        assert_eq!(cert("Rivet Root CA", 0).holder(), None);
    }

    #[test]
    fn validity_counts_whole_days_either_side_of_expiry() {
        let expiry = cert("x", 10 * DAY_MS);
        assert_eq!(expiry.validity(DAY_MS / 2), Validity::Valid { days_left: 9 });
        assert_eq!(expiry.validity(10 * DAY_MS), Validity::Expired { days_ago: 0 });
        assert_eq!(expiry.validity(13 * DAY_MS), Validity::Expired { days_ago: 3 });
    }
}
