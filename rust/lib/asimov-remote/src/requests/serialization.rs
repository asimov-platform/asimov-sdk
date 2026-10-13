// This is free and unencumbered software released into the public domain.

use super::{FetcherRequest, ListerRequest};
use alloc::{
    borrow::Cow,
    format,
    string::{String, ToString},
    vec::Vec,
};
use asimov_patterns::{
    CachingOptions, FetcherOptions, FilteringOptions, ListerOptions, TimingOptions,
};
use clientele::options::sort::SortKeys;
use core::{fmt, str::FromStr};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

#[derive(Serialize)]
struct OptionsRef<'a, O> {
    #[serde(flatten)]
    pattern: O,
    #[serde(flatten)]
    caching: &'a CachingOptions,
    #[serde(flatten)]
    filtering: &'a FilteringOptions,
    #[serde(flatten)]
    timing: &'a TimingOptions,
}

#[derive(Default, Deserialize)]
struct Options<O> {
    #[serde(flatten)]
    pattern: O,
    #[serde(flatten)]
    caching: CachingOptions,
    #[serde(flatten)]
    filtering: FilteringOptions,
    #[serde(flatten)]
    timing: TimingOptions,
}

// Owned on input, borrowed where possible on output. Keeping this wire shape
// separate from ListerOptions supports Display-only keys and an empty sort.
#[derive(Default, Deserialize, Serialize)]
#[serde(default)]
struct ListerOptionsBody<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    sort: Option<Cow<'a, str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    before: Option<Cow<'a, str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    after: Option<Cow<'a, str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    offset: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output: Option<Cow<'a, str>>,
    #[serde(skip_serializing_if = "<[String]>::is_empty")]
    other: Cow<'a, [String]>,
}

fn shared_options_empty(
    caching: &CachingOptions,
    filtering: &FilteringOptions,
    timing: &TimingOptions,
) -> bool {
    let CachingOptions { max_age } = caching;
    let FilteringOptions { jev, jq } = filtering;
    let TimingOptions { deadline } = timing;
    max_age.is_none() && jev.is_none() && jq.is_none() && deadline.is_none()
}

impl Serialize for FetcherRequest {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Request<'a> {
            urls: &'a [String],
            #[serde(skip_serializing_if = "Option::is_none")]
            options: Option<OptionsRef<'a, &'a FetcherOptions>>,
        }
        let FetcherOptions { output, other } = &self.options;
        let empty = output.is_none()
            && other.is_empty()
            && shared_options_empty(&self.caching, &self.filtering, &self.timing);
        Request {
            urls: &self.urls,
            options: (!empty).then_some(OptionsRef {
                pattern: &self.options,
                caching: &self.caching,
                filtering: &self.filtering,
                timing: &self.timing,
            }),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for FetcherRequest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Request {
            urls: Vec<String>,
            #[serde(default)]
            options: Options<FetcherOptions>,
        }
        let Request { urls, options } = Request::deserialize(deserializer)?;
        Ok(Self {
            urls,
            options: options.pattern,
            caching: options.caching,
            filtering: options.filtering,
            timing: options.timing,
        })
    }
}

impl<T: Clone + fmt::Display, F: fmt::Display> Serialize for ListerRequest<T, F> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Request<'a> {
            url: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            options: Option<OptionsRef<'a, ListerOptionsBody<'a>>>,
        }
        // Keep this exhaustive so new pattern fields require a wire mapping.
        let ListerOptions {
            sort,
            before,
            after,
            offset,
            limit,
            output,
            other,
        } = &self.options;
        let empty = sort.is_none()
            && before.is_none()
            && after.is_none()
            && offset.is_none()
            && limit.is_none()
            && output.is_none()
            && other.is_empty()
            && shared_options_empty(&self.caching, &self.filtering, &self.timing);
        Request {
            url: &self.url,
            options: (!empty).then(|| OptionsRef {
                pattern: ListerOptionsBody {
                    sort: sort.as_ref().map(|keys| {
                        // SortKeys<T> only implements Display for string keys.
                        keys.keys()
                            .iter()
                            .map(|key| {
                                format!("{}{}", if key.descending() { "-" } else { "" }, key.key())
                            })
                            .collect::<Vec<_>>()
                            .join(",")
                            .into()
                    }),
                    before: before.as_deref().map(Cow::Borrowed),
                    after: after.as_deref().map(Cow::Borrowed),
                    offset: *offset,
                    limit: *limit,
                    output: output.as_ref().map(|value| Cow::Owned(value.to_string())),
                    other: Cow::Borrowed(other),
                },
                caching: &self.caching,
                filtering: &self.filtering,
                timing: &self.timing,
            }),
        }
        .serialize(serializer)
    }
}

impl<'de, T: Clone, F> Deserialize<'de> for ListerRequest<T, F>
where
    SortKeys<T>: FromStr,
    <SortKeys<T> as FromStr>::Err: fmt::Display,
    F: FromStr,
    F::Err: fmt::Display,
{
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Request {
            url: String,
            #[serde(default)]
            options: Options<ListerOptionsBody<'static>>,
        }
        let Request { url, options } = Request::deserialize(deserializer)?;
        let ListerOptionsBody {
            sort,
            before,
            after,
            offset,
            limit,
            output,
            other,
        } = options.pattern;
        Ok(Self {
            url,
            options: ListerOptions {
                sort: sort
                    .map(|value| {
                        if value.is_empty() {
                            Ok(Vec::new().into())
                        } else {
                            value.parse().map_err(D::Error::custom)
                        }
                    })
                    .transpose()?,
                before: before.map(Cow::into_owned),
                after: after.map(Cow::into_owned),
                offset,
                limit,
                output: output
                    .map(|value| value.parse().map_err(D::Error::custom))
                    .transpose()?,
                other: other.into_owned(),
            },
            caching: options.caching,
            filtering: options.filtering,
            timing: options.timing,
        })
    }
}
