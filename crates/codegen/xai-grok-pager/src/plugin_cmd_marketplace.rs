//! Marketplace CLI handlers (feature `marketplace` only).
use super::*;
use xai_grok_plugin_marketplace::SourceKind;

pub(super) async fn run_marketplace(cmd: MarketplaceCommand) -> Result<()> {
    // Policy-filtered: list/update must not touch blocked marketplaces.
    let sources = xai_grok_shell::plugin::load_filtered_marketplace_sources();

    match cmd {
        MarketplaceCommand::List { json } => marketplace_list(&sources, json),
        MarketplaceCommand::Add { url, force } => marketplace_add(&url, force),
        // Remove is cleanup, not bypass: it must find blocked sources too.
        MarketplaceCommand::Remove { source } => {
            marketplace_remove(&xai_grok_shell::plugin::load_marketplace_sources(), &source)
        }
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
            println!("  {}: {}", s.name, s.identity());
        }
    }
    Ok(())
}

fn marketplace_add(url: &str, force: bool) -> Result<()> {
    use xai_grok_shell::plugin::MarketplaceAddInput;

    let url = url.trim();
    if url.is_empty() {
        bail!("URL cannot be empty.");
    }

    let cwd = std::env::current_dir().unwrap_or_default();
    let input = plugin::classify_marketplace_add_input(url, &cwd);

    // Fail fast on missing local paths: otherwise a path input is stored as a git URL and only errors after network clone attempts
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

    let allowlist =
        &xai_grok_workspace::permission::resolution::managed_settings().marketplace_allowlist;
    if let Some(reason) = allowlist.add_block_reason(&identity) {
        bail!("Marketplace source blocked: {reason}");
    }

    // Dedupe against the FULL unfiltered source list by canonical git-URL identity, mirroring the
    // shell modal twin; the locked add core below re-checks under the flock.
    let existing = xai_grok_shell::plugin::load_marketplace_sources();
    let already_configured = match &input {
        MarketplaceAddInput::GitUrl(git_url) => {
            use xai_grok_workspace::permission::resolution::normalize_git_url;
            let normalized = normalize_git_url(git_url);
            existing.iter().any(|s| {
                matches!(&s.kind, SourceKind::Git { url: u, .. }
                    if normalize_git_url(u) == normalized)
            })
        }
        MarketplaceAddInput::LocalPath(path) => existing
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

    let is_official = matches!(&input, MarketplaceAddInput::GitUrl(u)
        if xai_grok_plugin_marketplace::is_official_source_url(u));
    let name = if is_official {
        xai_grok_plugin_marketplace::OFFICIAL_SOURCE_NAME.to_string()
    } else {
        match &input {
            MarketplaceAddInput::GitUrl(u) => plugin::name_from_url(u),
            MarketplaceAddInput::LocalPath(p) => plugin::name_from_path(p),
        }
    };

    // Shared locked add core (same as the shell modal): init flock across the
    // read-modify-write, idempotent normalized dedup, atomic replace.
    let grok_home = xai_grok_config::grok_home();
    let _flock = xai_grok_shell::util::config::acquire_init_lock(&grok_home)?;
    plugin::add_marketplace_source(
        &grok_home.join(xai_grok_config::USER_CONFIG_FILENAME),
        &name,
        &input,
        is_official,
    )?;

    println!("Added marketplace source: {name} ({identity})");
    Ok(())
}

/// Resolve `remove` input to a source: exact name match first, then the same URL or path matching `marketplace add` uses.
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
                .map(|s| s.identity())
                .collect();
            return Err(format!(
                "Multiple sources are named \"{input}\"; remove by URL/path instead: {}",
                identities.join(", ")
            ));
        }
        return Ok(first);
    }

    let expanded = plugin::expand_github_shorthand(input);
    // Full git-URL normalization (`.git`, host case, scp-vs-https spelling),
    // same matching `marketplace add` dedupes with.
    use xai_grok_workspace::permission::resolution::normalize_git_url;
    let norm = normalize_git_url(input);
    let exp_norm = normalize_git_url(&expanded);
    // Loaded local sources carry expanded paths, so expand `~` and relative inputs the same way `marketplace add` does before comparing
    let local_input = match plugin::classify_marketplace_add_input(input, cwd) {
        xai_grok_shell::plugin::MarketplaceAddInput::LocalPath(p) => Some(p),
        _ => None,
    };

    sources
        .iter()
        .find(|s| match &s.kind {
            SourceKind::Git { url: u, .. } => {
                let un = normalize_git_url(u);
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

    let identity = source.identity();

    // Uninstall + config rewrite under the init flock with atomic replace, mirroring the shell
    // modal twin (`remove_source_locked`); an unlocked remove is the lost-update race.
    let grok_home = xai_grok_config::grok_home();
    let _flock = xai_grok_shell::util::config::acquire_init_lock(&grok_home)?;

    let uninstalled = plugin::uninstall_marketplace_source_plugins(&identity)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    // Shared remove-write core (same as the shell modal): config.toml with the
    // official flag folded in, JSON stores as fallback.
    let config_path = grok_home.join(xai_grok_config::USER_CONFIG_FILENAME);
    if plugin::remove_marketplace_source_from_stores(&config_path, &identity)?
        == plugin::MarketplaceSourceRemoval::NotFound
    {
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

