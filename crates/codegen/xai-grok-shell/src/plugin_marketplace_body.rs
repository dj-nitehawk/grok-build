//! Marketplace-specific plugin helpers (feature `marketplace` only).
use super::*;
use xai_grok_plugin_marketplace::git::{self, SourceCacheLease};
use xai_grok_plugin_marketplace::{
    MarketplaceEntry, MarketplaceRelativePath, MarketplaceSource, SourceKind, install_resolve,
    installer, is_official_source_url, load_extra_sources_from_settings, load_sources,
    scan_marketplace,
};


struct MarketplaceSourceRoot {
    path: PathBuf,
    _lease: Option<SourceCacheLease>,
}

fn update_marketplace_repo(
    registry: &mut InstallRegistry,
    repo: &InstalledRepo,
    source_cache: &mut std::collections::HashMap<String, MarketplaceSourceRoot>,
) -> Result<installer::MarketplaceUpdateResult, InstallError> {
    let provenance = repo
        .marketplace
        .clone()
        .ok_or_else(|| InstallError::InstallFailed {
            detail: "installed repo is missing marketplace provenance".into(),
        })?;
    let entry_path = MarketplaceRelativePath::parse(&provenance.plugin_subdir).map_err(|e| {
        InstallError::InstallFailed {
            detail: format!("invalid marketplace plugin path: {e}"),
        }
    })?;

    let cache_key = provenance.source_url_or_path.clone();
    if !source_cache.contains_key(&cache_key) {
        source_cache.insert(
            cache_key.clone(),
            marketplace_root_for_provenance(&provenance)?,
        );
    }
    let marketplace_root = source_cache
        .get(&cache_key)
        .unwrap_or_else(|| unreachable!());
    let scan = scan_marketplace(&marketplace_root.path);
    let entry = scan
        .entries
        .into_iter()
        .find(|entry| entry.relative_path == entry_path.as_str())
        .ok_or_else(|| InstallError::PluginNotFound {
            name: provenance.plugin_subdir.clone(),
        })?;

    let require_sha = crate::plugin::marketplace_require_sha();
    installer::update_from_marketplace_entry_transactional(
        &marketplace_root.path,
        &entry,
        provenance,
        registry,
        require_sha,
    )
}

fn marketplace_root_for_provenance(
    provenance: &xai_grok_agent::plugins::install_registry::MarketplaceProvenance,
) -> Result<MarketplaceSourceRoot, InstallError> {
    let source = &provenance.source_url_or_path;
    if let Some((url, branch)) = configured_marketplace_git_source(source) {
        let cache_root = git::default_cache_root();
        let lease = git::sync_source_cache_with_mode(
            &url,
            branch.as_deref(),
            &cache_root,
            git::SyncMode::Force,
        )
        .map_err(|e| InstallError::InstallFailed {
            detail: format!("Git sync failed: {e}"),
        })?;
        return Ok(MarketplaceSourceRoot {
            path: lease.path.clone(),
            _lease: Some(lease),
        });
    }

    if source.contains("://") || source.contains("git@") {
        let cache_root = git::default_cache_root();
        let lease =
            git::sync_source_cache_with_mode(source, None, &cache_root, git::SyncMode::Force)
                .map_err(|e| InstallError::InstallFailed {
                    detail: format!("Git sync failed: {e}"),
                })?;
        Ok(MarketplaceSourceRoot {
            path: lease.path.clone(),
            _lease: Some(lease),
        })
    } else {
        Ok(MarketplaceSourceRoot {
            path: PathBuf::from(source),
            _lease: None,
        })
    }
}

fn configured_marketplace_git_source(source_url_or_path: &str) -> Option<(String, Option<String>)> {
    load_marketplace_sources()
        .into_iter()
        .find_map(|source| match source.kind {
            SourceKind::Git { url, branch } if url == source_url_or_path => Some((url, branch)),
            _ => None,
        })
}

/// Update one or all installed plugins. Saves the registry once at the end.
pub fn update_plugins(name: Option<&str>) -> Result<Vec<RepoUpdateOutcome>, UpdateError> {
    update_plugins_by_selector(name.map(|name| PluginUpdateSelector::PluginName(name.to_string())))
}

