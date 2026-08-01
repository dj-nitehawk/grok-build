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
    // Untrusted project `.grok/config.toml` must not opt user-scoped MCP tools
    // into the model tool list. Startup already refuses to connect those servers.
    let allow_project = crate::agent::folder_trust::project_scope_allowed(cwd);
    promoted_names_from_scoped(
        configs
            .iter()
            .map(|(name, (cfg, scope))| (name.as_str(), cfg, *scope)),
        allow_project,
    )
}

fn promoted_names_from_scoped<'a>(
    servers: impl IntoIterator<Item = (&'a str, &'a xai_grok_config_types::McpServerConfig, &'a str)>,
    allow_project: bool,
) -> HashSet<String> {
    collect_promoted_mcp_tool_names(servers.into_iter().filter_map(|(name, cfg, scope)| {
        if scope == crate::util::config::MCP_SCOPE_PROJECT && !allow_project {
            None
        } else {
            Some((name, cfg))
        }
    }))
}

/// Append registered MCP tool definitions that match `promoted_qualified`.
///
/// No-op when `promoted_qualified` is empty. Skips names already present in
/// `defs` (e.g. if a prior path already included them).
///
/// Catalog keys may be longer than a provider function name (up to 256). Those
/// stay on `search_tool` / `use_tool`: sending one as a native tool 400s the
/// whole sampling request.
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
        if !(name.contains("__") && promoted_qualified.contains(name) && !present.contains(name)) {
            continue;
        }
        if let Err(reason) = xai_grok_mcp::servers::validate_tool_name(name) {
            tracing::warn!(
                tool = %name,
                reason = %reason,
                "skipping MCP promotion; tool stays on search_tool/use_tool"
            );
            continue;
        }
        present.insert(def.function.name.clone());
        defs.push(def);
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
        assert_eq!(names, vec!["read_file", "grep", "linear__save_issue"]);
    }

    fn server(promote: &[&str]) -> xai_grok_config_types::McpServerConfig {
        serde_json::from_value(serde_json::json!({
            "command": "true",
            "promote_tools": promote,
        }))
        .expect("mcp server config")
    }

    #[test]
    fn untrusted_project_cannot_promote_another_servers_tools() {
        let user = server(&["list_issues"]);
        let project = server(&["github__create_issue"]);
        let names = promoted_names_from_scoped(
            [
                ("github", &user, "user"),
                ("dummy", &project, crate::util::config::MCP_SCOPE_PROJECT),
            ],
            false,
        );
        assert!(names.contains("github__list_issues"));
        assert!(
            !names.contains("github__create_issue"),
            "untrusted project promote list must not surface a user MCP tool"
        );
    }

    #[test]
    fn trusted_project_promote_list_is_kept() {
        let project = server(&["create_issue"]);
        let names = promoted_names_from_scoped(
            [("github", &project, crate::util::config::MCP_SCOPE_PROJECT)],
            true,
        );
        assert!(names.contains("github__create_issue"));
    }

    #[test]
    fn append_promoted_skips_duplicates() {
        let mut defs = vec![def("read_file"), def("linear__save_issue")];
        let mut promote = HashSet::new();
        promote.insert("linear__save_issue".to_string());
        append_promoted_mcp_definitions(vec![def("linear__save_issue")], &promote, &mut defs);
        assert_eq!(defs.len(), 2);
    }

    #[test]
    fn append_promoted_skips_names_the_provider_rejects() {
        let over = format!("linear__{}", "a".repeat(64));
        assert!(over.len() > xai_grok_mcp::servers::PROVIDER_TOOL_NAME_MAX_CHARS);
        assert!(xai_grok_mcp::servers::validate_tool_name(&over).is_err());

        let ok_len = xai_grok_mcp::servers::PROVIDER_TOOL_NAME_MAX_CHARS;
        let ok = format!("lin__{}", "a".repeat(ok_len - "lin__".len()));
        assert_eq!(ok.len(), ok_len);
        assert!(xai_grok_mcp::servers::validate_tool_name(&ok).is_ok());

        let mut defs = vec![def("read_file")];
        let promote = HashSet::from([over.clone(), ok.clone()]);
        append_promoted_mcp_definitions(vec![def(&over), def(&ok)], &promote, &mut defs);
        let names: Vec<_> = defs.iter().map(|d| d.function.name.as_str()).collect();
        assert_eq!(names, vec!["read_file", ok.as_str()]);
    }
}
