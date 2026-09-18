// This is free and unencumbered software released into the public domain.

//! Language-model inference: the prompter marker trait, formats, and model selection.

use crate::Execute;
use alloc::{string::String, vec::Vec};
use bon::Builder;

/// A language-model inference provider that turns a prompt into a response.
///
/// The default contract is UTF-8 text in and out. It does not standardize chat
/// roles, conversation delimiters, or tool calls; programs assigning those
/// meanings to text must document their conventions. Structured formats require
/// explicitly selected tokens and compatible profiles.
///
/// # Command-line contract
///
/// `PROGRAM [OPTIONS] [INPUT-FILE [OUTPUT-FILE]]`
///
/// Input and output default to stdin and stdout. A single operand selects
/// input; two select input then output, with `-` denoting a standard stream.
/// [`PrompterOptions`] selects formats (both default to `text`) and an inference
/// model (default `auto`). Repeated invocations need not be deterministic.
/// Response whitespace and newlines are part of the result and are preserved.
///
/// `T` is the implementation's response representation. See [`crate::programs`]
/// for links to concrete execution behavior and the
/// [prompter specification][spec].
///
/// [spec]: https://asimov-specs.github.io/program-patterns/#prompter
pub trait Prompter<T>: Execute<T> {}

/// Prompt/response formats and inference-model selection for a [`Prompter`].
///
/// `Default` leaves every optional field unset and `other` empty, requesting
/// the program's defaults. Values are forwarded without capability checks;
/// model identifiers and any nonstandard format tokens belong to the selected
/// provider. The example uses an illustrative provider-specific model name.
///
/// # Examples
///
/// ```rust
/// use asimov_patterns::PrompterOptions;
///
/// let options = PrompterOptions::builder()
///     .input("text")
///     .output("text")
///     .model("gemma3:1b")
///     .build();
/// ```
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Builder)]
#[builder(derive(Debug), on(String, into))]
pub struct PrompterOptions {
    /// Additional arguments, including optional prompt and response file operands.
    ///
    /// The wrapper appends these after generated format/model options. Each
    /// string is one literal argument; see [`crate::programs`] for ordering.
    /// A named prompt file replaces stdin as the program's payload source.
    #[builder(field)]
    pub other: Vec<String>,

    /// Prompt serialization passed as `--input=FORMAT` (`-i` in the CLI).
    ///
    /// `None` omits the option; the specified default is `text` (UTF-8).
    /// This does not select a prompt file or convert a stored prompt into the
    /// requested format. In particular, `text` implies no standard chat envelope.
    pub input: Option<String>,

    /// Inference model passed as `--model=MODEL` (`-m` in the CLI).
    ///
    /// `None` omits the option; the specified default is `auto`, whose selection
    /// policy is program-defined. An explicitly requested unavailable or
    /// unsupported model must cause program failure rather than silent substitution.
    pub model: Option<String>,

    /// Response serialization passed as `--output=FORMAT` (`-o` in the CLI).
    ///
    /// `None` omits the option; the specified default is `text` (UTF-8).
    /// This selects neither a response file nor a capture policy.
    pub output: Option<String>,
}

impl<S: prompter_options_builder::State> PrompterOptionsBuilder<S> {
    /// Appends one literal argument to [`PrompterOptions::other`], preserving order.
    pub fn other(mut self, flag: impl Into<String>) -> Self {
        self.other.push(flag.into());
        self
    }

    /// Appends a present argument to [`PrompterOptions::other`]; `None` adds nothing.
    pub fn maybe_other(mut self, flag: Option<impl Into<String>>) -> Self {
        if let Some(flag) = flag {
            self.other.push(flag.into());
        }
        self
    }
}
