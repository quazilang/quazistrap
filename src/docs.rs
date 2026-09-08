use std::fs;
use std::path::{Path, PathBuf};

/// Returns every Markdown file below `root`, in a stable order.
fn markdown_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    let entries =
        fs::read_dir(root).map_err(|error| format!("cannot read {}: {error}", root.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("cannot read documentation entry: {error}"))?;
        let path = entry.path();
        if path.is_dir() {
            files.extend(markdown_files(&path)?);
        } else if path.extension().is_some_and(|extension| extension == "md") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn local_destination(destination: &str) -> Option<&str> {
    let destination = destination.trim();
    if destination.is_empty()
        || destination.starts_with('#')
        || destination.contains("://")
        || destination.starts_with("mailto:")
    {
        return None;
    }

    let destination = destination
        .strip_prefix('<')
        .and_then(|value| value.strip_suffix('>'))
        .unwrap_or(destination);
    Some(destination.split_whitespace().next().unwrap_or(destination))
}

fn inline_link_destinations(line: &str) -> Vec<&str> {
    let mut destinations = Vec::new();
    let bytes = line.as_bytes();
    let mut index = 0;
    let mut in_code = false;
    while index < bytes.len() {
        if bytes[index] == b'`' {
            in_code = !in_code;
            index += 1;
            continue;
        };
        if !in_code && bytes[index] == b']' && bytes.get(index + 1) == Some(&b'(') {
            let destination_start = index + 2;
            let Some(destination_end) = line[destination_start..].find(')') else {
                break;
            };
            let destination_end = destination_start + destination_end;
            destinations.push(&line[destination_start..destination_end]);
            index = destination_end + 1;
            continue;
        }
        index += 1;
    }
    destinations
}

fn heading_id(heading: &str) -> String {
    let mut id = String::new();
    let mut previous_was_separator = false;
    for character in heading.chars() {
        if character == '`' {
            continue;
        }
        if character.is_alphanumeric() || character == '_' || character == '-' {
            id.extend(character.to_lowercase());
            previous_was_separator = false;
        } else if character.is_whitespace() && !id.is_empty() && !previous_was_separator {
            id.push('-');
            previous_was_separator = true;
        }
    }
    id.trim_end_matches('-').to_owned()
}

fn resolve_repository_path(
    current: &Path,
    repository_root: &Path,
    target: &str,
) -> Option<PathBuf> {
    if Path::new(target).is_absolute() {
        return None;
    }
    let relative_parent = current.parent()?.strip_prefix(repository_root).ok()?;
    let mut components = relative_parent
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(component) => Some(component.to_os_string()),
            _ => None,
        })
        .collect::<Vec<_>>();
    for component in Path::new(target).components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                components.pop()?;
            }
            std::path::Component::Normal(component) => components.push(component.to_os_string()),
            std::path::Component::RootDir | std::path::Component::Prefix(_) => return None,
        }
    }
    Some(repository_root.join(components.iter().collect::<PathBuf>()))
}

fn heading_ids(source: &str) -> Vec<String> {
    let mut headings = Vec::new();
    let mut in_fence = false;
    for line in source.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let trimmed = line.trim_start();
        let hashes = trimmed.bytes().take_while(|byte| *byte == b'#').count();
        if hashes > 0 && hashes <= 6 && trimmed.as_bytes().get(hashes) == Some(&b' ') {
            let base = heading_id(trimmed[hashes..].trim());
            if base.is_empty() {
                continue;
            }
            let mut candidate = base.clone();
            let mut suffix = 1;
            while headings.contains(&candidate) {
                candidate = format!("{base}-{suffix}");
                suffix += 1;
            }
            headings.push(candidate);
        }
    }
    headings
}

