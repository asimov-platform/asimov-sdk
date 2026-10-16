// This is free and unencumbered software released into the public domain.

use crate::{Component, Error, FieldSection, Message, SecretKey, VerifyingKey, fields};
use alloc::{
    borrow::ToOwned,
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use http::{HeaderMap, HeaderValue};
use sfv::{BareItem, FieldType, InnerList, Item, ListEntry};

/// Application tag for ASIMOV remote execution requests.
pub const REQUEST_SIGNATURE_TAG: &str = "asimov-remote-request-v1";

/// ASCII key identifier used by ASIMOV's HTTP signature profiles.
pub fn key_id(key: &VerifyingKey) -> String {
    format!("ed25519:{}", bs58::encode(key.as_bytes()).into_string())
}

/// Parameters for newly generated Ed25519 signatures.
///
/// Parsed inputs retain their original parameter ordering and extensions in
/// `SignatureInput`; they are never reconstructed through this configuration.
#[derive(Clone, Debug)]
pub struct SignatureParams {
    /// Creation time in UNIX seconds.
    pub created: i64,
    /// Optional expiration time in UNIX seconds.
    pub expires: Option<i64>,
    /// Opaque key identifier, resolved and authorized by the application.
    pub key_id: String,
    /// Optional per-attempt nonce.
    pub nonce: Option<String>,
    /// Optional application profile tag.
    pub tag: Option<String>,
}

impl SignatureParams {
    /// Creates a 60-second signature with a fresh 256-bit OS-random nonce.
    /// Clock and entropy failures are returned before any request is dispatched.
    pub fn fresh(key: &VerifyingKey, tag: &str) -> Result<Self, Error> {
        let created = Self::now()?;
        let mut nonce = [0; 32];
        getrandom::fill(&mut nonce).map_err(|error| Error::Random(error.to_string()))?;
        Ok(Self {
            created,
            expires: Some(created.checked_add(60).ok_or(Error::Clock)?),
            key_id: key_id(key),
            nonce: Some(STANDARD.encode(nonce)),
            tag: Some(tag.into()),
        })
    }

    /// Returns the current UNIX time, for explicitly timed verification.
    pub fn now() -> Result<i64, Error> {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| Error::Clock)?
            .as_secs()
            .try_into()
            .map_err(|_| Error::Clock)
    }
}

/// An ordered, validated RFC 9421 inner list and its signature parameters.
///
/// RFC 8941 serialization preserves received component/parameter order, including
/// unknown signature parameters. Component identity compares parameter sets
/// without depending on their order, so equivalent duplicates are rejected.
/// At most 64 components and 64 KiB of serialized metadata are accepted.
#[derive(Clone, Debug)]
pub struct SignatureInput {
    inner: InnerList,
    components: Vec<Component>,
}

impl SignatureInput {
    /// Creates an input with `alg="ed25519"`, using Structured Fields escaping.
    pub fn new(components: Vec<Component>, params: SignatureParams) -> Result<Self, Error> {
        let mut inner = InnerList::new(components.iter().map(|c| c.0.clone()).collect());
        let integer = |value| {
            sfv::Integer::try_from(value)
                .map(BareItem::Integer)
                .map_err(Error::from)
        };
        inner
            .params
            .insert(sfv::key_ref("created").to_owned(), integer(params.created)?);
        if let Some(expires) = params.expires {
            inner
                .params
                .insert(sfv::key_ref("expires").to_owned(), integer(expires)?);
        }
        for (name, value) in [
            ("nonce", params.nonce.as_deref()),
            ("alg", Some("ed25519")),
            ("keyid", Some(params.key_id.as_str())),
            ("tag", params.tag.as_deref()),
        ] {
            if let Some(value) = value {
                inner.params.insert(
                    sfv::key_ref(name).to_owned(),
                    sfv::StringRef::from_str(value)?.into(),
                );
            }
        }
        Self::from_inner(inner)
    }

    /// Parses the inner-list value, excluding the dictionary label (`sig1=`).
    pub fn parse(value: &str) -> Result<Self, Error> {
        let mut list: sfv::List = fields::parser(value)?.parse()?;
        if list.len() != 1 {
            return Err(Error::InvalidField("expected one signature inner list"));
        }
        match list.remove(0) {
            ListEntry::InnerList(inner) => Self::from_inner(inner),
            _ => Err(Error::InvalidField("signature input must be an inner list")),
        }
    }