pub(crate) fn update_plugins_by_selector(
    selector: Option<PluginUpdateSelector>,
) -> Result<Vec<RepoUpdateOutcome>, UpdateError> {
    let mut registry = InstallRegistry::load();
    let repos_to_update: Vec<(String, InstalledRepo)> = match selector {
        Some(PluginUpdateSelector::PluginName(plugin_name)) => {
            match registry.find_plugin(&plugin_name) {
                Some((key, repo, _)) => vec![(key.to_string(), repo.clone())],
                None => {
                    return Err(UpdateError::NotFound {
                        name: plugin_name.to_string(),
                    });
                }
            }
        }
        Some(PluginUpdateSelector::RepoKey(repo_key)) => match registry.get_repo(&repo_key) {
            Some(repo) => vec![(repo_key.to_string(), repo.clone())],
            None => {
                return Err(UpdateError::NotFound {
                    name: repo_key.to_string(),
                });
            }
        },
        None => registry
            .list()
            .into_iter()
            .map(|(k, r)| (k.to_string(), r.clone()))
            .collect(),
    };

    let mut outcomes = Vec::with_capacity(repos_to_update.len());
    let mut source_cache = std::collections::HashMap::new();

    for (repo_key, repo) in &repos_to_update {
        let outcome = if repo.marketplace.is_some() {
            match update_marketplace_repo(&mut registry, repo, &mut source_cache) {
                Ok(result) => {
                    if result.changed || result.reinstalled {
                        RepoUpdateOutcome::Updated {
                            repo_key: result.repo_key,
                            old_commit: result.old_version,
                            new_commit: result.new_version,
                        }
                    } else {
                        RepoUpdateOutcome::AlreadyUpToDate {
                            repo_key: result.repo_key,
                        }
                    }
                }
                Err(e) => RepoUpdateOutcome::Failed {
                    repo_key: repo_key.clone(),
                    error: e.to_string(),
                },
            }
        } else {
            match git_install::update_repo(repo_key, repo, marketplace_require_sha()) {
                Ok(UpdateStatus::Updated(result)) if result.changed => {
                    apply_update_to_registry(&mut registry, repo_key, &result);
                    RepoUpdateOutcome::Updated {
                        repo_key: repo_key.clone(),
                        old_commit: result.old_commit,
                        new_commit: result.new_commit,
                    }
                }
                Ok(UpdateStatus::Updated(_)) => RepoUpdateOutcome::AlreadyUpToDate {
                    repo_key: repo_key.clone(),
                },
                Ok(UpdateStatus::Pinned { ref_name }) => RepoUpdateOutcome::Pinned {
                    repo_key: repo_key.clone(),
                    ref_name,
                },
                Ok(UpdateStatus::LiveLocal) => RepoUpdateOutcome::LiveLocal {
                    repo_key: repo_key.clone(),
                },
                Err(e) => RepoUpdateOutcome::Failed {
                    repo_key: repo_key.clone(),
                    error: e.to_string(),
                },
            }
        };
        outcomes.push(outcome);
    }

    save_registry_or_warn(&registry);

    Ok(outcomes)
}

// ── Marketplace helpers ─────────────────────────────────────────────

// ── Marketplace plugin install (direct CLI install) ─────────────────

pub struct MarketplaceInstallOutcome {
    pub repo_key: String,
    pub plugin_names: Vec<String>,
    pub warnings: Vec<String>,
    pub source_display_name: String,
    pub plugin_subdir: String,
    pub source_is_git: bool,
    pub already_installed: bool,
    pub other_copies_note: Option<String>,
}

