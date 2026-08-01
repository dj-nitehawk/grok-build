//! Config-driven promotion of selected MCP tools into the sampling tool list.
//!
//! By default MCP tools stay behind `search_tool` / `use_tool` so the model
//! tool set stays stable. Servers can opt tools in via
//! `[mcp_servers.<name>] promote_tools = [...]`.

use std::collections::HashSet;
use std::path::Path;

use xai_grok_config_types::collect_promoted_mcp_tool_names;
use xai_grok_tools::types::definition::ToolDefinition;

/// Fully-qualified MCP tool names promoted for `cwd` (user + project config).
///
/// Empty when no server lists `promote_tools` (the default). Re-reads config
/// each call so toolset rebuilds / config edits apply on the next prepare.
pub fn promoted_tool_names_for_cwd(cwd: &Path) -> HashSet<String> {
    let configs = crate::util::config::load_mcp_server_configs_with_project(cwd);
    collect_promoted_mcp_tool_names(
        configs
            .iter()
            .map(|(name, (cfg, _))| (name.as_str(), cfg)),
    )
}

/// Append registered MCP tool definitions that match `promoted_qualified`.
///
/// No-op when `promoted_qualified` is empty. Skips names already present in
/// `defs` (e.g. if a prior path already included them).
pub fn append_promoted_mcp_definitions(
    all_registered: Vec<ToolDefinition>,
    promoted_qualified: &HashSet<String>,
    defs: &mut Vec<ToolDefinition>,
) {
    if promoted_qualified.is_empty() {
        return;
    }
    let mut present: HashSet<String> = defs.iter().map(|d| d.function.name.clone()).collect();
    for def in all_registered {
        let name = def.function.name.as_str();
        if name.contains("__")
            && promoted_qualified.contains(name)
            && !present.contains(name)
        {
            present.insert(def.function.name.clone());
            defs.push(def);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xai_grok_tools::types::definition::ToolDefinition;

    fn def(name: &str) -> ToolDefinition {
        ToolDefinition::function(
            name,
            Some(format!("desc for {name}")),
            serde_json::json!({"type": "object"}),
        )
    }

    #[test]
    fn append_promoted_is_noop_when_empty() {
        let mut defs = vec![def("read_file")];
        append_promoted_mcp_definitions(
            vec![def("linear__save_issue")],
            &HashSet::new(),
            &mut defs,
        );
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].function.name, "read_file");
    }

    #[test]
    fn append_promoted_adds_listed_mcp_only() {
        let mut defs = vec![def("read_file"), def("grep")];
        let mut promote = HashSet::new();
        promote.insert("linear__save_issue".to_string());
        append_promoted_mcp_definitions(
            vec![
                def("read_file"),
                def("grep"),
                def("linear__save_issue"),
                def("linear__list_issues"),
                def("github__create_issue"),
            ],
            &promote,
            &mut defs,
        );
        let names: Vec<_> = defs.iter().map(|d| d.function.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["read_file", "grep", "linear__save_issue"]
        );
    }

    #[test]
    fn append_promoted_skips_duplicates() {
        let mut defs = vec![def("read_file"), def("linear__save_issue")];
        let mut promote = HashSet::new();
        promote.insert("linear__save_issue".to_string());
        append_promoted_mcp_definitions(
            vec![def("linear__save_issue")],
            &promote,
            &mut defs,
        );
        assert_eq!(defs.len(), 2);
    }
}