    fn from_inner(inner: InnerList) -> Result<Self, Error> {
        if inner.items.len() > fields::MAX_COMPONENTS {
            return Err(Error::Limit);
        }
        let mut components = Vec::new();
        for item in &inner.items {
            let component = Component::from_item(item.clone())?;
            if components.contains(&component) {
                return Err(Error::DuplicateComponent);
            }
            components.push(component);
        }
        for (name, value) in &inner.params {
            match name.as_str() {
                "created" | "expires" => {
                    if value.as_integer().is_none_or(|n| i64::from(n) < 0) {
                        return Err(Error::InvalidField("invalid signature timestamp"));
                    }
                },
                "alg" | "keyid" | "nonce" | "tag" if value.as_string().is_none() => {
                    return Err(Error::InvalidField("signature parameter must be a string"));
                },
                _ => (), // Preserve extensions in the signed parameter list.
            }
        }
        let result = Self { inner, components };
        if result.parameter("alg").is_some_and(|alg| alg != "ed25519") {
            return Err(Error::Algorithm);
        }
        if result.to_field_value().len() > fields::MAX_FIELD_BYTES {
            return Err(Error::Limit);
        }
        Ok(result)
    }

    /// Extracts the selected signature input from headers and/or trailers.
    /// Duplicate labels across sections are rejected as an application policy.
    pub fn from_message(message: &Message, label: &str) -> Result<Self, Error> {
        match selected(message, "signature-input", label)? {
            ListEntry::InnerList(inner) => Self::from_inner(inner),
            _ => Err(Error::InvalidField("signature input must be an inner list")),
        }
    }

    /// Returns the ordered covered components.
    pub fn components(&self) -> &[Component] {
        &self.components
    }

    /// Returns a string parameter, including preserved extension parameters.
    pub fn parameter(&self, name: &str) -> Option<&str> {
        self.inner
            .params
            .get(name)?
            .as_string()
            .map(|value| value.as_str())
    }

    /// Returns an integer parameter.
    pub fn integer_parameter(&self, name: &str) -> Option<i64> {
        self.inner.params.get(name)?.as_integer().map(i64::from)
    }

    /// Serializes the original ordered inner list with strict SF serialization.
    pub fn to_field_value(&self) -> String {
        vec![ListEntry::InnerList(self.inner.clone())]
            .serialize()
            .expect("one inner list")
    }

    /// Creates the RFC 9421 signature base, without a final newline.
    /// This contains covered credentials; callers must not log it indiscriminately.
    pub fn signature_base(&self, message: &Message) -> Result<Vec<u8>, Error> {
        let mut base = String::new();
        for component in &self.components {
            let value = message.value(component)?;
            base.push_str(&component.to_field_value());
            base.push_str(": ");
            base.push_str(&value);
            base.push('\n');
            if base.len() > fields::MAX_FIELD_BYTES {
                return Err(Error::Limit);
            }
        }
        base.push_str("\"@signature-params\": ");
        base.push_str(&self.to_field_value());
        if base.len() > fields::MAX_FIELD_BYTES {
            return Err(Error::Limit);
        }
        Ok(base.into_bytes())
    }

    /// Checks presently available covered fields before starting a stream.
    pub(crate) fn preflight(&self, message: &Message) -> Result<(), Error> {
        for component in &self.components {
            if !component.flag("tr") || component.flag("req") {
                message.value(component)?;
            }
        }
        Ok(())
    }

    pub(crate) fn covers_digest(&self, section: FieldSection) -> bool {
        self.components.iter().any(|component| {
            component.name() == "content-digest"
                && !component.flag("req")
                && component.flag("tr") == (section == FieldSection::Trailers)
                && component
                    .string_param("key")
                    .is_none_or(|key| key == "sha-256")
        })
    }

    /// Verifies Ed25519 only. Freshness, required coverage, authorization, replay,
    /// and body integrity are separate; use `Verifier` and body adapters for a
    /// policy-checked message. This primitive also supports static RFC fixtures.
    pub fn verify(
        &self,
        message: &Message,
        signature: &[u8],
        key: &VerifyingKey,
    ) -> Result<(), Error> {
        let signature =
            asimov_protocol::iroh::Signature::try_from(signature).map_err(|_| Error::Signature)?;
        key.verify(&self.signature_base(message)?, &signature)
            .map_err(|_| Error::Signature)
    }
}

/// The two RFC 9421 fields for a single signature label.
#[derive(Clone, Debug)]
pub struct SignatureFields {
    /// Serialized `Signature-Input` dictionary member.
    pub signature_input: HeaderValue,
    /// Serialized `Signature` dictionary member.
    pub signature: HeaderValue,
    label: String,
}