#[derive(Debug)]
pub enum MarketplaceInstallError {
    UnknownQualifier {
        qualifier: String,
        registered: Vec<String>,
    },
    AmbiguousQualifier {
        qualifier: String,
        sources: Vec<String>,
    },
    QualifiedNameNotFound {
        name: String,
        source_display: String,
    },
    NameNotFound {
        name: String,
        skipped_sources: Vec<String>,
    },
    NameAmbiguous {
        name: String,
        candidates: Vec<String>,
    },
    PartialScan {
        name: String,
        skipped_sources: Vec<String>,
    },
    Sync {
        source_display: String,
        detail: String,
    },
    Install(InstallError),
}

impl MarketplaceInstallError {
    /// Stable telemetry category, reusing [`classify_install_error`] for the
    /// underlying install failure.
    pub fn category(&self) -> String {
        match self {
            Self::UnknownQualifier { .. } => "unknown_marketplace".to_string(),
            Self::AmbiguousQualifier { .. } => "ambiguous_marketplace".to_string(),
            Self::QualifiedNameNotFound { .. } | Self::NameNotFound { .. } => {
                "not_found".to_string()
            }
            Self::NameAmbiguous { .. } => "ambiguous_plugin".to_string(),
            Self::PartialScan { .. } => "partial_scan".to_string(),
            Self::Sync { .. } => "sync_failed".to_string(),
            Self::Install(e) => classify_install_error(e),
        }
    }
}

impl std::fmt::Display for MarketplaceInstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownQualifier {
                qualifier,
                registered,
            } => {
                if registered.is_empty() {
                    write!(
                        f,
                        "Unknown marketplace \"{qualifier}\". No marketplaces are registered; \
                         add one with `grok plugin marketplace add`."
                    )
                } else {
                    let list = bullet_list(registered);
                    write!(
                        f,
                        "Unknown marketplace \"{qualifier}\".\n\
                         Registered marketplaces (pin with <name>@<qualifier>):\n{list}"
                    )
                }
            }
            Self::AmbiguousQualifier { qualifier, sources } => {
                let list = bullet_list(sources);
                write!(
                    f,
                    "Marketplace qualifier \"{qualifier}\" matches multiple registered sources \
                     that cannot be distinguished by qualifier:\n{list}\n\
                     Rename or remove one in your marketplace config so each source has a unique \
                     qualifier."
                )
            }
            Self::QualifiedNameNotFound {
                name,
                source_display,
            } => {
                write!(
                    f,
                    "No marketplace plugin named \"{name}\" in \"{source_display}\"."
                )
            }
            Self::NameNotFound {
                name,
                skipped_sources,
            } => {
                write!(
                    f,
                    "No marketplace plugin named \"{name}\" in any registered marketplace.\n\
                     Install a local directory with `grok plugin install ./{name}`, or add a \
                     source with `grok plugin marketplace add`."
                )?;
                if !skipped_sources.is_empty() {
                    write!(
                        f,
                        "\n({} marketplace source(s) could not be synced and were skipped: {})",
                        skipped_sources.len(),
                        skipped_sources.join(", "),
                    )?;
                }
                Ok(())
            }
            Self::NameAmbiguous { name, candidates } => {
                let list = bullet_list(candidates);
                write!(
                    f,
                    "Multiple marketplaces provide a plugin named \"{name}\":\n{list}\n\
                     Pin one with `grok plugin install {name}@<qualifier>`."
                )
            }
            Self::PartialScan {
                name,
                skipped_sources,
            } => {
                let list = bullet_list(skipped_sources);
                write!(
                    f,
                    "Couldn't scan every marketplace while resolving \"{name}\", so it can't be \
                     resolved safely. Unscanned source(s):\n{list}\n\
                     Retry, or pin the source explicitly with `grok plugin install {name}@<qualifier>`."
                )
            }
            Self::Sync {
                source_display,
                detail,
            } => {
                write!(
                    f,
                    "Failed to sync marketplace \"{source_display}\": {detail}"
                )
            }
            Self::Install(e) => write!(f, "{e}"),
        }
    }
}

