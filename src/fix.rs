//! Fixes broken file references by replacing them with matching file names.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::lint::{FileResult, RULE_FILE_REFERENCE_MISSING};

/// A broken reference and its location in a file.
#[derive(Debug, Clone)]
pub struct BrokenReference {
    pub file_path: String,
    pub line: usize,
    pub reference: String,
}

/// A fix to apply: replace a broken reference with a working path.
#[derive(Debug, Clone)]
pub struct Fix {
    pub file_path: String,
    pub line: usize,
    pub old_reference: String,
    pub new_reference: String,
}

/// Finds all broken file references from lint results.
pub fn find_broken_references(results: &[FileResult]) -> Vec<BrokenReference> {
    let mut broken = Vec::new();
    
    for result in results {
        for finding in &result.findings {
            if finding.rule == RULE_FILE_REFERENCE_MISSING {
                // Extract the reference from the message
                // Message format: "referenced file {target:?} does not exist"
                if let Some(reference) = extract_reference_from_message(&finding.message) {
                    broken.push(BrokenReference {
                        file_path: result.path.clone(),
                        line: finding.line,
                        reference,
                    });
                }
            }
        }
    }
    
    broken
}

/// Extracts the file reference from a finding message.
fn extract_reference_from_message(message: &str) -> Option<String> {
    // Message format: "referenced file {target:?} does not exist"
    // The target is quoted in the message
    let start = message.find('"')?;
    let end = message.rfind('"')?;
    if start < end {
        let target = &message[start + 1..end];
        Some(target.to_string())
    } else {
        None
    }
}

/// Recursively walks directories to find all files.
fn find_all_files(root: &Path, excludes: &[String]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    
    // Skip directories that should be excluded
    let skip_dirs: std::collections::HashSet<&str> = [
        ".git",
        ".next",
        ".venv",
        "build",
        "dist",
        "node_modules",
        "target",
        "vendor",
    ].iter().cloned().collect();
    
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries {
            if let Ok(entry) = entry {
                let path = entry.path();
                let file_name_str: String;
                {
                    let file_name = entry.file_name();
                    file_name_str = file_name.to_string_lossy().into_owned();
                    
                    // Skip excluded directories
                    if entry.file_type().map_or(false, |ft| ft.is_dir()) {
                        if skip_dirs.contains(file_name_str.as_str()) {
                            continue;
                        }
                        files.extend(find_all_files(&path, excludes));
                    } else {
                        files.push(path);
                    }
                }
            }
        }
    }
    
    files
}

/// Searches for files with the same name as the broken reference.
/// Returns a map from file name to all paths where it exists.
pub fn find_matching_files(
    broken_refs: &[BrokenReference],
    paths: &[String],
    excludes: &[String],
) -> HashMap<String, Vec<PathBuf>> {
    let mut file_map: HashMap<String, Vec<PathBuf>> = HashMap::new();
    
    // Collect all file names from broken references
    let ref_names: std::collections::HashSet<String> = broken_refs
        .iter()
        .map(|r| {
            let path = Path::new(&r.reference);
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&r.reference)
                .to_string()
        })
        .collect();
    
    // Find all files in the linted paths
    for path_str in paths {
        let path = Path::new(path_str);
        if path.is_dir() {
            let files = find_all_files(path, excludes);
            for file_path in files {
                if let Some(file_name) = file_path.file_name().and_then(|n| n.to_str()) {
                    if ref_names.contains(file_name) {
                        file_map
                            .entry(file_name.to_string())
                            .or_default()
                            .push(file_path);
                    }
                }
            }
        } else {
            // If it's a file, check if it matches
            if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                if ref_names.contains(file_name) {
                    file_map
                        .entry(file_name.to_string())
                        .or_default()
                        .push(path.to_path_buf());
                }
            }
        }
    }
    
    file_map
}

