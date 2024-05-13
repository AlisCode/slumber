use crate::{
    collection::{
        self, Chain, ChainOutputTrim, ChainSource, Collection, Folder, Profile,
        ProfileId, Recipe, RecipeId, RecipeNode, RecipeTree,
    },
    config::Config,
    db::CollectionDatabase,
    http::{Body, Request, RequestId, RequestRecord, Response},
    template::{Prompt, Prompter, Template, TemplateContext},
    tui::{
        context::TuiContext,
        message::{Message, MessageSender},
    },
};
use chrono::Utc;
use indexmap::IndexMap;
use ratatui::{backend::TestBackend, Terminal};
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue},
    StatusCode,
};
use std::{
    env, fs,
    path::{Path, PathBuf},
};
use tokio::sync::{mpsc, mpsc::UnboundedReceiver};
use uuid::Uuid;

/// Test-only trait to build a placeholder instance of a struct. This is similar
/// to `Default`, but allows for useful placeholders that may not make sense in
/// the context of the broader app. It also makes it possible to implement a
/// factory for a type that already has `Default`.
pub trait Factory {
    fn factory() -> Self;
}

impl Factory for Collection {
    fn factory() -> Self {
        Self::default()
    }
}

impl Factory for Profile {
    fn factory() -> Self {
        Self {
            id: "profile1".into(),
            name: None,
            data: IndexMap::new(),
        }
    }
}

impl Factory for Folder {
    fn factory() -> Self {
        Self {
            id: "folder1".into(),
            name: None,
            children: IndexMap::new(),
        }
    }
}

impl Factory for Recipe {
    fn factory() -> Self {
        Self {
            id: "recipe1".into(),
            name: None,
            method: collection::Method::Get,
            url: "http://localhost/url".into(),
            body: None,
            authentication: None,
            query: IndexMap::new(),
            headers: IndexMap::new(),
        }
    }
}

impl Factory for Chain {
    fn factory() -> Self {
        Self {
            id: "chain1".into(),
            source: ChainSource::Request {
                recipe: "recipe1".into(),
                trigger: Default::default(),
                section: Default::default(),
            },
            sensitive: false,
            selector: None,
            content_type: None,
            trim: ChainOutputTrim::default(),
        }
    }
}

impl Factory for Request {
    fn factory() -> Self {
        Self {
            id: RequestId::new(),
            profile_id: None,
            recipe_id: "recipe1".into(),
            method: reqwest::Method::GET,
            url: "http://localhost/url".parse().unwrap(),
            headers: HeaderMap::new(),
            body: None,
        }
    }
}

impl Factory for Response {
    fn factory() -> Self {
        Self {
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            body: Body::default(),
        }
    }
}

impl Factory for RequestRecord {
    fn factory() -> Self {
        let request = Request::factory();
        let response = Response::factory();
        Self {
            id: request.id,
            request: request.into(),
            response: response.into(),
            start_time: Utc::now(),
            end_time: Utc::now(),
        }
    }
}

impl Factory for TemplateContext {
    fn factory() -> Self {
        Self {
            collection: Collection::default(),
            selected_profile: None,
            http_engine: None,
            database: CollectionDatabase::factory(),
            overrides: IndexMap::new(),
            prompter: Box::<TestPrompter>::default(),
            recursion_count: 0.into(),
        }
    }
}

/// Directory containing static test data
#[rstest::fixture]
pub fn test_data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("test_data")
}

/// Create a new temporary folder. This will include a random subfolder to
/// guarantee uniqueness for this test.
#[rstest::fixture]
pub fn temp_dir() -> PathBuf {
    let path = env::temp_dir().join(Uuid::new_v4().to_string());
    fs::create_dir(&path).unwrap();
    path
}

/// Create a terminal instance for testing
#[rstest::fixture]
pub fn terminal() -> Terminal<TestBackend> {
    let backend = TestBackend::new(10, 10);
    Terminal::new(backend).unwrap()
}

/// Test fixture for using context. This will initialize it once for all tests
#[rstest::fixture]
#[once]
pub fn tui_context() {
    TuiContext::init(Config::default(), CollectionDatabase::factory());
}

#[rstest::fixture]
pub fn messages() -> MessageQueue {
    let (tx, rx) = mpsc::unbounded_channel();
    MessageQueue { tx: tx.into(), rx }
}

/// Test-only wrapper for MPSC receiver, to test what messages have been queued
pub struct MessageQueue {
    tx: MessageSender,
    rx: UnboundedReceiver<Message>,
}

impl MessageQueue {
    /// Get the message sender
    pub fn tx(&self) -> &MessageSender {
        &self.tx
    }

    /// Pop the next message off the queue. Panic if the queue is empty
    pub fn pop_now(&mut self) -> Message {
        self.rx.try_recv().expect("Message queue empty")
    }

    /// Pop the next message off the queue, waiting if empty
    pub async fn pop_wait(&mut self) -> Message {
        self.rx.recv().await.expect("Message queue closed")
    }

    /// Clear all messages in the queue
    pub fn clear(&mut self) {
        while self.rx.try_recv().is_ok() {}
    }
}

/// Return a static value when prompted, or no value if none is given
#[derive(Debug, Default)]
pub struct TestPrompter {
    value: Option<String>,
}

impl TestPrompter {
    pub fn new<T: Into<String>>(value: Option<T>) -> Self {
        Self {
            value: value.map(Into::into),
        }
    }
}

impl Prompter for TestPrompter {
    fn prompt(&self, prompt: Prompt) {
        // If no value was given, check default. If no default, don't respond
        if let Some(value) = self.value.as_ref() {
            prompt.channel.respond(value.clone())
        } else if let Some(default) = prompt.default {
            prompt.channel.respond(default);
        }
    }
}

// Some helpful conversion implementations
impl From<&str> for ProfileId {
    fn from(value: &str) -> Self {
        value.to_owned().into()
    }
}

impl From<IndexMap<RecipeId, Recipe>> for RecipeTree {
    fn from(value: IndexMap<RecipeId, Recipe>) -> Self {
        let tree = value
            .into_iter()
            .map(|(id, recipe)| (id, RecipeNode::Recipe(recipe)))
            .collect();
        Self::new(tree).expect("Duplicate recipe ID")
    }
}

impl From<&str> for RecipeId {
    fn from(value: &str) -> Self {
        value.to_owned().into()
    }
}

impl From<&str> for Template {
    fn from(value: &str) -> Self {
        value.to_owned().try_into().unwrap()
    }
}
// Can't implement this for From<String> because it conflicts with TryFrom

/// Helper for creating a header map
pub fn header_map<'a>(
    headers: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> HeaderMap {
    headers
        .into_iter()
        .map(|(header, value)| {
            (
                HeaderName::try_from(header).unwrap(),
                HeaderValue::try_from(value).unwrap(),
            )
        })
        .collect()
}

/// Assert a result is the `Err` variant, and the stringified error contains
/// the given message
macro_rules! assert_err {
    ($e:expr, $msg:expr) => {{
        use itertools::Itertools as _;

        let msg = $msg;
        // Include all source errors so wrappers don't hide the important stuff
        let error: anyhow::Error = $e.unwrap_err().into();
        let actual = error.chain().map(ToString::to_string).join(": ");
        assert!(
            actual.contains(msg),
            "Expected error message to contain {msg:?}, but was: {actual:?}"
        )
    }};
}
pub(crate) use assert_err;