impl SignatureFields {
    /// Appends both fields while preserving other labels. Existing matching
    /// labels are rejected rather than overwritten.
    pub fn append_to(&self, fields: &mut HeaderMap) -> Result<(), Error> {
        for name in ["signature-input", "signature"] {
            if let Some(value) = fields::combined(&[fields], name)?
                && fields::dictionary(&value)?.contains_key(self.label.as_str())
            {
                return Err(Error::DuplicateLabel);
            }
        }
        fields.append("signature-input", self.signature_input.clone());
        fields.append("signature", self.signature.clone());
        Ok(())
    }
}

/// An Ed25519 signer for one signature input and label.
///
/// Construct a fresh input/nonce for each attempt. The native secret-key type
/// zeroizes its storage and redacts its debug representation.
#[derive(Clone, Debug)]
pub struct Signer {
    key: SecretKey,
    label: String,
    input: SignatureInput,
}

impl Signer {
    /// Creates a signer, validating the Structured Fields dictionary label.
    pub fn new(key: SecretKey, label: &str, input: SignatureInput) -> Result<Self, Error> {
        sfv::KeyRef::from_str(label)?;
        Ok(Self {
            key,
            label: label.into(),
            input,
        })
    }

    /// Returns the selected covered components and parameters.
    pub fn input(&self) -> &SignatureInput {
        &self.input
    }

    /// Returns the signature label.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Generates the signature fields without modifying the message or body.
    pub fn sign(&self, message: &Message) -> Result<SignatureFields, Error> {
        let signature = self
            .key
            .sign(&self.input.signature_base(message)?)
            .to_bytes();
        Ok(SignatureFields {
            signature_input: self.input_header()?,
            signature: fields::header(format!(
                "{}={}",
                self.label,
                Item::new(signature.as_slice()).serialize()
            ))?,
            label: self.label.clone(),
        })
    }

    pub(crate) fn input_header(&self) -> Result<HeaderValue, Error> {
        fields::header(format!("{}={}", self.label, self.input.to_field_value()))
    }
}

/// Required coverage and freshness for an application of RFC 9421.
///
/// Verification uses an explicit validation instant, normally message receipt,
/// so long-running response streams need not finish within the validity window.
/// Nonce checks here validate its format; applications must atomically reject
/// reused nonces after authenticating the signature and the complete body.
#[derive(Clone, Debug)]
pub struct VerificationPolicy {
    /// Components that must be covered, including their exact parameter sets.
    ///
    /// `"content-digest"` requires header coverage; `"content-digest";tr`
    /// requires trailer coverage. Listing both requires both, not either one.
    /// To let a response choose its digest location, omit its digest from this
    /// list and use [`crate::body::VerifyingBody`] with a verifier. That adapter
    /// independently requires a matching SHA-256 digest and signature coverage
    /// of its actual section. [`Verifier::verify`] alone does not check the body.
    ///
    /// # Examples
    ///
    /// ```
    /// # extern crate alloc;
    /// use alloc::vec;
    /// use asimov_http::{Component, VerificationPolicy};
    ///
    /// // With VerifyingBody, accept the response digest in either section.
    /// let either = VerificationPolicy::new(vec![
    ///     Component::new("@status")?,
    ///     Component::new("content-type")?,
    /// ]);
    ///
    /// // Require header coverage; a trailer-only digest cannot satisfy this.
    /// let mut in_headers = either.clone();
    /// in_headers.required_components.push(Component::new("content-digest")?);
    ///
    /// // Or require trailer coverage instead. Choose one policy per verifier.
    /// let mut in_trailers = either;
    /// in_trailers
    ///     .required_components
    ///     .push(Component::trailer("content-digest")?);
    /// # Ok::<(), asimov_http::Error>(())
    /// ```
    pub required_components: Vec<Component>,
    /// Required application tag, when set.
    pub required_tag: Option<String>,
    /// Maximum age and advertised lifetime in seconds. Defaults to 60.
    pub max_age: u64,
    /// Permitted clock skew in seconds. Defaults to 30.
    pub clock_skew: u64,
    /// Require a standard-Base64 nonce containing at least 128 bits.
    pub require_nonce: bool,
}

impl VerificationPolicy {
    /// The ASIMOV remote request profile: method, absolute target URI, bearer
    /// credential, Accept, content type, and SHA-256 content digest coverage.
    pub fn request() -> Self {
        let components = [
            "@method",
            "@target-uri",
            "authorization",
            "accept",
            "content-type",
            "content-digest",
        ]
        .into_iter()
        .map(|name| Component::new(name).expect("constant component identifier"))
        .collect();
        let mut policy = Self::new(components);
        policy.required_tag = Some(REQUEST_SIGNATURE_TAG.into());
        policy
    }

