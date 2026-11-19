//! Keevoro for an AI agent: a Model Context Protocol server.
//!
//! `keevoro mcp` starts this. An AI app (Claude Desktop, Claude Code and
//! others) starts it as a program of its own and speaks the protocol over
//! its standard input and output. No network port is opened.
//!
//! It holds no logic about photos and no rules of its own. Each tool is an
//! operation in the API's register ([`tools`]); each call goes through
//! [`keevoro_api::call_for_agent`], which applies what the person has
//! allowed and writes what the agent did to Keevoro's log; and each answer
//! is the API's own JSON with a short summary in plain English ([`said`]).
//! Keevoro's log can also be read, as a resource.
//!
//! Standard output carries the protocol and nothing else. Anything Keevoro
//! has to say goes to its log, which is why this program writes nothing
//! itself. It does nothing between calls, and when the AI app closes its
//! side, any job it started is stopped after the file in hand, as closing
//! the Keevoro app would, and it ends.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod said;
pub mod tools;

use keevoro_api::{call_for_agent, read_for_agent, Keevoro, Op};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation, ListResourcesResult,
    ListToolsResult, PaginatedRequestParams, ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult,
    Resource, ResourceContents, ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};

/// The address of Keevoro's log, as a resource.
pub const LOG: &str = "keevoro://log";

/// How long a job the agent started is given to finish the file in hand
/// once the AI app has closed its side.
const LET_JOBS_FINISH: Duration = Duration::from_secs(60);

/// What the agent is told when it connects, before it reads any tool.
const INSTRUCTIONS: &str = "Keevoro finds the photos and videos on a person's drives, memory cards and folders \
     (its sources) and backs them up to folders or cloud storage they choose (its destinations). Sources are only \
     ever read. Start with list_backups and my_access: the person decides which folders you may look in and back \
     up to, and whether you may change anything; until they allow changes you may only look. When something is \
     refused, pass the reason on to the person in their words. Looking through a source, surveying, backing up \
     and checking are jobs: the tool answers at once with a job id, and job_status (with waitSeconds) follows it. \
     Before run_backup, use rehearse_backup and tell the person what it says.";

/// The server: Keevoro, open for an agent.
#[derive(Clone)]
pub struct Server {
    keevoro: Arc<Keevoro>,
}

impl Server {
    /// `keevoro` should be opened as the agent's front end, so that the
    /// jobs it starts say an agent started them.
    pub fn new(keevoro: Keevoro) -> Self {
        Self {
            keevoro: Arc::new(keevoro),
        }
    }

    /// Serves one AI app over `input` and `output` until it closes its
    /// side, then stops any job it started (after the file in hand) and
    /// returns.
    pub async fn serve<R, W>(self, input: R, output: W) -> std::io::Result<()>
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        let keevoro = self.keevoro.clone();
        keevoro.log().say("An AI agent's app started Keevoro's agent server");
        let served = match ServiceExt::serve(self, (input, output)).await {
            Ok(running) => running.waiting().await.map(drop).map_err(std::io::Error::other),
            // The app went away, or spoke something other than the
            // protocol, before saying hello. Nothing was started.
            Err(e) => {
                keevoro
                    .log()
                    .say(&format!("An AI agent's app could not start talking to Keevoro: {e}"));
                Ok(())
            }
        };
        let finished = tokio::task::spawn_blocking(move || {
            let finished = keevoro.stop_here_and_wait(LET_JOBS_FINISH);
            keevoro.log().say(if finished {
                "An AI agent's app closed Keevoro's agent server"
            } else {
                "An AI agent's app closed Keevoro's agent server while a job was still finishing the file in hand"
            });
        })
        .await;
        finished.map_err(std::io::Error::other)?;
        served
    }

    /// Calls one tool, by its name, with its arguments.
    fn call(&self, name: &str, arguments: Option<rmcp::model::JsonObject>) -> Result<CallToolResult, ErrorData> {
        let op = tools::op_named(name)
            .ok_or_else(|| ErrorData::invalid_params(format!("Keevoro has no tool called \"{name}\"."), None))?;
        let mut request = arguments.unwrap_or_default();
        // The backup a tool works on is named beside its request.
        let backup = if op.on_a_backup() {
            match request.remove("backup") {
                None | Some(Value::Null) => None,
                Some(Value::String(name)) => Some(name),
                Some(_) => {
                    return Ok(problem(
                        "not_understood",
                        "Name a backup as text, as list_backups gives it.",
                    ))
                }
            }
        } else {
            None
        };
        let answer = call_for_agent(&self.keevoro, op, backup.as_deref(), Value::Object(request));
        Ok(match answer {
            Ok(answer) => answered(op, answer),
            Err(e) => problem(&kind_name(e.kind), &e.message),
        })
    }
}

