// This is free and unencumbered software released into the public domain.

use derive_more::Display;

/// The direction of a follow relationship with another account.
///
/// Relationships are expressed from the perspective of a current account.
/// [`Follower`](Self::Follower) and [`Followee`](Self::Followee) are one-way
/// relationships, while [`Mutual`](Self::Mutual) represents both directions.
///
/// This enum intentionally has no `None` variant. Represent an optional
/// relationship as `Option<FollowRelationship>`, where `None` means no
/// relationship.
#[derive(Clone, Copy, Debug, Display, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[display(rename_all = "lowercase")]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "lowercase")
)]
pub enum FollowRelationship {
    /// A follower. (Someone who follows you.)
    ///
    /// A one-way relationship in which the other account follows the current
    /// account.
    Follower,

    /// A followee. (Someone whom you follow.)
    ///
    /// A one-way relationship in which the current account follows the other
    /// account.
    Followee,

    /// A mutual. (You both follow each other.)
    ///
    /// A two-way relationship in which both accounts follow each other.
    Mutual,
}

impl FollowRelationship {
    /// Returns the singular noun for this relationship.
    ///
    /// # Examples
    ///
    /// ```
    /// use asimov_social::FollowRelationship;
    ///
    /// assert_eq!(FollowRelationship::Follower.singular(), "follower");
    /// ```
    #[must_use]
    pub const fn singular(self) -> &'static str {
        match self {
            FollowRelationship::Follower => "follower",
            FollowRelationship::Followee => "followee",
            FollowRelationship::Mutual => "mutual",
        }
    }

    /// Returns the plural noun for this relationship.
    ///
    /// # Examples
    ///
    /// ```
    /// use asimov_social::FollowRelationship;
    ///
    /// assert_eq!(FollowRelationship::Mutual.plural(), "mutuals");
    /// ```
    #[must_use]
    pub const fn plural(self) -> &'static str {
        match self {
            FollowRelationship::Follower => "followers",
            FollowRelationship::Followee => "followees",
            FollowRelationship::Mutual => "mutuals",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FollowRelationship;
    use alloc::string::ToString;

    #[test]
    fn follow_relationship_labels() {
        let cases = [
            (FollowRelationship::Follower, "follower", "followers"),
            (FollowRelationship::Followee, "followee", "followees"),
            (FollowRelationship::Mutual, "mutual", "mutuals"),
        ];

        for (relationship, singular, plural) in cases {
            assert_eq!(relationship.singular(), singular);
            assert_eq!(relationship.plural(), plural);
            assert_eq!(relationship.to_string(), singular);
        }
    }
}