fn bullet_list(items: &[String]) -> String {
    items
        .iter()
        .map(|item| format!("  - {item}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The require-sha pin policy for remote plugin code. Disk-only config + env,
/// both tighten-only: a remote campaign overlay must not be able to relax a
/// local security policy, and an unreadable config falls back to the env knob.

/// Marketplace sources from config.toml + settings JSON, unfiltered.
pub(crate) fn load_marketplace_sources() -> Vec<MarketplaceSource> {
    let config = crate::config::load_effective_config()
        .ok()
        .unwrap_or(toml::Value::Table(toml::map::Map::new()));
    let mut sources = load_sources(&config);
    sources.extend(load_extra_sources_from_settings(&sources));
    sources
}

/// Like [`load_marketplace_sources`] but drops git sources blocked by the
/// managed `marketplace_allowlist`. Install paths must use this so policy
/// cannot be bypassed.
pub(crate) fn load_filtered_marketplace_sources() -> Vec<MarketplaceSource> {
    let allowlist =
        &xai_grok_workspace::permission::resolution::managed_settings().marketplace_allowlist;
    filter_sources_by_allowlist(load_marketplace_sources(), allowlist)
}

fn filter_sources_by_allowlist(
    mut sources: Vec<MarketplaceSource>,
    allowlist: &xai_grok_workspace::permission::resolution::MarketplaceAllowlist,
) -> Vec<MarketplaceSource> {
    if allowlist.is_restricted() {
        sources.retain(|source| match &source.kind {
            SourceKind::Git { url, .. } => {
                if allowlist.is_url_allowed(url) {
                    true
                } else {
                    tracing::warn!(
                        name = %source.name,
                        url,
                        reason = %allowlist.block_reason(),
                        "Marketplace source blocked by allowlist"
                    );
                    false
                }
            }
            SourceKind::Local { .. } => true,
        });
    }
    sources
}

fn registered_source_label(source: &MarketplaceSource) -> String {
    let qualifier = install_resolve::addressable_qualifier(source);
    format!("{} ({qualifier})", source.name)
}

fn candidate_label(source: &MarketplaceSource, name: &str) -> String {
    let qualifier = install_resolve::addressable_qualifier(source);
    format!("{} (pin: {name}@{qualifier})", source.name)
}

fn resolve_source_root_for_install(
    source: &MarketplaceSource,
    cache_root: &Path,
) -> Result<MarketplaceSourceRoot, String> {
    match &source.kind {
        SourceKind::Local { path } => {
            if path.is_dir() {
                Ok(MarketplaceSourceRoot {
                    path: path.clone(),
                    _lease: None,
                })
            } else {
                Err(format!(
                    "local source directory not found: {}",
                    path.display()
                ))
            }
        }
        SourceKind::Git { url, branch } => {
            let lease = git::sync_source_cache_with_mode(
                url,
                branch.as_deref(),
                cache_root,
                git::SyncMode::UseTtl,
            )?;
            Ok(MarketplaceSourceRoot {
                path: lease.path.clone(),
                _lease: Some(lease),
            })
        }
    }
}

#[derive(Debug)]
struct InstallPlan {
    source_index: usize,
    entry: MarketplaceEntry,
    other_copies_note: Option<String>,
    /// Sources skipped during a bare-name scan because they failed to sync.
    skipped_sources: Vec<String>,
}

/// Map a marketplace ref to the source + entry to install, or a typed error.
/// Pure over `sources` + the `scan` closure so it is unit-testable.
fn plan_install(
    sources: &[MarketplaceSource],
    name: &str,
    qualifier: Option<&str>,
    mut scan: impl FnMut(&MarketplaceSource) -> Result<Vec<MarketplaceEntry>, String>,
) -> Result<InstallPlan, MarketplaceInstallError> {
    match qualifier {
        Some(qualifier) => {
            let index = install_resolve::resolve_qualified_source(qualifier, sources)
                .map_err(|e| map_qualifier_resolve_error(qualifier, sources, e))?;
            let source = &sources[index];
            let entry = scan(source)
                .map_err(|detail| MarketplaceInstallError::Sync {
                    source_display: source.name.clone(),
                    detail,
                })?
                .into_iter()
                .find(|entry| entry.name.eq_ignore_ascii_case(name))
                .ok_or_else(|| MarketplaceInstallError::QualifiedNameNotFound {
                    name: name.to_string(),
                    source_display: source.name.clone(),
                })?;
            Ok(InstallPlan {
                source_index: index,
                entry,
                other_copies_note: None,
                skipped_sources: Vec::new(),
            })
        }
        None => {
            let mut owned: Vec<(usize, MarketplaceEntry)> = Vec::new();
            let mut skipped_sources = Vec::new();
            for (index, source) in sources.iter().enumerate() {
                match scan(source) {
                    Ok(entries) => {
                        for entry in entries {
                            owned.push((index, entry));
                        }
                    }
                    Err(_) => skipped_sources.push(source.name.clone()),
                }
            }
            let scanned: Vec<install_resolve::ScannedEntry> = owned
                .iter()
                .map(|(index, entry)| install_resolve::ScannedEntry {
                    source: &sources[*index],
                    entry,
                })
                .collect();
            let selection = match install_resolve::select_bare_name(name, &scanned) {
                Ok(selection) => selection,
                Err(install_resolve::BareNameError::NotFound) => {
                    drop(scanned);
                    return Err(if skipped_sources.is_empty() {
                        MarketplaceInstallError::NameNotFound {
                            name: name.to_string(),
                            skipped_sources,
                        }
                    } else {
                        MarketplaceInstallError::PartialScan {
                            name: name.to_string(),
                            skipped_sources,
                        }
                    });
                }
                Err(install_resolve::BareNameError::Ambiguous { matched }) => {
                    if !skipped_sources.is_empty() {
                        drop(scanned);
                        return Err(MarketplaceInstallError::PartialScan {
                            name: name.to_string(),
                            skipped_sources,
                        });
                    }
                    let candidates = matched
                        .iter()
                        .map(|&i| candidate_label(scanned[i].source, name))
                        .collect();
                    drop(scanned);
                    return Err(MarketplaceInstallError::NameAmbiguous {
                        name: name.to_string(),
                        candidates,
                    });
                }
            };
            let chosen_source_index = owned[selection.chosen].0;
            let chosen_is_official = match &sources[chosen_source_index].kind {
                SourceKind::Git { url, .. } => is_official_source_url(url),
                SourceKind::Local { .. } => false,
            };
            let other_copies_note = (selection.other_count > 0).then(|| {
                format!(
                    "Note: \"{name}\" is also available from {} other marketplace(s); \
                     pin a specific one with `{name}@<qualifier>`.",
                    selection.other_count
                )
            });
            drop(scanned);
            if !chosen_is_official && !skipped_sources.is_empty() {
                return Err(MarketplaceInstallError::PartialScan {
                    name: name.to_string(),
                    skipped_sources,
                });
            }
            let (source_index, entry) = owned.swap_remove(selection.chosen);
            Ok(InstallPlan {
                source_index,
                entry,
                other_copies_note,
                skipped_sources,
            })
        }
    }
}

/// Install a plugin by marketplace name, optionally pinned via `qualifier`
/// (`owner/repo` or `local/<slug>`). Loads allowlist-filtered sources and
/// delegates selection to [`plan_install`].
pub fn install_marketplace_plugin(
    name: &str,
    qualifier: Option<&str>,
) -> Result<MarketplaceInstallOutcome, MarketplaceInstallError> {
    let sources = load_filtered_marketplace_sources();
    let mut registry = InstallRegistry::load();
    let cache_root = git::default_cache_root();
    install_marketplace_plugin_with(
        &sources,
        &mut registry,
        &cache_root,
        name,
        qualifier,
        crate::config::post_install_plugin,
    )
}

fn install_marketplace_plugin_with(
    sources: &[MarketplaceSource],
    registry: &mut InstallRegistry,
    cache_root: &Path,
    name: &str,
    qualifier: Option<&str>,
    post_install: impl Fn(&str) -> (Vec<String>, Vec<String>),
) -> Result<MarketplaceInstallOutcome, MarketplaceInstallError> {
    let plan = plan_install(sources, name, qualifier, |source| {
        resolve_source_root_for_install(source, cache_root)
            .map(|root| scan_marketplace(&root.path).entries)
    })?;

    let source = &sources[plan.source_index];
    let root = resolve_source_root_for_install(source, cache_root).map_err(|detail| {
        MarketplaceInstallError::Sync {
            source_display: source.name.clone(),
            detail,
        }
    })?;
    let mut outcome =
        install_marketplace_entry(source, &root.path, &plan.entry, registry, post_install)?;
    if !outcome.already_installed {
        outcome.other_copies_note = plan.other_copies_note;
        for skipped in plan.skipped_sources {
            outcome.warnings.push(format!(
                "marketplace source \"{skipped}\" could not be synced and was skipped"
            ));
        }
    }
    Ok(outcome)
}

pub fn resolve_marketplace_source_name(
    name: &str,
    qualifier: Option<&str>,
) -> Result<String, MarketplaceInstallError> {
    let sources = load_filtered_marketplace_sources();
    let cache_root = git::default_cache_root();
    resolve_marketplace_source_name_with(&sources, &cache_root, name, qualifier)
}

fn resolve_marketplace_source_name_with(
    sources: &[MarketplaceSource],
    cache_root: &Path,
    name: &str,
    qualifier: Option<&str>,
) -> Result<String, MarketplaceInstallError> {
    let plan = plan_install(sources, name, qualifier, |source| {
        resolve_source_root_for_install(source, cache_root)
            .map(|root| scan_marketplace(&root.path).entries)
    })?;
    Ok(sources[plan.source_index].name.clone())
}

pub fn resolve_qualified_source_name(qualifier: &str) -> Result<String, MarketplaceInstallError> {
    resolve_qualified_source_name_with(&load_filtered_marketplace_sources(), qualifier)
}

fn resolve_qualified_source_name_with(
    sources: &[MarketplaceSource],
    qualifier: &str,
) -> Result<String, MarketplaceInstallError> {
    let index = install_resolve::resolve_qualified_source(qualifier, sources)
        .map_err(|e| map_qualifier_resolve_error(qualifier, sources, e))?;
    Ok(sources[index].name.clone())
}

fn map_qualifier_resolve_error(
    qualifier: &str,
    sources: &[MarketplaceSource],
    e: install_resolve::QualifierResolveError,
) -> MarketplaceInstallError {
    use install_resolve::QualifierResolveError;
    match e {
        QualifierResolveError::Unknown => MarketplaceInstallError::UnknownQualifier {
            qualifier: qualifier.to_string(),
            registered: sources.iter().map(registered_source_label).collect(),
        },
        QualifierResolveError::Ambiguous(indices) => MarketplaceInstallError::AmbiguousQualifier {
            qualifier: qualifier.to_string(),
            sources: indices.iter().map(|&i| sources[i].name.clone()).collect(),
        },
    }
}

fn install_marketplace_entry(
    source: &MarketplaceSource,
    marketplace_root: &Path,
    entry: &MarketplaceEntry,
    registry: &mut InstallRegistry,
    post_install: impl Fn(&str) -> (Vec<String>, Vec<String>),
) -> Result<MarketplaceInstallOutcome, MarketplaceInstallError> {
    let source_is_git = matches!(&source.kind, SourceKind::Git { .. });
    let source_identity = match &source.kind {
        SourceKind::Git { url, .. } => url.clone(),
        SourceKind::Local { path } => path.display().to_string(),
    };
    let plugin_subdir = MarketplaceRelativePath::parse(&entry.relative_path)
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|_| entry.relative_path.clone());

    if let Some((repo_key, _version)) =
        installer::find_installed_marketplace_plugin(registry, &source_identity, &plugin_subdir)
    {
        let plugin_names = registry
            .get_repo(&repo_key)
            .map(|repo| repo.plugins.keys().cloned().collect())
            .unwrap_or_default();
        return Ok(MarketplaceInstallOutcome {
            repo_key,
            plugin_names,
            warnings: Vec::new(),
            source_display_name: source.name.clone(),
            plugin_subdir,
            source_is_git,
            already_installed: true,
            other_copies_note: None,
        });
    }

    let provenance = MarketplaceProvenance {
        source_url_or_path: source_identity,
        source_display_name: source.name.clone(),
        plugin_subdir: plugin_subdir.clone(),
    };

    let result = if let Some(remote_url) = entry.remote_url.as_deref() {
        let require_sha = crate::plugin::marketplace_require_sha();
        installer::install_from_remote_url(
            remote_url,
            entry.remote_ref.as_deref(),
            entry.remote_sha.as_deref(),
            entry.remote_subdir.as_deref(),
            &plugin_subdir,
            provenance,
            registry,
            require_sha,
        )
    } else {
        installer::install_from_marketplace(marketplace_root, &plugin_subdir, provenance, registry)
    };

    let repo_key = match result {
        Ok(installer::MarketplaceInstallResult::Installed { repo_key })
        | Ok(installer::MarketplaceInstallResult::AlreadyInstalled { repo_key }) => repo_key,
        Err(e) => return Err(MarketplaceInstallError::Install(e)),
    };

    let (plugin_names, warnings) = post_install(&repo_key);

    Ok(MarketplaceInstallOutcome {
        repo_key,
        plugin_names,
        warnings,
        source_display_name: source.name.clone(),
        plugin_subdir,
        source_is_git,
        already_installed: false,
        other_copies_note: None,
    })
}

/// Remove all plugins installed from a marketplace source. Returns removed repo keys.
pub fn uninstall_marketplace_source_plugins(source_identity: &str) -> Vec<String> {
    let mut registry = InstallRegistry::load();
    let to_remove: Vec<(String, std::path::PathBuf, InstalledRepo)> = registry
        .list()
        .iter()
        .filter_map(|(key, repo)| {
            repo.marketplace.as_ref().and_then(|mp| {
                if mp.source_url_or_path == source_identity {
                    Some((key.to_string(), repo.path.clone(), (*repo).clone()))
                } else {
                    None
                }
            })
        })
        .collect();

    for (key, path, repo) in &to_remove {
        if let Err(e) = git_install::remove_repo_path(path) {
            tracing::warn!("failed to remove plugin dir for {key}: {e}");
        }
        let scope = match xai_dirs::home_dir() {
            Some(home) if path.starts_with(&home) => PluginScope::User,
            _ => PluginScope::ConfigPath,
        };
        git_install::cleanup_plugin_data(repo, scope);
        registry.remove(key);
    }

    if !to_remove.is_empty() {
        save_registry_or_warn(&registry);
    }

    to_remove.into_iter().map(|(key, _, _)| key).collect()
}

/// Remove a `[[marketplace.sources]]` entry matching `git` or `path`.
/// Returns `Some(new_content)` on removal, `None` if not found or unparseable.
pub fn remove_toml_marketplace_block(content: &str, source_identity: &str) -> Option<String> {
    let mut doc: toml_edit::DocumentMut = content.parse().ok()?;

    let sources = doc
        .get_mut("marketplace")?
        .get_mut("sources")?
        .as_array_of_tables_mut()?;

    let identity_normalized = source_identity.trim_end_matches(".git");
    let idx = sources.iter().position(|entry| {
        if let Some(git) = entry.get("git").and_then(|v| v.as_str()) {
            return git.trim_end_matches(".git") == identity_normalized;
        }
        if let Some(path) = entry.get("path").and_then(|v| v.as_str()) {
            // The identity comes from a loaded source, whose `~` was expanded —
            // match hand-written `path = "~/x"` entries by expanding them too.
            return path == source_identity || expand_tilde(path) == Path::new(source_identity);
        }
        false
    })?;

    sources.remove(idx);

    // Keep other `[marketplace]` keys (the sticky official_marketplace_auto_installed
    // flag) when `sources` empties; drop the table only when fully empty. Else
    // removing an unrelated source wipes the flag and auto-register re-adds it.
    let sources_now_empty = doc
        .get("marketplace")
        .and_then(|m| m.get("sources"))
        .and_then(|s| s.as_array_of_tables())
        .is_some_and(|a| a.is_empty());
    if sources_now_empty
        && let Some(marketplace) = doc.get_mut("marketplace").and_then(|m| m.as_table_mut())
    {
        marketplace.remove("sources");
        if marketplace.is_empty() {
            doc.remove("marketplace");
        }
    }

    Some(doc.to_string())
}

/// Try removing a source from `settings.json` / `known_marketplaces.json` under
/// `~/.grok/` and `~/.claude/`. Returns `true` if removed from at least one file.
pub fn try_remove_source_from_json_files(source_url_or_path: &str) -> bool {
    // Resolve user grok via user_grok_home() (None when no home resolves) and
    // home separately, so removal still runs from $GROK_HOME when no home dir
    // exists, and never touches a cwd-relative .grok.
    let home = xai_dirs::home_dir();
    let grok = xai_grok_config::user_grok_home();

    let mut settings_candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Some(ref grok) = grok {
        settings_candidates.push(grok.join("settings.local.json"));
        settings_candidates.push(grok.join("settings.json"));
    }
    if let Some(ref home) = home {
        settings_candidates.push(home.join(".claude").join("settings.local.json"));
        settings_candidates.push(home.join(".claude").join("settings.json"));
    }

    let mut known_candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Some(ref grok) = grok {
        known_candidates.push(grok.join("plugins").join("known_marketplaces.json"));
    }
    if let Some(ref home) = home {
        known_candidates.push(
            home.join(".claude")
                .join("plugins")
                .join("known_marketplaces.json"),
        );
    }

    let mut removed = false;

    for path in &settings_candidates {
        if try_remove_from_json_object(path, Some("extraKnownMarketplaces"), source_url_or_path) {
            removed = true;
        }
    }

    for path in &known_candidates {
        if try_remove_from_json_object(path, None, source_url_or_path) {
            removed = true;
        }
    }

    removed
}

/// Check whether a JSON source config matches a URL/path identity.
fn json_source_matches(config: &serde_json::Value, identity: &str) -> bool {
    let source_obj = match config.get("source") {
        Some(v) if v.is_string() => config,
        Some(v) if v.is_object() => v,
        _ => return false,
    };
    let Some(source_type) = source_obj.get("source").and_then(|v| v.as_str()) else {
        return false;
    };
    match source_type {
        "git" => source_obj
            .get("url")
            .and_then(|v| v.as_str())
            .is_some_and(|u| u.trim_end_matches(".git") == identity.trim_end_matches(".git")),
        "github" => source_obj
            .get("repo")
            .and_then(|v| v.as_str())
            .is_some_and(|repo| {
                let expanded = format!("https://github.com/{repo}.git");
                expanded.trim_end_matches(".git") == identity.trim_end_matches(".git")
            }),
        "local" => source_obj
            .get("path")
            .and_then(|v| v.as_str())
            .is_some_and(|p| p == identity),
        _ => false,
    }
}

/// Remove a matching source entry from a JSON file. Returns `true` if removed.
fn try_remove_from_json_object(
    path: &Path,
    nested_key: Option<&str>,
    source_url_or_path: &str,
) -> bool {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let mut json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return false,
    };

    let map = if let Some(key) = nested_key {
        match json.get_mut(key).and_then(|v| v.as_object_mut()) {
            Some(m) => m,
            None => return false,
        }
    } else {
        match json.as_object_mut() {
            Some(m) => m,
            None => return false,
        }
    };

    let matching_key = map.iter().find_map(|(name, config)| {
        if json_source_matches(config, source_url_or_path) {
            Some(name.clone())
        } else {
            None
        }
    });

    let Some(key) = matching_key else {
        return false;
    };

    map.remove(&key);

    match serde_json::to_string_pretty(&json) {
        Ok(new_content) => {
            if std::fs::write(path, format!("{new_content}\n")).is_ok() {
                tracing::info!(key = %key, "removed marketplace source from JSON file");
                true
            } else {
                false
            }
        }
        Err(_) => false,
    }
}

// ── Tests ───────────────────────────────────────────────────────────