    /// Creates a policy with explicit coverage, a 60-second lifetime, 30-second
    /// tolerance, and a required nonce. Creation and expiry are always required.
    pub fn new(required_components: Vec<Component>) -> Self {
        Self {
            required_components,
            required_tag: None,
            max_age: 60,
            clock_skew: 30,
            require_nonce: true,
        }
    }

    fn check(&self, input: &SignatureInput, now: i64) -> Result<(), Error> {
        let created = input
            .integer_parameter("created")
            .ok_or(Error::Policy("missing created"))? as i128;
        let expires = input
            .integer_parameter("expires")
            .ok_or(Error::Policy("missing expires"))? as i128;
        let now = i128::from(now);
        let skew = i128::from(self.clock_skew);
        let age = i128::from(self.max_age);
        if now < 0
            || created > now + skew
            || created < now - age - skew
            || expires <= created
            || expires > created + age
            || now >= expires + skew
        {
            return Err(Error::Policy("signature validity window"));
        }
        if self
            .required_tag
            .as_deref()
            .is_some_and(|tag| input.parameter("tag") != Some(tag))
        {
            return Err(Error::Policy("signature tag"));
        }
        if self.require_nonce {
            let nonce = input
                .parameter("nonce")
                .ok_or(Error::Policy("missing nonce"))?;
            if STANDARD
                .decode(nonce)
                .map_or(true, |bytes| bytes.len() < 16)
            {
                return Err(Error::Policy("nonce must contain at least 128 bits"));
            }
        }
        if !self
            .required_components
            .iter()
            .all(|c| input.components.contains(c))
        {
            return Err(Error::Policy("required signature coverage"));
        }
        Ok(())
    }
}

/// A signature successfully checked against an explicit key and policy.
/// This alone does not attest body integrity or application authorization.
#[derive(Clone, Debug)]
pub struct VerifiedSignature {
    /// The verified label.
    pub label: String,
    /// Authenticated parameters, including the nonce for replay rejection.
    pub input: SignatureInput,
    /// The explicitly supplied verification key.
    pub key: VerifyingKey,
}

/// Verification against one application-resolved key, label, and policy.
#[derive(Clone, Debug)]
pub struct Verifier {
    key: VerifyingKey,
    key_id: String,
    label: String,
    policy: VerificationPolicy,
}

impl Verifier {
    /// Binds a trusted/resolved key to its expected key ID and signature label.
    pub fn new(
        key: VerifyingKey,
        key_id: &str,
        label: &str,
        policy: VerificationPolicy,
    ) -> Result<Self, Error> {
        sfv::KeyRef::from_str(label)?;
        sfv::StringRef::from_str(key_id)?;
        Ok(Self {
            key,
            key_id: key_id.into(),
            label: label.into(),
            policy,
        })
    }

    /// Verifies the selected signature in headers and/or trailers at `now`.
    /// It never accepts another key, label, or algorithm as a fallback.
    pub fn verify(&self, message: &Message, now: i64) -> Result<VerifiedSignature, Error> {
        let input = SignatureInput::from_message(message, &self.label)?;
        if input.parameter("keyid") != Some(self.key_id.as_str()) {
            return Err(Error::Policy("key ID"));
        }
        self.policy.check(&input, now)?;
        let ListEntry::Item(item) = selected(message, "signature", &self.label)? else {
            return Err(Error::InvalidField("signature must be a byte sequence"));
        };
        let bytes = item
            .bare_item
            .as_byte_sequence()
            .ok_or(Error::InvalidField("signature byte sequence"))?;
        input.verify(message, bytes, &self.key)?;
        Ok(VerifiedSignature {
            label: self.label.clone(),
            input,
            key: self.key,
        })
    }
}

fn selected(message: &Message, name: &str, label: &str) -> Result<ListEntry, Error> {
    let mut selected = None;
    for section in [FieldSection::Headers, FieldSection::Trailers] {
        if let Some(value) = fields::combined(&[message.fields(section)], name)? {
            let mut dictionary = fields::dictionary(&value)?;
            if dictionary.len() > fields::MAX_COMPONENTS {
                return Err(Error::Limit);
            }
            for entry in dictionary.values() {
                let valid = match (name, entry) {
                    ("signature-input", ListEntry::InnerList(_)) => true,
                    ("signature", ListEntry::Item(item)) => {
                        item.bare_item.as_byte_sequence().is_some()
                    },
                    _ => false,
                };
                if !valid {
                    return Err(Error::InvalidField("signature dictionary member type"));
                }
            }
            if let Some(entry) = dictionary.shift_remove(label) {
                if selected.is_some() {
                    return Err(Error::DuplicateLabel);
                }
                selected = Some(entry);
            }
        }
    }
    selected.ok_or_else(|| Error::MissingComponent(format!("{name};key={label}")))
}