/// Finds the best replacement path for a broken reference.
/// Returns the relative path from the file's directory to the matching file.
pub fn find_replacement_path(
    broken: &BrokenReference,
    file_map: &HashMap<String, Vec<PathBuf>>,
    file_dir: &Path,
) -> Option<String> {
    let path = Path::new(&broken.reference);
    let file_name = path.file_name()?.to_str()?;
    
    if let Some(matches) = file_map.get(file_name) {
        // Look for exact match by file name
        for match_path in matches {
            if Some(path.file_name()) == Some(match_path.file_name()) {
                // Calculate relative path from file_dir to match_path
                if let Some(relative) = pathdiff::diff_paths(match_path, file_dir) {
                    let relative_str = relative.to_string_lossy();
                    // Convert to forward slashes for consistency
                    let normalized = relative_str.replace('\\', "/");
                    // Ensure it starts with ./
                    if !normalized.starts_with("./") && !normalized.starts_with("../") {
                        return Some(format!("./{}", normalized));
                    }
                    return Some(normalized);
                }
            }
        }
        
        // If no exact match, return the first match as a relative path
        if !matches.is_empty() {
            if let Some(relative) = pathdiff::diff_paths(&matches[0], file_dir) {
                let relative_str = relative.to_string_lossy();
                let normalized = relative_str.replace('\\', "/");
                // Ensure it starts with ./
                if !normalized.starts_with("./") && !normalized.starts_with("../") {
                    return Some(format!("./{}", normalized));
                }
                return Some(normalized);
            }
        }
    }
    
    None
}

/// Applies fixes to files by replacing broken references.
pub fn apply_fixes(
    fixes: &[Fix],
    dry_run: bool,
) -> Result<Vec<String>, String> {
    let mut applied = Vec::new();
    
    // Group fixes by file
    let mut fixes_by_file: HashMap<String, Vec<&Fix>> = HashMap::new();
    for fix in fixes {
        fixes_by_file.entry(fix.file_path.clone()).or_default().push(fix);
    }
    
    for (file_path, file_fixes) in fixes_by_file {
        let content = fs::read_to_string(&file_path)
            .map_err(|e| format!("cannot read {}: {}", file_path, e))?;
        
        let mut new_content = content.clone();
        let mut actual_fixes = Vec::new();
        
        // Sort fixes by line in descending order to avoid line number shifts
        let mut sorted_fixes = file_fixes.clone();
        sorted_fixes.sort_by(|a, b| b.line.cmp(&a.line));
        
        for fix in sorted_fixes {
            let lines: Vec<&str> = new_content.lines().collect();
            if fix.line <= lines.len() {
                let line = lines[fix.line - 1];
                if line.contains(&fix.old_reference) {
                    // Replace the old reference with the new one in this line
                    let new_line = line.replace(&fix.old_reference, &fix.new_reference);
                    new_content = new_content
                        .lines()
                        .enumerate()
                        .map(|(i, l)| {
                            if i == fix.line - 1 {
                                new_line.as_str()
                            } else {
                                l
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    
                    actual_fixes.push(format!(
                        "Fixed {}:{} - replaced {} with {}",
                        file_path, fix.line, fix.old_reference, fix.new_reference
                    ));
                }
            }
        }
        
        if !actual_fixes.is_empty() {
            if !dry_run {
                fs::write(&file_path, new_content)
                    .map_err(|e| format!("cannot write {}: {}", file_path, e))?;
            }
            applied.extend(actual_fixes);
        }
    }
    
    Ok(applied)
}

/// Generates fixes for broken references.
pub fn generate_fixes(
    results: &[FileResult],
    paths: &[String],
    excludes: &[String],
) -> Vec<Fix> {
    let broken_refs = find_broken_references(results);
    let file_map = find_matching_files(&broken_refs, paths, excludes);
    let mut fixes = Vec::new();
    
    for broken in broken_refs {
        let file_path = Path::new(&broken.file_path);
        let file_dir = file_path.parent().unwrap_or_else(|| Path::new(""));
        
        if let Some(replacement) = find_replacement_path(&broken, &file_map, file_dir) {
            fixes.push(Fix {
                file_path: broken.file_path,
                line: broken.line,
                old_reference: broken.reference,
                new_reference: replacement,
            });
        }
    }
    
    fixes
}