/// Checks repository-local inline Markdown links in the canonical documentation tree.
///
/// This intentionally does not fetch external URLs or traverse links into sibling
/// repositories. Those references remain valid integration documentation but are
/// outside this repository's offline test boundary.
fn invalid_local_links(docs_root: &Path) -> Result<Vec<String>, String> {
    let mut invalid = Vec::new();
    let repository_root = docs_root.parent().unwrap_or(docs_root);
    let canonical_repository_root = fs::canonicalize(repository_root).map_err(|error| {
        format!(
            "cannot resolve repository root {}: {error}",
            repository_root.display()
        )
    })?;
    let mut documents = markdown_files(docs_root)?;
    let repository_readme = docs_root.parent().unwrap_or(docs_root).join("README.md");
    if repository_readme.is_file() {
        documents.push(repository_readme);
    }
    for path in documents {
        let source = fs::read_to_string(&path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        let mut in_fence = false;
        for (line_number, line) in source.lines().enumerate() {
            if line.trim_start().starts_with("```") {
                in_fence = !in_fence;
                continue;
            }
            if in_fence {
                continue;
            }
            for destination in inline_link_destinations(line) {
                let Some(destination) = local_destination(destination) else {
                    continue;
                };
                let (target, fragment) = destination
                    .split_once('#')
                    .map_or((destination, None), |(target, fragment)| {
                        (target, Some(fragment))
                    });
                let resolved = if target.is_empty() {
                    path.clone()
                } else {
                    let Some(resolved) = resolve_repository_path(&path, repository_root, target)
                    else {
                        continue;
                    };
                    resolved
                };
                if !resolved.exists() {
                    invalid.push(format!(
                        "{}:{} links to missing local path `{destination}`",
                        path.display(),
                        line_number + 1
                    ));
                    continue;
                }
                let canonical_resolved = fs::canonicalize(&resolved).map_err(|error| {
                    format!("cannot resolve linked path {}: {error}", resolved.display())
                })?;
                if !canonical_resolved.starts_with(&canonical_repository_root) {
                    invalid.push(format!(
                        "{}:{} links through a path that escapes the repository `{destination}`",
                        path.display(),
                        line_number + 1
                    ));
                    continue;
                }
                if let Some(fragment) = fragment.filter(|fragment| !fragment.is_empty()) {
                    if resolved.is_dir()
                        || resolved
                            .extension()
                            .is_none_or(|extension| extension != "md")
                    {
                        invalid.push(format!(
                            "{}:{} links to fragment `{fragment}` on a non-Markdown path `{destination}`",
                            path.display(),
                            line_number + 1
                        ));
                        continue;
                    }
                    let target_source =
                        fs::read_to_string(&canonical_resolved).map_err(|error| {
                            format!(
                                "cannot read linked document {}: {error}",
                                canonical_resolved.display()
                            )
                        })?;
                    if !heading_ids(&target_source)
                        .iter()
                        .any(|heading| heading == fragment)
                    {
                        invalid.push(format!(
                            "{}:{} links to missing fragment `{fragment}` in `{}`",
                            path.display(),
                            line_number + 1,
                            canonical_resolved.display()
                        ));
                    }
                }
            }
        }
    }
    Ok(invalid)
}

#[cfg(test)]
mod tests {
    use super::{
        heading_ids, inline_link_destinations, invalid_local_links, local_destination,
        markdown_files, resolve_repository_path,
    };
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::Path;

    fn tutorial_fixture_entries(root: &Path) -> Vec<(String, std::path::PathBuf)> {
        let mut fixtures = Vec::new();
        let mut entries = fs::read_dir(root)
            .expect("tutorial fixture tree can be listed")
            .map(|entry| entry.expect("tutorial fixture entry can be read").path())
            .collect::<Vec<_>>();
        entries.sort();
        for entry in entries {
            if entry.is_dir() {
                let entry_source = entry.join("main.qz");
                if entry_source.is_file() {
                    fixtures.push((
                        entry
                            .file_name()
                            .expect("fixture directory has a name")
                            .to_string_lossy()
                            .into_owned(),
                        entry_source,
                    ));
                }
            } else if entry.extension().is_some_and(|extension| extension == "qz") {
                fixtures.push((
                    entry
                        .file_stem()
                        .expect("fixture source has a stem")
                        .to_string_lossy()
                        .into_owned(),
                    entry,
                ));
            }
        }
        fixtures
    }

    fn tutorial_chapter_ids(root: &Path) -> BTreeSet<String> {
        markdown_files(root)
            .expect("tutorial chapters can be listed")
            .into_iter()
            .filter_map(|chapter| {
                let name = chapter.file_name()?.to_str()?;
                name.strip_suffix(".md")
                    .filter(|name| name.as_bytes().get(2) == Some(&b'-'))
                    .map(str::to_owned)
            })
            .collect()
    }

    #[test]
    fn extracts_inline_link_destinations() {
        assert_eq!(
            inline_link_destinations("See [one](first.md) and [two](<second file.md>)."),
            vec!["first.md", "<second file.md>"]
        );
    }

    #[test]
    fn ignores_external_and_fragment_destinations() {
        assert_eq!(local_destination("https://example.test/docs"), None);
        assert_eq!(local_destination("mailto:docs@example.test"), None);
        assert_eq!(local_destination("#section"), None);
        assert_eq!(
            local_destination("guide.md#section"),
            Some("guide.md#section")
        );
    }

    #[test]
    fn ignores_link_syntax_inside_inline_code() {
        assert_eq!(
            inline_link_destinations("`fn id[T](x: T)` is not a link; [guide](guide.md) is."),
            vec!["guide.md"]
        );
    }

    #[test]
    fn creates_stable_heading_ids() {
        assert_eq!(
            heading_ids("# A `code` heading!\n## A code heading!\n# Repeated\n# Repeated\n"),
            [
                "a-code-heading",
                "a-code-heading-1",
                "repeated",
                "repeated-1"
            ]
        );
    }

    #[test]
    fn does_not_follow_links_outside_the_repository() {
        let root = Path::new("/workspace/quazistrap");
        assert_eq!(
            resolve_repository_path(
                &root.join("docs/tooling/editors.md"),
                root,
                "../../../tree-sitter/"
            ),
            None
        );
        assert_eq!(
            resolve_repository_path(
                &root.join("docs/tutorial/README.md"),
                root,
                "../../examples/"
            ),
            Some(root.join("examples"))
        );
    }

    #[test]
    fn canonical_documentation_has_no_broken_local_links() {
        let docs_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs");
        let invalid = invalid_local_links(&docs_root).expect("documentation can be scanned");
        assert!(
            invalid.is_empty(),
            "broken canonical documentation links:\n{}",
            invalid.join("\n")
        );
    }

    #[test]
    fn tutorial_fixtures_analyze_and_lower_to_bytecode() {
        let fixture_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/tutorial/fixtures");
        let fixtures = tutorial_fixture_entries(&fixture_root);
        assert!(!fixtures.is_empty(), "tutorial fixtures must not be empty");
        let fixture_ids = fixtures
            .iter()
            .map(|(chapter, _)| chapter.clone())
            .collect::<BTreeSet<_>>();
        let tutorial_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/tutorial");
        assert_eq!(fixture_ids, tutorial_chapter_ids(&tutorial_root));
        for (_, fixture) in fixtures {
            let mut loaded = crate::loader::load_programs_configured(
                std::slice::from_ref(&fixture),
                None,
                true,
                &[],
            )
            .unwrap_or_else(|error| panic!("cannot load {}: {error}", fixture.display()));
            assert!(
                loaded.parse_error.is_none(),
                "cannot parse {}: {}",
                fixture.display(),
                loaded.parse_error.take().unwrap_or_default()
            );
            let program = crate::semantic::strip_cfg(&loaded.program);
            let namespaced_paths = loaded
                .namespaced_paths
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect();
            let report = crate::analysis::analyze_program_with_source_files(
                &loaded.merged_source,
                &program,
                loaded.library_fn_names,
                loaded.library_char_ranges,
                loaded.source_files.clone(),
                namespaced_paths,
            );
            assert!(
                report.errors.is_empty(),
                "tutorial fixture {} has semantic errors: {:#?}",
                fixture.display(),
                report.errors
            );
            crate::bytecode::Codegen::new(&report)
                .compile_program(&program, &loaded.source_files)
                .unwrap_or_else(|error| panic!("cannot lower {}: {error}", fixture.display()));
        }
    }
}
