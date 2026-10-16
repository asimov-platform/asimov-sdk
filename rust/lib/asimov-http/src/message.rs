// This is free and unencumbered software released into the public domain.

use crate::{Error, fields};
use alloc::{
    borrow::ToOwned,
    boxed::Box,
    collections::BTreeMap,
    format,
    string::{String, ToString},
    vec::Vec,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use http::{HeaderMap, Method, StatusCode, header::HeaderName};
use sfv::{FieldType, Item};
use url::Url;

/// The field section containing a digest, signature, or covered HTTP field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FieldSection {
    /// Fields preceding the message body.
    Headers,
    /// Fields following the message body.
    Trailers,
}

/// The known type of a Structured Field covered using `;sf`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StructuredFieldType {
    /// A single item and its parameters.
    Item,
    /// A list of items or inner lists.
    List,
    /// An ordered dictionary.
    Dictionary,
}

#[derive(Clone, Debug)]
enum Kind {
    Request { method: Method, target: Url },
    Response(StatusCode),
}

/// Owned signature context, independent of HTTP versions and body types.
///
/// Servers must supply the effective *external* target URI. The HTTP/1.1
/// request target alone generally lacks scheme and authority. Only trusted
/// routing/proxy configuration may provide missing context. HTTP/2 uses the
/// same effective URI; pseudo-headers are represented by derived components.
///
/// Headers and trailers remain separate: `"content-digest"` and
/// `"content-digest";tr` are different covered components. Responses may retain
/// their originating request for `;req` components. The snapshot must describe
/// the message actually transmitted, after content coding and header changes.
#[derive(Clone)]
pub struct Message {
    kind: Kind,
    /// The message's header section.
    pub headers: HeaderMap,
    /// The message's trailer section, populated when it becomes available.
    pub trailers: HeaderMap,
    request: Option<Box<Message>>,
    request_target: Option<String>,
    structured: BTreeMap<String, StructuredFieldType>,
}

