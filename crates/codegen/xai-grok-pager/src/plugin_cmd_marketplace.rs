//! Marketplace CLI handlers (feature `marketplace` only).
use super::*;
use xai_grok_plugin_marketplace::SourceKind;

pub(super) async fn run_marketplace(cmd: MarketplaceCommand) -> Result<()> {
    let config = xai_grok_shell::config::load_effective_config()
        .ok()
        .unwrap_or(toml::Value::Table(toml::map::Map::new()));
    let mut sources = xai_grok_plugin_marketplace::load_sources(&config);
    sources.extend(xai_grok_plugin_marketplace::load_extra_sources_from_settings(&sources));

    match cmd {
        MarketplaceCommand::List { json } => marketplace_list(&sources, json),
        MarketplaceCommand::Add { url, force } => marketplace_add(&sources, &url, force),
        MarketplaceCommand::Remove { source } => marketplace_remove(&sources, &source),
        MarketplaceCommand::Update { name } => marketplace_update(&sources, name.as_deref()),
    }
}

fn marketplace_list(
    sources: &[xai_grok_plugin_marketplace::MarketplaceSource],
    json: bool,
) -> Result<()> {
    if json {
        let entries: Vec<MarketplaceSourceEntry> = sources
            .iter()
            .map(|s| {
                let detail = match &s.kind {
                    SourceKind::Git { url, branch } => MarketplaceSourceDetail::Git {
                        url: url.clone(),
                        branch: branch.clone(),
                    },
                    SourceKind::Local { path } => {
                        MarketplaceSourceDetail::Local { path: path.clone() }
                    }
                };
                MarketplaceSourceEntry {
                    name: s.name.clone(),
                    kind: detail.kind().to_string(),
                    source: detail,
                }
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else if sources.is_empty() {
        println!(
            "No marketplace sources configured.\n\
             Run `grok plugin marketplace add --help` to get started."
        );
    } else {
        for s in sources {
            let id = match &s.kind {
                SourceKind::Git { url, .. } => url.clone(),
                SourceKind::Local { path } => path.display().to_string(),
            };
            println!("  {}: {id}", s.name);
        }
    }
    Ok(())
}

fn marketplace_add(
    sources: &[xai_grok_plugin_marketplace::MarketplaceSource],
    url: &str,
    force: bool,
) -> Result<()> {
    use xai_grok_shell::plugin::MarketplaceAddInput;

    let url = url.trim();
    if url.is_empty() {
        bail!("URL cannot be empty.");
    }

    let cwd = std::env::current_dir().unwrap_or_default();
    let input = plugin::classify_marketplace_add_input(url, &cwd);

    // Fail fast on missing local paths: without this, a path input would be
    // stored as a git URL and only error after network clone attempts.
    if let MarketplaceAddInput::LocalPath(path) = &input
        && !path.is_dir()
    {
        bail!(
            "Local marketplace path not found (or is not a directory): {}",
            path.display()
        );
    }

    let identity = match &input {
        MarketplaceAddInput::GitUrl(u) => u.clone(),
        MarketplaceAddInput::LocalPath(p) => p.display().to_string(),
    };

    // Local paths never match the git-URL allowlist, so a restricted
    // strictKnownMarketplaces policy blocks them — intentionally fail-closed.
    let allowlist =
        &xai_grok_workspace::permission::resolution::managed_settings().marketplace_allowlist;
    if allowlist.is_restricted() && !allowlist.is_url_allowed(&identity) {
        bail!("Marketplace source blocked: {}", allowlist.block_reason());
    }

    let already_configured = match &input {
        MarketplaceAddInput::GitUrl(git_url) => {
            let normalized = git_url.trim_end_matches(".git");
            sources.iter().any(|s| {
                matches!(&s.kind, SourceKind::Git { url: u, .. }
                    if u.trim_end_matches(".git") == normalized)
            })
        }
        MarketplaceAddInput::LocalPath(path) => sources
            .iter()
            .any(|s| matches!(&s.kind, SourceKind::Local { path: p } if p == path)),
    };
    if already_configured {
        bail!("Marketplace source already configured: {identity}");
    }

    if !force && let MarketplaceAddInput::GitUrl(git_url) = &input {
        xai_grok_plugin_marketplace::git::probe_git_remote(git_url).map_err(|e| {
            anyhow::anyhow!(
                "{e}\nNot adding \"{url}\": it doesn't look like a reachable git repository. \
                 Re-run with --force to add it anyway (e.g. a host only reachable on VPN)."
            )
        })?;
    }

    let name = match &input {
        MarketplaceAddInput::GitUrl(u) => plugin::name_from_url(u),
        MarketplaceAddInput::LocalPath(p) => plugin::name_from_path(p),
    };
    let config_path = xai_grok_config::grok_home().join("config.toml");

    let content = std::fs::read_to_string(&config_path).unwrap_or_default();
    let mut doc: toml_edit::DocumentMut = content
        .parse()
        .map_err(|e| anyhow::anyhow!("Failed to parse config.toml: {e}"))?;

    if doc.get("marketplace").is_none() {
        doc["marketplace"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    if doc["marketplace"].get("sources").is_none() {
        doc["marketplace"]["sources"] =
            toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new());
    }

    let sources = doc["marketplace"]["sources"]
        .as_array_of_tables_mut()
        .ok_or_else(|| anyhow::anyhow!("marketplace.sources is not an array of tables"))?;

    let mut entry = toml_edit::Table::new();
    entry["name"] = toml_edit::value(&name);
    match &input {
        MarketplaceAddInput::GitUrl(git_url) => {
            entry["git"] = toml_edit::value(git_url);
        }
        MarketplaceAddInput::LocalPath(path) => {
            entry["path"] = toml_edit::value(path.display().to_string());
        }
    }
    sources.push(entry);

    std::fs::write(&config_path, doc.to_string())?;

    println!("Added marketplace source: {name} ({identity})");
    Ok(())
}

/// Resolve `remove` input to a source: exact name match first, then the same
/// URL/path matching `marketplace add` uses.
fn find_removal_source<'a>(
    sources: &'a [xai_grok_plugin_marketplace::MarketplaceSource],
    input: &str,
    cwd: &Path,
) -> Result<&'a xai_grok_plugin_marketplace::MarketplaceSource, String> {
    let mut by_name = sources.iter().filter(|s| s.name == input);
    if let Some(first) = by_name.next() {
        if by_name.next().is_some() {
            let identities: Vec<String> = sources
                .iter()
                .filter(|s| s.name == input)
                .map(source_identity)
                .collect();
            return Err(format!(
                "Multiple sources are named \"{input}\"; remove by URL/path instead: {}",
                identities.join(", ")
            ));
        }
        return Ok(first);
    }

    let expanded = plugin::normalize_git_url(input);
    let norm = input.trim_end_matches(".git");
    let exp_norm = expanded.trim_end_matches(".git");
    // Loaded local sources carry expanded paths, so expand `~`/relative inputs
    // the same way `marketplace add` does before comparing.
    let local_input = match plugin::classify_marketplace_add_input(input, cwd) {
        xai_grok_shell::plugin::MarketplaceAddInput::LocalPath(p) => Some(p),
        _ => None,
    };

    sources
        .iter()
        .find(|s| match &s.kind {
            SourceKind::Git { url: u, .. } => {
                let un = u.trim_end_matches(".git");
                un == norm || un == exp_norm
            }
            SourceKind::Local { path } => {
                path.display().to_string() == input
                    || local_input.as_ref().is_some_and(|p| p == path)
            }
        })
        .ok_or_else(|| {
            let names: Vec<&str> = sources.iter().map(|s| s.name.as_str()).collect();
            if names.is_empty() {
                format!("Marketplace source \"{input}\" not found; no sources are configured.")
            } else {
                format!(
                    "Marketplace source \"{input}\" not found. Configured sources: {}",
                    names.join(", ")
                )
            }
        })
}

fn marketplace_remove(
    sources: &[xai_grok_plugin_marketplace::MarketplaceSource],
    name_or_url: &str,
) -> Result<()> {
    let input = name_or_url.trim();
    if input.is_empty() {
        bail!("Provide the source name, git URL, or local path to remove.");
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    let source = find_removal_source(sources, input, &cwd).map_err(|e| anyhow::anyhow!("{e}"))?;

    let identity = source_identity(source);

    let uninstalled = plugin::uninstall_marketplace_source_plugins(&identity);

    let config_path = xai_grok_config::grok_home().join("config.toml");
    let mut removed_from_config = false;
    if let Ok(content) = std::fs::read_to_string(&config_path)
        && let Some(new) = plugin::remove_toml_marketplace_block(&content, &identity)
    {
        if let Err(e) = std::fs::write(&config_path, new) {
            tracing::warn!("failed to write config.toml: {e}");
        } else {
            removed_from_config = true;
        }
    }

    // Fallback: settings.json / known_marketplaces.json.
    if !removed_from_config && !plugin::try_remove_source_from_json_files(&identity) {
        eprintln!(
            "Warning: source was found but could not be removed from config files.\n\
             It may be defined in a managed or read-only settings file."
        );
    }

    if uninstalled.is_empty() {
        println!("Removed marketplace source: {} ({identity})", source.name);
    } else {
        println!(
            "Removed marketplace source and uninstalled {} plugin(s): {}",
            uninstalled.len(),
            uninstalled.join(", "),
        );
    }
    Ok(())
}

fn marketplace_update(
    sources: &[xai_grok_plugin_marketplace::MarketplaceSource],
    name: Option<&str>,
) -> Result<()> {
    marketplace_update_with_cache_root(
        sources,
        name,
        &xai_grok_plugin_marketplace::git::default_cache_root(),
    )
}

fn marketplace_update_with_cache_root(
    sources: &[xai_grok_plugin_marketplace::MarketplaceSource],
    name: Option<&str>,
    cache_root: &Path,
) -> Result<()> {
    let mut refreshed = 0;
    let mut errors = Vec::new();
    let mut name_matched = false;

    for source in sources {
        if let Some(filter) = name {
            if source.name != filter {
                continue;
            }
            name_matched = true;
        }
        if let SourceKind::Git { url, branch } = &source.kind {
            match xai_grok_plugin_marketplace::git::force_sync_source_cache(
                url,
                branch.as_deref(),
                cache_root,
            ) {
                Ok(_) => {
                    println!("  {}: synced", source.name);
                    refreshed += 1;
                }
                Err(e) => errors.push(format!("{}: {e}", source.name)),
            }
        }
    }

    if refreshed == 0 && errors.is_empty() {
        if let Some(filter) = name {
            if name_matched {
                // Source exists but is local, nothing to sync.
                println!("Source \"{filter}\" is local, nothing to sync.");
            } else {
                bail!("Marketplace source \"{filter}\" not found.");
            }
        } else {
            println!("No marketplace sources configured.");
        }
    } else if errors.is_empty() {
        println!("Refreshed {refreshed} source(s).");
    } else {
        eprintln!(
            "Refreshed {refreshed} source(s) with {} error(s): {}",
            errors.len(),
            errors.join("; "),
        );
    }
    Ok(())
}
