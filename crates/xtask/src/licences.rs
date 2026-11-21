//! The list of other people's work shipped in Keevoro, with each licence:
//! `cargo xtask licences`.
//!
//! The app shows it under About, "Open source licences". It is made from
//! what the build really uses, so it cannot drift: every crate the app and
//! the `keevoro` command are built from, read from `cargo metadata` (which
//! follows `Cargo.lock`), and every npm package the app's screens are built
//! from, read from `package-lock.json`. Only what is shipped is listed: a
//! crate used only to build or test, or an npm package marked `dev`, is
//! not. The list is checked in as [`DATA`]; `cargo xtask licences --check`
//! and the test at the foot of this file fail when it is out of date.
//!
//! Deciding what is listed is pure ([`rust_crates`], [`npm_packages`],
//! [`render`]), so it is tested with made-up metadata.

use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::process::Command;

/// Where the list is kept, for the screens to show.
pub const DATA: &str = "ui/src/licences.json";

/// The workspace's packages that are shipped: the app, and the `keevoro`
/// command (which is also the server for AI agents).
pub const SHIPPED: &[&str] = &["keevoro-app", "keevoro-cli"];

/// One piece of other people's work.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Entry {
    pub name: String,
    pub version: String,
    /// As its authors declare it, an SPDX expression such as
    /// `MIT OR Apache-2.0`.
    pub licence: String,
    /// Where to find it and its licence.
    pub home: String,
}

/// Every third-party crate the `shipped` packages are built from, found by
/// following their normal dependencies (not those only for building or
/// testing) through `cargo metadata`'s resolved graph.
pub fn rust_crates(metadata: &Value, shipped: &[&str]) -> Result<Vec<Entry>> {
    let packages: HashMap<&str, &Value> = list(metadata, "packages")?
        .iter()
        .filter_map(|p| Some((p.get("id")?.as_str()?, p)))
        .collect();
    let members: HashSet<&str> = list(metadata, "workspace_members")?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let resolve = metadata
        .get("resolve")
        .context("cargo metadata gave no resolved graph")?;
    let nodes: HashMap<&str, &Value> = list(resolve, "nodes")?
        .iter()
        .filter_map(|n| Some((n.get("id")?.as_str()?, n)))
        .collect();
    let mut todo: Vec<&str> = members
        .iter()
        .copied()
        .filter(|id| {
            packages
                .get(id)
                .and_then(|p| p.get("name")?.as_str())
                .is_some_and(|name| shipped.contains(&name))
        })
        .collect();
    if todo.len() != shipped.len() {
        bail!("cargo metadata does not list all of {}", shipped.join(", "));
    }
    let mut seen: HashSet<&str> = HashSet::new();
    while let Some(id) = todo.pop() {
        let Some(node) = nodes.get(id) else { continue };
        for dep in node.get("deps").and_then(Value::as_array).into_iter().flatten() {
            let normal = dep
                .get("dep_kinds")
                .and_then(Value::as_array)
                .is_some_and(|kinds| kinds.iter().any(|k| k.get("kind").is_none_or(Value::is_null)));
            let Some(pkg) = dep.get("pkg").and_then(Value::as_str) else {
                continue;
            };
            if normal && seen.insert(pkg) {
                todo.push(pkg);
            }
        }
    }
    let mut found = BTreeSet::new();
    for id in seen.into_iter().filter(|id| !members.contains(id)) {
        let package = packages
            .get(id)
            .with_context(|| format!("{id} is not among the packages"))?;
        let text = |field: &str| package.get(field).and_then(Value::as_str).unwrap_or_default();
        let licence = match (text("license"), text("license_file")) {
            ("", "") => bail!("{} {} declares no licence", text("name"), text("version")),
            ("", _) => "See its licence file".to_string(),
            (licence, _) => licence.to_string(),
        };
        found.insert(Entry {
            name: text("name").to_string(),
            version: text("version").to_string(),
            licence,
            home: match text("repository") {
                "" => format!("https://crates.io/crates/{}", text("name")),
                repository => repository.to_string(),
            },
        });
    }
    Ok(found.into_iter().collect())
}

/// Every npm package in `package-lock.json` that the app's screens are
/// built from: everything not marked as for development only.
pub fn npm_packages(lock: &Value) -> Result<Vec<Entry>> {
    let packages = lock
        .get("packages")
        .and_then(Value::as_object)
        .context("package-lock.json has no packages (it needs lockfileVersion 2 or later)")?;
    let mut found = BTreeSet::new();
    for (path, package) in packages {
        let yes = |field: &str| package.get(field).and_then(Value::as_bool).unwrap_or(false);
        // The root is Keevoro itself.
        if path.is_empty() || yes("dev") || yes("devOptional") || yes("link") {
            continue;
        }
        let text = |field: &str| package.get(field).and_then(Value::as_str).unwrap_or_default();
        let name = match text("name") {
            "" => path.rsplit("node_modules/").next().unwrap_or(path),
            name => name,
        };
        let licence = match text("license") {
            "" => bail!("{name} {} declares no licence", text("version")),
            licence => licence,
        };
        found.insert(Entry {
            name: name.to_string(),
            version: text("version").to_string(),
            licence: licence.to_string(),
            home: format!("https://www.npmjs.com/package/{name}"),
        });
    }
    Ok(found.into_iter().collect())
}