impl core::fmt::Debug for Message {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Message")
            .field("is_request", &matches!(self.kind, Kind::Request { .. }))
            .field("header_names", &self.headers.keys().collect::<Vec<_>>())
            .field("trailer_names", &self.trailers.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

impl Message {
    /// Creates request context from a method, absolute HTTP(S) URI, and headers.
    /// Userinfo and fragments are rejected because they are not HTTP targets.
    pub fn request(method: Method, target: &str, headers: HeaderMap) -> Result<Self, Error> {
        let target = Url::parse(target).map_err(|_| Error::InvalidTargetUri)?;
        if !matches!(target.scheme(), "http" | "https")
            || target.host_str().is_none()
            || !target.username().is_empty()
            || target.password().is_some()
            || target.fragment().is_some()
        {
            return Err(Error::InvalidTargetUri);
        }
        Ok(Self::new(Kind::Request { method, target }, headers))
    }

    /// Snapshots a request of any body type, including Axum's non-`Sync` body.
    /// `effective_uri` supplies the external URI; the original request-target
    /// form is also retained for HTTP/1.x `@request-target` signatures.
    pub fn from_request<B>(request: &http::Request<B>, effective_uri: &str) -> Result<Self, Error> {
        let mut result = Self::request(
            request.method().clone(),
            effective_uri,
            request.headers().clone(),
        )?;
        let target = if request.version() == http::Version::HTTP_2 {
            if request.method() == Method::CONNECT {
                request.uri().authority().map(ToString::to_string)
            } else {
                request.uri().path_and_query().map(ToString::to_string)
            }
        } else {
            Some(request.uri().to_string())
        };
        result.request_target = target;
        Ok(result)
    }

    /// Creates response context. Supply a request snapshot to enable `;req`.
    pub fn response(
        status: StatusCode,
        headers: HeaderMap,
        request: Option<Self>,
    ) -> Result<Self, Error> {
        if request
            .as_ref()
            .is_some_and(|r| !matches!(r.kind, Kind::Request { .. }))
        {
            return Err(Error::InvalidComponent("response request context".into()));
        }
        let mut result = Self::new(Kind::Response(status), headers);
        result.request = request.map(Box::new);
        Ok(result)
    }

    fn new(kind: Kind, headers: HeaderMap) -> Self {
        let structured = [
            "content-digest",
            "repr-digest",
            "want-content-digest",
            "want-repr-digest",
            "signature",
            "signature-input",
        ]
        .into_iter()
        .map(|name| (name.into(), StructuredFieldType::Dictionary))
        .collect();
        Self {
            kind,
            headers,
            trailers: HeaderMap::new(),
            request: None,
            request_target: None,
            structured,
        }
    }

    /// Registers a field's actual Structured Fields type for `;sf` processing.
    /// Digest and signature dictionaries are already registered. Types are never
    /// guessed by trying multiple parsers.
    pub fn with_structured_field(mut self, name: HeaderName, kind: StructuredFieldType) -> Self {
        self.structured.insert(name.as_str().into(), kind);
        self
    }

    /// Supplies the actual protocol-specific request target for `@request-target`.
    /// Prefer `@target-uri` when signatures must survive HTTP version changes.
    pub fn with_request_target(mut self, target: impl Into<String>) -> Self {
        self.request_target = Some(target.into());
        self
    }

    /// Returns the chosen field section without merging headers and trailers.
    pub fn fields(&self, section: FieldSection) -> &HeaderMap {
        match section {
            FieldSection::Headers => &self.headers,
            FieldSection::Trailers => &self.trailers,
        }
    }

    /// Returns the chosen mutable field section.
    pub fn fields_mut(&mut self, section: FieldSection) -> &mut HeaderMap {
        match section {
            FieldSection::Headers => &mut self.headers,
            FieldSection::Trailers => &mut self.trailers,
        }
    }

    pub(crate) fn permits_response_trailers(&self) -> bool {
        let Kind::Response(status) = self.kind else {
            return false;
        };
        !status.is_informational() && status != StatusCode::NO_CONTENT && status != StatusCode::RESET_CONTENT
            && status != StatusCode::NOT_MODIFIED
            && !self.request.as_ref().is_some_and(|request| matches!(&request.kind,
                Kind::Request { method, .. } if *method == Method::HEAD || (*method == Method::CONNECT && status.is_success())))
    }

    fn source(&self, component: &Component) -> Result<&Self, Error> {
        if component.flag("req") {
            if !matches!(self.kind, Kind::Response(_)) {
                return Err(Error::InvalidComponent("req on a request".into()));
            }
            self.request
                .as_deref()
                .ok_or_else(|| Error::MissingComponent("request context".into()))
        } else {
            Ok(self)
        }
    }

    pub(crate) fn value(&self, component: &Component) -> Result<String, Error> {
        let source = self.source(component)?;
        let name = component.name();
        let value = if name.starts_with('@') {
            source.derived(component)?
        } else {
            source.field(component)?
        };
        if !value.is_ascii() || value.contains(['\r', '\n']) {
            return Err(Error::InvalidComponent(
                "non-ASCII or multiline component value".into(),
            ));
        }
        Ok(value)
    }

    fn derived(&self, component: &Component) -> Result<String, Error> {
        if component.name() == "@status" {
            return match self.kind {
                Kind::Response(status) => Ok(status.as_str().into()),
                _ => Err(Error::InvalidComponent("@status on a request".into())),
            };
        }
        let Kind::Request { method, target } = &self.kind else {
            return Err(Error::InvalidComponent(
                "request-derived component without req".into(),
            ));
        };
        Ok(match component.name() {
            "@method" => method.as_str().into(), // Methods are case-sensitive.
            "@target-uri" => target.as_str().into(),
            "@authority" => target[url::Position::BeforeHost..url::Position::AfterPort].into(),
            "@scheme" => target.scheme().into(),
            "@path" => target.path().into(),
            "@query" => format!("?{}", target.query().unwrap_or("")),
            "@request-target" => self
                .request_target
                .clone()
                .ok_or_else(|| Error::MissingComponent("actual request-target".into()))?,
            "@query-param" => {
                let name = component
                    .string_param("name")
                    .ok_or_else(|| Error::InvalidComponent("query parameter name".into()))?;
                // RFC 9421 uses form parsing followed by form serialization, not
                // raw substring matching or query/body parameter merging.
                let encoded = |value: &str| {
                    url::form_urlencoded::byte_serialize(value.as_bytes()).collect::<String>()
                };
                let mut matches = target.query_pairs().filter(|(key, _)| encoded(key) == name);
                let (_, value) = matches
                    .next()
                    .ok_or_else(|| Error::MissingComponent(name.into()))?;
                if matches.next().is_some() {
                    return Err(Error::InvalidComponent("duplicate query parameter".into()));
                }
                encoded(&value)
            },
            _ => return Err(Error::InvalidComponent(component.name().into())),
        })
    }

    fn field(&self, component: &Component) -> Result<String, Error> {
        let map = if component.flag("tr") {
            &self.trailers
        } else {
            &self.headers
        };
        let name = component.name();
        if component.flag("bs") {
            let values = fields::values(&[map], name)?;
            if values.is_empty() {
                return Err(Error::MissingComponent(name.into()));
            }
            return Ok(values
                .iter()
                .map(|value| format!(":{}:", STANDARD.encode(value)))
                .collect::<Vec<_>>()
                .join(", "));
        }
        let value =
            fields::combined(&[map], name)?.ok_or_else(|| Error::MissingComponent(name.into()))?;
        if let Some(key) = component.string_param("key") {
            return fields::serialize_entry(
                fields::dictionary(&value)?
                    .get(key)
                    .ok_or_else(|| Error::MissingComponent(format!("{name};key={key}")))?,
            );
        }
        if component.flag("sf") {
            return match self
                .structured
                .get(name)
                .ok_or_else(|| Error::InvalidComponent("unknown structured field type".into()))?
            {
                StructuredFieldType::Item => {
                    Ok(fields::parser(&value)?.parse::<Item>()?.serialize())
                },
                StructuredFieldType::List => Ok(fields::parser(&value)?
                    .parse::<sfv::List>()?
                    .serialize()
                    .unwrap_or_default()),
                StructuredFieldType::Dictionary => {
                    Ok(fields::dictionary(&value)?.serialize().unwrap_or_default())
                },
            };
        }
        Ok(value)
    }
}

/// An RFC 9421 component identifier with ordered parameters.
///
/// Supports ordinary fields, `sf`, `key`, `bs`, `tr`, `req`, and the standard
/// request/response derived components. Boolean flags must be true. Unknown
/// component names or parameters are rejected, never silently discarded.
#[derive(Clone, Debug)]
pub struct Component(pub(crate) Item);

impl Component {
    /// Creates a plain lowercase field or derived-component identifier.
    pub fn new(name: &str) -> Result<Self, Error> {
        Self::from_item(Item::new(sfv::StringRef::from_str(name)?))
    }

    /// Parses an identifier such as `"content-digest";tr` or `"@method";req`.
    pub fn parse(value: &str) -> Result<Self, Error> {
        Self::from_item(fields::parser(value)?.parse()?)
    }

    /// Creates a trailer field identifier.
    pub fn trailer(name: &str) -> Result<Self, Error> {
        let mut component = Self::new(name)?;
        component
            .0
            .params
            .insert(sfv::key_ref("tr").to_owned(), true.into());
        Self::from_item(component.0)
    }

    pub(crate) fn from_item(item: Item) -> Result<Self, Error> {
        let name = item
            .bare_item
            .as_string()
            .ok_or_else(|| Error::InvalidComponent("identifier must be a string".into()))?
            .as_str();
        if name.starts_with('@') {
            if !matches!(
                name,
                "@method"
                    | "@target-uri"
                    | "@authority"
                    | "@scheme"
                    | "@path"
                    | "@query"
                    | "@query-param"
                    | "@request-target"
                    | "@status"
            ) {
                return Err(Error::InvalidComponent(name.into()));
            }
        } else if name.bytes().any(|b| b.is_ascii_uppercase())
            || HeaderName::from_bytes(name.as_bytes()).is_err()
        {
            return Err(Error::InvalidComponent(name.into()));
        }
        for (key, value) in &item.params {
            let allowed = match key.as_str() {
                "req" => name != "@status" && value.as_boolean() == Some(true),
                "name" => name == "@query-param" && value.as_string().is_some(),
                "key" => !name.starts_with('@') && value.as_string().is_some(),
                "sf" | "bs" | "tr" => !name.starts_with('@') && value.as_boolean() == Some(true),
                _ => false,
            };
            if !allowed {
                return Err(Error::InvalidComponent(format!("{name};{key}")));
            }
        }
        if name == "@query-param" && !item.params.contains_key("name") {
            return Err(Error::InvalidComponent(
                "query parameter requires name".into(),
            ));
        }
        if item.params.contains_key("bs")
            && (item.params.contains_key("sf") || item.params.contains_key("key"))
        {
            return Err(Error::InvalidComponent(
                "bs cannot be combined with sf/key".into(),
            ));
        }
        Ok(Self(item))
    }

    /// Returns the field or derived-component name.
    pub fn name(&self) -> &str {
        self.0
            .bare_item
            .as_string()
            .expect("validated component string")
            .as_str()
    }

    /// Serializes the identifier, preserving parameter order and escaping.
    pub fn to_field_value(&self) -> String {
        self.0.serialize()
    }

    pub(crate) fn flag(&self, name: &str) -> bool {
        self.0.params.contains_key(name)
    }

    pub(crate) fn string_param(&self, name: &str) -> Option<&str> {
        self.0.params.get(name)?.as_string().map(|v| v.as_str())
    }
}

impl PartialEq for Component {
    fn eq(&self, other: &Self) -> bool {
        self.0.bare_item == other.0.bare_item
            && self.0.params.len() == other.0.params.len()
            && self
                .0
                .params
                .iter()
                .all(|(key, value)| other.0.params.get(key) == Some(value))
    }
}
impl Eq for Component {}