/// An answer, as the protocol carries it: a summary to pass on, then the
/// API's JSON exactly. A picture is a picture.
fn answered(op: Op, answer: Value) -> CallToolResult {
    if op == Op::PhotosPicture {
        if let Some(data) = answer.get("base64").and_then(Value::as_str) {
            return CallToolResult::success(vec![
                ContentBlock::text("Here is the picture."),
                ContentBlock::image(data, "image/webp"),
            ]);
        }
    }
    let mut result = CallToolResult::success(vec![
        ContentBlock::text(said::summary(op, &answer)),
        ContentBlock::text(answer.to_string()),
    ]);
    // Structured content must be an object; an answer that is a list, a
    // yes or no or nothing is in the text alone.
    if answer.is_object() {
        result.structured_content = Some(answer);
    }
    result
}

/// A tool that could not do what was asked: the API's sentence, and its
/// kind, for the agent to act on.
fn problem(kind: &str, message: &str) -> CallToolResult {
    let mut result = CallToolResult::error(vec![
        ContentBlock::text(message),
        ContentBlock::text(json!({ "kind": kind, "message": message }).to_string()),
    ]);
    result.structured_content = Some(json!({ "kind": kind, "message": message }));
    result
}

/// An API problem's kind, as its JSON names it.
fn kind_name(kind: keevoro_api::Kind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "failed".to_string())
}

impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().enable_resources().build())
            .with_server_info(
                Implementation::new("keevoro", env!("CARGO_PKG_VERSION"))
                    .with_title("Keevoro")
                    .with_website_url("https://github.com/markcastle/keevoro"),
            )
            .with_instructions(INSTRUCTIONS)
    }

    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: tools::all(),
            ..Default::default()
        })
    }

    fn get_tool(&self, name: &str) -> Option<rmcp::model::Tool> {
        tools::op_named(name).and_then(tools::tool_for)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        // Keevoro's operations block; a long one (a survey, a wait for a
        // job) must not hold up the protocol, so each has a thread.
        let server = self.clone();
        let CallToolRequestParams { name, arguments, .. } = request;
        let result = tokio::task::spawn_blocking(move || server.call(&name, arguments))
            .await
            .map_err(|e| ErrorData::internal_error(format!("Keevoro ran into a problem: {e}"), None))??;
        Ok(result.into())
    }

    async fn list_resources(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        Ok(ListResourcesResult {
            resources: vec![Resource::new(LOG, "keevoro.log")
                .with_title("Keevoro's log")
                .with_description(
                    "The plain record of what Keevoro did and when, newest last: every search, backup and \
                     problem, and everything an AI agent did. It never holds keys or passwords.",
                )
                .with_mime_type("text/plain")],
            ..Default::default()
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        if request.uri != LOG {
            return Err(ErrorData::resource_not_found(
                format!("Keevoro has nothing at \"{}\".", request.uri),
                None,
            ));
        }
        let keevoro = self.keevoro.clone();
        let read = tokio::task::spawn_blocking(move || read_for_agent(&keevoro, Op::KeevoroLog))
            .await
            .map_err(|e| ErrorData::internal_error(format!("Keevoro ran into a problem: {e}"), None))?
            .map_err(|e| ErrorData::internal_error(e.message, None))?;
        let text = read.as_str().unwrap_or_default().to_string();
        Ok(ReadResourceResult::new(vec![ResourceContents::text(text, LOG)]).into())
    }
}

/// Serves an AI app on this program's standard input and output until it
/// closes them. Anything that goes wrong is said in Keevoro's log, never on
/// standard output, where it would break the protocol.
pub fn serve_on_stdio(keevoro: Keevoro) -> std::io::Result<()> {
    let log_path = keevoro.log().path();
    // A panic would otherwise be written to standard error, which most AI
    // apps show nowhere. The log is where a person looks.
    std::panic::set_hook(Box::new(move |info| {
        let log = keevoro_api::log::Log::open(log_path.parent().unwrap_or(std::path::Path::new(".")));
        log.say(&format!(
            "Keevoro's agent server ran into a problem it cannot carry on from: {info}"
        ));
    }));
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let (input, output) = rmcp::transport::stdio();
    runtime.block_on(Server::new(keevoro).serve(input, output))
}

#[cfg(test)]
mod tests;
