use serde::{Deserialize, Serialize};
use thiserror::Error;
use utoipa::ToSchema;

#[derive(Error, Debug)]
pub enum CompletionError {
    #[error("missing completion segments in request")]
    MissingSegments,

    #[error("fim prompt template is not configured")]
    MissingFimPromptTemplate,

    #[error("fim prompt template must contain both {{prefix}} and {{suffix}} placeholders")]
    InvalidFimPromptTemplate,
}

#[derive(Serialize, Deserialize, ToSchema, Clone, Debug)]
#[schema(example=json!({
    "language": "python",
    "segments": {
        "prefix": "def fib(n):\n    ",
        "suffix": "\n        return fib(n - 1) + fib(n - 2)"
    },
    "model": "random/model"
}))]
pub struct CompletionRequest {
    /// Language identifier, full list is maintained at
    /// https://code.visualstudio.com/docs/languages/identifiers
    #[schema(example = "python")]
    language: Option<String>,

    /// When segments are set, the `prompt` is ignored during the inference.
    pub segments: Option<Segments>,

    /// A unique identifier representing your end-user, which can help shaide to monitor & generating
    /// reports.
    pub(crate) user: Option<String>,

    debug_options: Option<DebugOptions>,

    /// The temperature parameter for the model, used to tune variance and "creativity" of the model output
    pub(crate) temperature: Option<f32>,

    /// The seed used for randomly selecting tokens
    pub(crate) seed: Option<u64>,

    /// The model that we wish to forward the requests
    pub model: String,
}

impl CompletionRequest {
    pub fn fim_segments(&self) -> Result<(&str, &str), CompletionError> {
        let segments = self
            .segments
            .as_ref()
            .ok_or(CompletionError::MissingSegments)?;
        Ok((
            segments.prefix.as_str(),
            segments.suffix.as_deref().unwrap_or(""),
        ))
    }

    pub fn has_raw_prompt(&self) -> bool {
        self.debug_options
            .as_ref()
            .is_some_and(|x| x.raw_prompt.is_some())
    }
}

#[derive(Serialize, Deserialize, ToSchema, Clone, Debug)]
pub struct DebugOptions {
    /// When `raw_prompt` is specified, it will be passed directly to the inference engine for completion. `segments` field in `CompletionRequest` will be ignored.
    ///
    /// This is useful for certain requests that aim to test the shaide's e2e quality.
    raw_prompt: Option<String>,

    /// When true, returns `snippets` in `debug_data`.
    #[serde(default = "default_false")]
    return_snippets: bool,

    /// When true, returns `prompt` in `debug_data`.
    #[serde(default = "default_false")]
    return_prompt: bool,

    /// When true, disable retrieval augmented code completion.
    #[serde(default = "default_false")]
    disable_retrieval_augmented_code_completion: bool,
}

fn default_false() -> bool {
    false
}

#[derive(Serialize, Deserialize, ToSchema, Clone, Debug)]
pub struct Segments {
    /// Content that appears before the cursor in the editor window.
    prefix: String,

    /// Content that appears after the cursor in the editor window.
    suffix: Option<String>,

    /// The relative path of the file that is being edited.
    /// - When [Segments::git_url] is set, this is the path of the file in the git repository.
    /// - When [Segments::git_url] is empty, this is the path of the file in the workspace.
    filepath: Option<String>,

    /// The remote URL of the current git repository.
    /// Leave this empty if the file is not in a git repository,
    /// or the git repository does not have a remote URL.
    git_url: Option<String>,

    /// The relevant declaration code snippets provided by the editor's LSP,
    /// contain declarations of symbols extracted from [Segments::prefix].
    declarations: Option<Vec<Declaration>>,

    /// The relevant code snippets extracted from recently edited files.
    /// These snippets are selected from candidates found within code chunks
    /// based on the edited location.
    /// The current editing file is excluded from the search candidates.
    ///
    /// When provided alongside [Segments::declarations], the snippets have
    /// already been deduplicated to ensure no duplication with entries
    /// in [Segments::declarations].
    ///
    /// Sorted in descending order of [Snippet::score].
    relevant_snippets_from_changed_files: Option<Vec<Snippet>>,

    /// The relevant code snippets extracted from recently opened files.
    /// These snippets are selected from candidates found within code chunks
    /// based on the last visited location.
    ///
    /// Current Active file is excluded from the search candidates.
    /// When provided with [Segments::relevant_snippets_from_changed_files], the snippets have
    /// already been deduplicated to ensure no duplication with entries
    /// in [Segments::relevant_snippets_from_changed_files].
    relevant_snippets_from_recently_opened_files: Option<Vec<Snippet>>,

