//! The tools an AI agent sees, made from the API's register.
//!
//! Nothing about a tool is written here: its name, what it tells the
//! agent, what it takes and whether it changes anything all come from the
//! register, so the app, the command line and an agent cannot drift
//! apart. This only puts them in the shape the protocol asks for.

use keevoro_api::register::NAMING_A_BACKUP;
use keevoro_api::{Agents, Changes, Op};
use rmcp::model::{JsonObject, Tool, ToolAnnotations};
use serde_json::{json, Value};
use std::sync::Arc;

/// Every tool, in the register's order.
pub fn all() -> Vec<Tool> {
    Op::ALL.iter().copied().filter_map(tool_for).collect()
}

/// The operation behind a tool, by the tool's name.
pub fn op_named(name: &str) -> Option<Op> {
    Op::ALL
        .iter()
        .copied()
        .find(|op| op.tool().is_some_and(|tool| tool.name == name))
}

/// The tool for one operation, if the register offers it to agents.
pub fn tool_for(op: Op) -> Option<Tool> {
    let tool = op.tool()?;
    let entry = op.entry();
    // An operation offered only to look takes nothing from an agent: what
    // it could be asked to change is for the person.
    let look_only = entry.agents == Agents::LookOnly;
    let mut schema = if look_only {
        json!({ "type": "object", "properties": {}, "additionalProperties": false })
    } else {
        op.request_schema()
    };
    // Every tool lists what it takes, if only to say it takes nothing:
    // some AI apps expect to find the list.
    if let Some(object) = schema.as_object_mut() {
        let properties = object.entry("properties").or_insert_with(|| json!({}));
        if let (true, Some(properties)) = (op.on_a_backup(), properties.as_object_mut()) {
            properties.insert(
                "backup".into(),
                json!({ "type": "string", "description": NAMING_A_BACKUP }),
            );
        }
    }
    in_tools_words(&mut schema);
    let schema: JsonObject = match schema {
        Value::Object(object) => object,
        _ => JsonObject::new(),
    };
    let read_only = look_only || entry.changes == Changes::Never;
    let mut annotations = ToolAnnotations::new()
        .read_only(read_only)
        // Only cloud storage, and GitHub for updates, are outside this
        // computer.
        .open_world(matches!(
            op,
            Op::DestinationsAdd | Op::BackupRun | Op::BackupCheck | Op::KeevoroUpdate
        ));
    if !read_only {
        annotations = annotations.destructive(op.loses_something());
    }
    Some(
        Tool::new(tool.name, tool.tells, Arc::new(schema))
            .with_title(title(tool.name))
            .with_annotations(annotations),
    )
}

/// A tool's name as a person reads it: `run_backup` is "Run backup".
fn title(name: &str) -> String {
    let words = name.replace('_', " ").replace("keevoro", "Keevoro");
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => words,
    }
}

/// A request's descriptions name other operations as the API knows them,
/// such as `sources.list`; an agent knows them as tools.
fn in_tools_words(value: &mut Value) {
    match value {
        Value::Object(object) => {
            for (key, inner) in object.iter_mut() {
                match inner {
                    Value::String(text) if key == "description" => {
                        for op in Op::ALL {
                            if let Some(tool) = op.tool() {
                                *text = text.replace(&format!("`{}`", op.name()), tool.name);
                            }
                        }
                    }
                    _ => in_tools_words(inner),
                }
            }
        }
        Value::Array(all) => all.iter_mut().for_each(in_tools_words),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_operation_offered_to_agents_is_a_tool_and_nothing_else_is() {
        let tools = all();
        let named: HashSet<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
        assert_eq!(named.len(), tools.len(), "no two tools share a name");
        for entry in keevoro_api::entries() {
            let offered = matches!(entry.agents, Agents::Yes | Agents::LookOnly);
            let tool = entry.op.tool().map(|t| t.name);
            assert_eq!(
                tool.is_some_and(|name| named.contains(name)),
                offered,
                "{} is offered to agents as {:?}",
                entry.name,
                entry.agents
            );
            if let Some(name) = tool {
                assert_eq!(op_named(name), Some(entry.op));
            }
        }
        // Nothing kept for the person has a tool by any name.
        for kept in [
            "forget_backup",
            "reset",
            "reset_keevoro",
            "follow",
            "set_access",
            "install_update",
            "add_file_kind",
            "change_file_kind",
        ] {
            assert!(op_named(kept).is_none(), "{kept}");
        }
        assert_eq!(tools.len(), 44);
        assert_eq!(title("run_backup"), "Run backup");
        assert_eq!(title("about_keevoro"), "About Keevoro");
    }

    #[test]
    fn a_tool_says_whether_it_only_looks_and_never_that_it_destroys_a_photo() {
        for tool in all() {
            let op = op_named(&tool.name).unwrap();
            let hints = tool.annotations.clone().unwrap();
            let looks = op.entry().changes == Changes::Never || op.entry().agents == Agents::LookOnly;
            assert_eq!(hints.read_only_hint, Some(looks), "{}", tool.name);
            if looks {
                assert_eq!(hints.destructive_hint, None, "{}", tool.name);
            } else {
                assert_eq!(hints.destructive_hint, Some(op.loses_something()), "{}", tool.name);
            }
            assert_eq!(tool.description.as_deref(), op.tool().map(|t| t.tells));
        }
        let destructive: Vec<String> = all()
            .into_iter()
            .filter(|t| t.annotations.as_ref().and_then(|a| a.destructive_hint) == Some(true))
            .map(|t| t.name.to_string())
            .collect();
        assert_eq!(destructive, ["forget_source", "remove_destination"]);
    }

    #[test]
    fn what_a_tool_takes_is_the_request_with_the_backup_it_works_on() {
        let tool = |name: &str| tool_for(op_named(name).unwrap()).unwrap();
        let look = tool("look_through");
        let schema = Value::Object((*look.input_schema).clone());
        assert_eq!(schema["required"], json!(["place"]));
        assert_eq!(schema["properties"]["backup"]["type"], "string");
        assert_eq!(schema["additionalProperties"], false);

        let list = tool("list_backups");
        assert_eq!(list.input_schema["properties"], json!({}), "not about one backup");
        // Every tool about one backup can name it, even one that takes
        // nothing else.
        for tool in all() {
            let op = op_named(&tool.name).unwrap();
            let named = tool.input_schema["properties"].get("backup").is_some();
            // Renaming takes the backup to rename in its own request.
            let in_its_request = op.request_schema()["properties"].get("backup").is_some();
            assert_eq!(named, op.on_a_backup() || in_its_request, "{}", tool.name);
        }

        // A look-only tool takes nothing, so an agent cannot ask it to
        // change anything.
        for name in ["my_access", "read_settings", "check_for_update"] {
            let schema = tool(name).input_schema;
            assert_eq!(schema["properties"], json!({}), "{name}");
            assert_eq!(schema["additionalProperties"], false, "{name}");
        }

        // Other operations are named as the agent knows them.
        let text = Value::Object((*tool("forget_source").input_schema).clone()).to_string();
        assert!(text.contains("as list_sources gives it"), "{text}");
        for op in Op::ALL {
            assert!(
                !all().iter().any(|t| Value::Object((*t.input_schema).clone())
                    .to_string()
                    .contains(&format!("`{}`", op.name()))
                    && op.tool().is_some()),
                "{} is named in code words",
                op.name()
            );
        }
    }
}
