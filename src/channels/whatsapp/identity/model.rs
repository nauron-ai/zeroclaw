use super::normalize::{normalize_phone_number, normalize_text};
use super::WhatsAppAllowlistEntry;
use std::fmt;
use wa_rs_binary::jid::Jid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WhatsAppIdentityStatus {
    Recognized,
    Technical,
}

impl fmt::Display for WhatsAppIdentityStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Recognized => f.write_str("recognized"),
            Self::Technical => f.write_str("technical"),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct WhatsAppIdentity {
    pub(in crate::channels::whatsapp::identity) lid: Option<String>,
    pub(in crate::channels::whatsapp::identity) phone_number: Option<String>,
    pub(in crate::channels::whatsapp::identity) full_name: Option<String>,
    pub(in crate::channels::whatsapp::identity) first_name: Option<String>,
    pub(in crate::channels::whatsapp::identity) push_name: Option<String>,
    pub(in crate::channels::whatsapp::identity) username: Option<String>,
    pub(in crate::channels::whatsapp::identity) about: Option<String>,
    pub(in crate::channels::whatsapp::identity) verified_name: Option<String>,
    pub(in crate::channels::whatsapp::identity) last_seen_jid: Option<String>,
    pub(in crate::channels::whatsapp::identity) updated_at: i64,
}

impl WhatsAppIdentity {
    pub(crate) fn lid(&self) -> Option<&str> {
        self.lid.as_deref()
    }

    pub(crate) fn phone_number(&self) -> Option<&str> {
        self.phone_number.as_deref()
    }

    pub(crate) fn display_name(&self) -> Option<&str> {
        self.verified_name
            .as_deref()
            .or(self.full_name.as_deref())
            .or(self.push_name.as_deref())
            .or(self.first_name.as_deref())
            .or(self.username.as_deref())
            .or(self.phone_number())
            .or(self.lid())
    }

    pub(crate) fn status(&self) -> WhatsAppIdentityStatus {
        if self.verified_name.is_some()
            || self.full_name.is_some()
            || self.push_name.is_some()
            || self.first_name.is_some()
            || self.username.is_some()
        {
            WhatsAppIdentityStatus::Recognized
        } else {
            WhatsAppIdentityStatus::Technical
        }
    }

    pub(crate) fn canonical_sender(&self) -> Option<String> {
        self.phone_number
            .clone()
            .or_else(|| self.lid.as_ref().map(|lid| format!("+{lid}")))
    }

    pub(crate) fn matches_allowlist(&self, allowed_identities: &[WhatsAppAllowlistEntry]) -> bool {
        allowed_identities
            .iter()
            .any(|allowed_identity| allowed_identity.matches(self))
    }

    pub(crate) fn summary(&self) -> String {
        let phone = self.phone_number().unwrap_or("-");
        let lid = self.lid().unwrap_or("-");
        let name = self.display_name().unwrap_or("-");
        format!(
            "phone={phone} lid={lid} name={name} status={}",
            self.status()
        )
    }

    pub(in crate::channels::whatsapp::identity) fn merge(&self, newer: &Self) -> Self {
        Self {
            lid: merge_field(self.lid.as_deref(), newer.lid.as_deref()),
            phone_number: merge_field(self.phone_number.as_deref(), newer.phone_number.as_deref()),
            full_name: merge_field(self.full_name.as_deref(), newer.full_name.as_deref()),
            first_name: merge_field(self.first_name.as_deref(), newer.first_name.as_deref()),
            push_name: merge_field(self.push_name.as_deref(), newer.push_name.as_deref()),
            username: merge_field(self.username.as_deref(), newer.username.as_deref()),
            about: merge_field(self.about.as_deref(), newer.about.as_deref()),
            verified_name: merge_field(
                self.verified_name.as_deref(),
                newer.verified_name.as_deref(),
            ),
            last_seen_jid: merge_field(
                self.last_seen_jid.as_deref(),
                newer.last_seen_jid.as_deref(),
            ),
            updated_at: self.updated_at.max(newer.updated_at),
        }
    }

    pub(in crate::channels::whatsapp::identity) fn with_updated_at(
        mut self,
        updated_at: i64,
    ) -> Self {
        self.updated_at = updated_at;
        self
    }

    pub(in crate::channels::whatsapp::identity) fn matches_identity(&self, other: &Self) -> bool {
        let self_keys = self.match_keys();
        let other_keys = other.match_keys();

        self_keys.iter().flatten().any(|self_key| {
            other_keys
                .iter()
                .flatten()
                .any(|other_key| self_key == other_key)
        })
    }

    fn match_keys(&self) -> [Option<String>; 3] {
        [
            self.phone_number.clone(),
            self.canonical_sender(),
            self.last_seen_jid
                .as_deref()
                .and_then(normalize_phone_number),
        ]
    }
}

fn merge_field(current: Option<&str>, newer: Option<&str>) -> Option<String> {
    newer.or(current).map(ToOwned::to_owned)
}

impl From<&str> for WhatsAppIdentity {
    fn from(recipient: &str) -> Self {
        if recipient.trim().contains('@') {
            if let Ok(jid) = recipient.parse::<Jid>() {
                return Self::from(&jid);
            }
        }

        Self {
            phone_number: normalize_phone_number(recipient),
            last_seen_jid: normalize_text(recipient),
            ..Self::default()
        }
    }
}