    /// Clipboard content when requesting code completion.
    clipboard: Option<String>,
}

/// A snippet of declaration code that is relevant to the current completion request.
#[derive(Serialize, Deserialize, ToSchema, Clone, Debug)]
pub struct Declaration {
    /// Filepath of the file where the snippet is from.
    /// - When the file belongs to the same workspace as the current file,
    ///   this is a relative filepath, use the same rule as [Segments::filepath].
    /// - When the file located outside the workspace, such as in a dependency package,
    ///   this is a file URI with an absolute filepath.
    pub filepath: String,

    /// Body of the snippet.
    pub body: String,
}

#[derive(Serialize, Deserialize, ToSchema, Clone, Debug)]
pub struct Choice {
    index: u32,
    text: String,
}

impl Choice {
    pub fn new(text: String) -> Self {
        Self { index: 0, text }
    }
}

#[derive(Serialize, Deserialize, ToSchema, Clone, Debug, PartialEq)]
pub struct Snippet {
    filepath: String,
    body: String,
    score: f32,
}

#[derive(Serialize, Deserialize, ToSchema, Clone, Debug)]
#[schema(example=json!({
    "id": "string",
    "choices": [ { "index": 0, "text": "string" } ]
}))]
pub struct CompletionResponse {
    id: String,
    choices: Vec<Choice>,

    #[serde(skip_serializing_if = "Option::is_none")]
    debug_data: Option<DebugData>,
}

impl CompletionResponse {
    pub fn new(id: String, choices: Vec<Choice>, debug_data: Option<DebugData>) -> Self {
        Self {
            id,
            choices,
            debug_data,
        }
    }
}

#[derive(Serialize, Deserialize, ToSchema, Clone, Debug)]
pub struct DebugData {
    #[serde(skip_serializing_if = "Option::is_none")]
    snippets: Option<Vec<Snippet>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    prompt: Option<String>,
}

pub fn render_fim_prompt(
    template: Option<&str>,
    prefix: &str,
    suffix: &str,
) -> Result<String, CompletionError> {
    let template = template.ok_or(CompletionError::MissingFimPromptTemplate)?;
    if !template.contains("{prefix}") || !template.contains("{suffix}") {
        return Err(CompletionError::InvalidFimPromptTemplate);
    }

    let mut rendered = String::with_capacity(template.len() + prefix.len() + suffix.len());
    let mut remaining = template;

    while !remaining.is_empty() {
        if let Some(rest) = remaining.strip_prefix("{prefix}") {
            rendered.push_str(prefix);
            remaining = rest;
            continue;
        }

        if let Some(rest) = remaining.strip_prefix("{suffix}") {
            rendered.push_str(suffix);
            remaining = rest;
            continue;
        }

        let mut chars = remaining.chars();
        if let Some(ch) = chars.next() {
            rendered.push(ch);
            remaining = chars.as_str();
        }
    }

    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{CompletionError, CompletionRequest, render_fim_prompt};

    #[test]
    fn render_fim_prompt_replaces_prefix_and_suffix() {
        let rendered = render_fim_prompt(
            Some("<|fim_prefix|>{prefix}<|fim_suffix|>{suffix}<|fim_middle|>"),
            "let value = ",
            "42;",
        )
        .unwrap();

        assert_eq!(
            rendered,
            "<|fim_prefix|>let value = <|fim_suffix|>42;<|fim_middle|>"
        );
    }

    #[test]
    fn render_fim_prompt_requires_both_placeholders() {
        let err = render_fim_prompt(Some("{prefix} only"), "abc", "def").unwrap_err();
        assert!(matches!(err, CompletionError::InvalidFimPromptTemplate));
    }

    #[test]
    fn render_fim_prompt_does_not_mutate_inserted_segments() {
        let rendered = render_fim_prompt(
            Some("before:{prefix}:middle:{suffix}:after"),
            r#"let s = "{suffix}";"#,
            r#"let t = "{prefix}";"#,
        )
        .unwrap();

        assert_eq!(
            rendered,
            r#"before:let s = "{suffix}";:middle:let t = "{prefix}";:after"#
        );
    }

    #[test]
    fn completion_request_detects_raw_prompt() {
        let request: CompletionRequest = serde_json::from_value(json!({
            "model": "test-model",
            "debug_options": {
                "raw_prompt": "return 42;"
            }
        }))
        .unwrap();

        assert!(request.has_raw_prompt());
    }

    #[test]
    fn completion_request_without_raw_prompt_returns_false() {
        let request: CompletionRequest = serde_json::from_value(json!({
            "model": "test-model",
            "debug_options": {}
        }))
        .unwrap();

        assert!(!request.has_raw_prompt());
    }
}