/// The list as it is kept: JSON, one entry to a line, so a change to one
/// dependency is a change to one line.
pub fn render(rust: &[Entry], npm: &[Entry]) -> String {
    let lines = |entries: &[Entry]| {
        entries
            .iter()
            .map(|e| {
                // Written field by field, so the order never depends on how
                // serde_json happens to be built.
                let quoted = |text: &str| Value::String(text.to_string()).to_string();
                format!(
                    "    {{\"name\": {}, \"version\": {}, \"licence\": {}, \"home\": {}}}",
                    quoted(&e.name),
                    quoted(&e.version),
                    quoted(&e.licence),
                    quoted(&e.home)
                )
            })
            .collect::<Vec<_>>()
            .join(",\n")
    };
    format!(
        "{{\n  \"rust\": [\n{}\n  ],\n  \"npm\": [\n{}\n  ]\n}}\n",
        lines(rust),
        lines(npm)
    )
}

/// The list as it should be now, worked out from the workspace at `root`.
pub fn generate(root: &Path) -> Result<String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let out = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--locked"])
        .current_dir(root)
        .output()
        .context("Couldn't run cargo metadata")?;
    if !out.status.success() {
        bail!("cargo metadata failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    let metadata: Value = serde_json::from_slice(&out.stdout).context("cargo metadata gave something unreadable")?;
    let lock = std::fs::read_to_string(root.join("package-lock.json")).context("Couldn't read package-lock.json")?;
    let lock: Value = serde_json::from_str(&lock).context("package-lock.json is not JSON")?;
    Ok(render(&rust_crates(&metadata, SHIPPED)?, &npm_packages(&lock)?))
}

/// True when the list kept at `root` is the one [`generate`] makes. Line
/// endings are not counted, so a checkout on Windows agrees.
pub fn is_up_to_date(root: &Path) -> Result<bool> {
    let kept = std::fs::read_to_string(root.join(DATA)).unwrap_or_default();
    Ok(kept.replace('\r', "") == generate(root)?)
}

fn list<'v>(value: &'v Value, field: &str) -> Result<&'v Vec<Value>> {
    value
        .get(field)
        .and_then(Value::as_array)
        .with_context(|| format!("cargo metadata gave no {field}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn package(id: &str, licence: &str) -> Value {
        let (name, version) = id.split_once('@').unwrap();
        json!({ "id": id, "name": name, "version": version, "license": licence, "repository": null })
    }

    fn dep(pkg: &str, kind: Option<&str>) -> Value {
        json!({ "pkg": pkg, "dep_kinds": [{ "kind": kind, "target": null }] })
    }

    /// The app depends on a library of ours, which depends on `serde`;
    /// `cc` is only for building, `tempfile` only for testing, and the
    /// xtask, which is not shipped, depends on `regex`.
    fn metadata() -> Value {
        json!({
            "packages": [
                package("keevoro-app@1.0.0", "FSL-1.1-MIT"),
                package("keevoro-cli@1.0.0", "FSL-1.1-MIT"),
                package("keevoro-core@1.0.0", "FSL-1.1-MIT"),
                package("xtask@1.0.0", "FSL-1.1-MIT"),
                package("serde@1.0.1", "MIT OR Apache-2.0"),
                package("serde_derive@1.0.1", "MIT OR Apache-2.0"),
                package("cc@1.2.0", "MIT OR Apache-2.0"),
                package("tempfile@3.0.0", "MIT OR Apache-2.0"),
                package("regex@1.0.0", "MIT OR Apache-2.0"),
                package("webpki-roots@1.0.0", "CDLA-Permissive-2.0"),
            ],
            "workspace_members": ["keevoro-app@1.0.0", "keevoro-cli@1.0.0", "keevoro-core@1.0.0", "xtask@1.0.0"],
            "resolve": { "nodes": [
                { "id": "keevoro-app@1.0.0", "deps": [
                    dep("keevoro-core@1.0.0", None), dep("cc@1.2.0", Some("build")), dep("tempfile@3.0.0", Some("dev")),
                ]},
                { "id": "keevoro-cli@1.0.0", "deps": [dep("keevoro-core@1.0.0", None), dep("webpki-roots@1.0.0", None)] },
                { "id": "keevoro-core@1.0.0", "deps": [dep("serde@1.0.1", None)] },
                { "id": "serde@1.0.1", "deps": [dep("serde_derive@1.0.1", None)] },
                { "id": "serde_derive@1.0.1", "deps": [] },
                { "id": "cc@1.2.0", "deps": [] },
                { "id": "tempfile@3.0.0", "deps": [] },
                { "id": "webpki-roots@1.0.0", "deps": [] },
                { "id": "xtask@1.0.0", "deps": [dep("regex@1.0.0", None)] },
                { "id": "regex@1.0.0", "deps": [] },
            ]}
        })
    }

    #[test]
    fn only_crates_the_shipped_programs_are_built_from_are_listed() {
        let names: Vec<String> = rust_crates(&metadata(), SHIPPED)
            .unwrap()
            .into_iter()
            .map(|e| format!("{} {} {}", e.name, e.version, e.licence))
            .collect();
        assert_eq!(
            names,
            [
                "serde 1.0.1 MIT OR Apache-2.0",
                "serde_derive 1.0.1 MIT OR Apache-2.0",
                "webpki-roots 1.0.0 CDLA-Permissive-2.0",
            ]
        );
        let serde = &rust_crates(&metadata(), SHIPPED).unwrap()[0];
        assert_eq!(serde.home, "https://crates.io/crates/serde");
    }

    #[test]
    fn a_crate_with_no_licence_stops_the_list_being_made() {
        let mut m = metadata();
        m["packages"][4]["license"] = Value::Null;
        let e = rust_crates(&m, SHIPPED).unwrap_err();
        assert_eq!(e.to_string(), "serde 1.0.1 declares no licence");
        m["packages"][4]["license_file"] = json!("LICENCE");
        assert_eq!(rust_crates(&m, SHIPPED).unwrap()[0].licence, "See its licence file");
        let e = rust_crates(&metadata(), &["keevoro-app", "keevoro-nothing"]).unwrap_err();
        assert!(e.to_string().contains("does not list all"), "{e}");
    }

    #[test]
    fn only_npm_packages_not_for_development_are_listed() {
        let lock = json!({ "lockfileVersion": 3, "packages": {
            "": { "name": "keevoro", "license": "FSL-1.1-MIT" },
            "node_modules/react": { "version": "19.0.0", "license": "MIT" },
            "node_modules/@tauri-apps/api": { "version": "2.0.0", "license": "Apache-2.0 OR MIT" },
            "node_modules/vite": { "version": "8.0.0", "license": "MIT", "dev": true },
            "node_modules/fsevents": { "version": "2.0.0", "license": "MIT", "devOptional": true },
            "node_modules/a/node_modules/b": { "version": "1.0.0", "license": "ISC" },
        }});
        let found: Vec<String> = npm_packages(&lock)
            .unwrap()
            .into_iter()
            .map(|e| format!("{} {} {} {}", e.name, e.version, e.licence, e.home))
            .collect();
        assert_eq!(
            found,
            [
                "@tauri-apps/api 2.0.0 Apache-2.0 OR MIT https://www.npmjs.com/package/@tauri-apps/api",
                "b 1.0.0 ISC https://www.npmjs.com/package/b",
                "react 19.0.0 MIT https://www.npmjs.com/package/react",
            ]
        );
        let unsaid = json!({ "packages": { "node_modules/x": { "version": "1.0.0" } } });
        assert_eq!(
            npm_packages(&unsaid).unwrap_err().to_string(),
            "x 1.0.0 declares no licence"
        );
    }

    #[test]
    fn the_list_is_one_entry_to_a_line_and_reads_back_as_json() {
        let one = Entry {
            name: "serde".into(),
            version: "1.0.1".into(),
            licence: "MIT OR Apache-2.0".into(),
            home: "https://serde.rs".into(),
        };
        let text = render(std::slice::from_ref(&one), &[one.clone(), one.clone()]);
        assert_eq!(text.lines().count(), 9, "{text}");
        assert!(
            text.contains(r#"{"name": "serde", "version": "1.0.1", "licence""#),
            "{text}"
        );
        let back: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(back["npm"][1]["licence"], "MIT OR Apache-2.0");
        assert_eq!(back["rust"][0]["home"], "https://serde.rs");
    }

    /// The list kept in the repository is the one the build would make
    /// now. When this fails, run `cargo xtask licences` and commit what
    /// it changes.
    #[test]
    fn the_list_of_licences_kept_is_up_to_date() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        assert!(
            is_up_to_date(&root).unwrap(),
            "{DATA} is out of date: run `cargo xtask licences` and commit it"
        );
    }
}
