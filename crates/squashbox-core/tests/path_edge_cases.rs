//! Tests for path handling edge cases and cross-platform brittleness.
//!
//! These tests verify that our path handling in `SquashFsProvider` is robust
//! across platforms, particularly around:
//! - Forward vs backslash separators
//! - Path::parent() / file_name() behavior on Windows with Unix-style paths
//! - The `is_root_path` function  
//! - PathBuf comparison semantics
//! - Edge cases in resolve_path (empty, trailing slashes, dots, etc.)
//!
//! Run with the ubuntu_stress feature for full-image tests:
//!   cargo test -p squashbox-core --features ubuntu_stress --test path_edge_cases

use squashbox_core::provider::VirtualFsProvider;
use squashbox_core::types::*;
use squashbox_core::SquashFsProvider;
use std::path::{Component, Path, PathBuf};

// ═══════════════════════════════════════════════════════════
// Path construction and comparison tests
// (These don't need any squashfs image)
// ═══════════════════════════════════════════════════════════

#[test]
fn pathbuf_from_unix_slash_has_no_prefix_on_windows() {
    // Backhand stores paths like "/usr/bin". On Windows, this should NOT
    // get a drive letter prefix component.
    let p = PathBuf::from("/usr/bin");
    let components: Vec<_> = p.components().collect();
    
    for c in &components {
        assert!(
            !matches!(c, Component::Prefix(_)),
            "Unix-style path '/usr/bin' should not have a Prefix component on any platform, got: {:?}",
            components
        );
    }
}

#[test]
fn pathbuf_parent_of_unix_path_on_windows() {
    // Verify Path::parent() works correctly with forward-slash paths on Windows
    let p = PathBuf::from("/usr/bin/ls");
    let parent = p.parent().expect("should have parent");
    
    // Parent should be "/usr/bin" (with forward slashes preserved)
    let parent_str = parent.to_string_lossy();
    assert!(
        parent_str.contains("usr") && parent_str.contains("bin"),
        "parent of '/usr/bin/ls' should contain 'usr' and 'bin', got: '{}'",
        parent_str
    );
    
    // file_name should be "ls"
    let name = p.file_name().expect("should have file_name");
    assert_eq!(name, "ls");
}

#[test]
fn pathbuf_file_name_of_root_is_none() {
    let p = PathBuf::from("/");
    assert!(
        p.file_name().is_none(),
        "file_name of '/' should be None"
    );
}

#[test]
fn pathbuf_parent_of_root_child() {
    // "/hello.txt" -> parent should be "/"
    let p = PathBuf::from("/hello.txt");
    let parent = p.parent().expect("should have parent");
    let parent_str = parent.to_string_lossy();
    
    // On Windows, parent of "/hello.txt" is "/" or "\"
    assert!(
        parent_str == "/" || parent_str == "\\",
        "parent of '/hello.txt' should be root, got: '{}'",
        parent_str
    );
}

#[test]
fn pathbuf_comparison_forward_vs_backslash() {
    // This is a critical test: on Windows, are "/usr/bin" and "\\usr\\bin" 
    // considered equal by PathBuf?
    let forward = PathBuf::from("/usr/bin");
    let backward = PathBuf::from("\\usr\\bin");
    
    // Document the actual behavior (may differ by platform)
    if forward == backward {
        eprintln!("INFO: PathBuf considers '/usr/bin' == '\\usr\\bin' on this platform");
    } else {
        eprintln!("INFO: PathBuf considers '/usr/bin' != '\\usr\\bin' on this platform");
        // If they're not equal, our HashMap lookups in build_index must be
        // consistent — we must always use the same separator style.
    }
    
    // Regardless, components should be equivalent
    let fwd_names: Vec<_> = forward.components()
        .filter_map(|c| match c {
            Component::Normal(n) => Some(n.to_string_lossy().to_string()),
            _ => None,
        })
        .collect();
    let bwd_names: Vec<_> = backward.components()
        .filter_map(|c| match c {
            Component::Normal(n) => Some(n.to_string_lossy().to_string()),
            _ => None,
        })
        .collect();
    #[cfg(windows)]
    assert_eq!(fwd_names, bwd_names, "component names should be identical");
    
    #[cfg(not(windows))]
    assert_ne!(fwd_names, bwd_names, "Unix treats backslashes as valid filename characters");
}

#[test]
fn pathbuf_parent_chain_to_root() {
    // Walk up from a deep path and verify we eventually reach root
    let p = PathBuf::from("/a/b/c/d/e");
    let mut current = p.as_path();
    let mut depth = 0;
    
    while let Some(parent) = current.parent() {
        depth += 1;
        if parent == current {
            break; // Reached a fixed point (root on some platforms)
        }
        current = parent;
        if depth > 20 {
            panic!("parent chain didn't terminate after 20 iterations");
        }
    }
    
    // Should have gone through at least 4 parents (e, d, c, b, a, root)
    assert!(depth >= 4, "expected at least 4 parents, got {}", depth);
}

// ═══════════════════════════════════════════════════════════
// resolve_path edge cases using our tiny test fixture
// ═══════════════════════════════════════════════════════════

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

fn test_image_path() -> PathBuf {
    fixtures_dir().join("test.sqsh")
}

fn has_test_fixture() -> bool {
    test_image_path().exists()
}

fn provider() -> Option<SquashFsProvider> {
    if !has_test_fixture() {
        eprintln!("SKIP: test fixture not found");
        return None;
    }
    SquashFsProvider::open(&test_image_path()).ok()
}

#[test]
fn resolve_empty_path_is_root() {
    let Some(p) = provider() else { return };
    assert_eq!(p.resolve_path(Path::new("")).unwrap(), Some(ROOT_INODE));
}

#[test]
fn resolve_dot_is_root() {
    let Some(p) = provider() else { return };
    assert_eq!(p.resolve_path(Path::new(".")).unwrap(), Some(ROOT_INODE));
}

#[test]
fn resolve_slash_is_root() {
    let Some(p) = provider() else { return };
    assert_eq!(p.resolve_path(Path::new("/")).unwrap(), Some(ROOT_INODE));
}

#[test]
fn resolve_backslash_is_root() {
    let Some(p) = provider() else { return };
    assert_eq!(p.resolve_path(Path::new("\\")).unwrap(), Some(ROOT_INODE));
}

#[test]
fn resolve_dot_slash_is_root() {
    let Some(p) = provider() else { return };
    assert_eq!(p.resolve_path(Path::new("./")).unwrap(), Some(ROOT_INODE));
}

#[test]
fn resolve_with_leading_slash() {
    let Some(p) = provider() else { return };
    let result = p.resolve_path(Path::new("/hello.txt")).unwrap();
    assert!(result.is_some(), "/hello.txt should resolve");
}

#[test]
fn resolve_with_trailing_slash() {
    let Some(p) = provider() else { return };
    let result = p.resolve_path(Path::new("subdir/")).unwrap();
    assert!(result.is_some(), "subdir/ (trailing slash) should resolve");
}

#[test]
fn resolve_with_double_slash() {
    let Some(p) = provider() else { return };
    let result = p.resolve_path(Path::new("subdir//nested.txt")).unwrap();
    // Double slash should be treated as single separator
    assert!(result.is_some(), "subdir//nested.txt should resolve");
}

#[test]
fn resolve_with_backslash_separator() {
    let Some(p) = provider() else { return };
    let fwd = p.resolve_path(Path::new("subdir/nested.txt")).unwrap();
    let bwd = p.resolve_path(Path::new("subdir\\nested.txt")).unwrap();
    assert_eq!(fwd, bwd, "forward and backslash paths should resolve to same inode");
    assert!(fwd.is_some());
}

#[test]
fn resolve_with_dot_component() {
    let Some(p) = provider() else { return };
    let direct = p.resolve_path(Path::new("subdir/nested.txt")).unwrap();
    let dotted = p.resolve_path(Path::new("./subdir/./nested.txt")).unwrap();
    assert_eq!(direct, dotted, ". components should be ignored");
}

#[test]
fn resolve_nonexistent_returns_none_not_error() {
    let Some(p) = provider() else { return };
    // This should return Ok(None), not Err
    let result = p.resolve_path(Path::new("does/not/exist/at/all"));
    assert!(result.is_ok(), "nonexistent deep path should not error");
    assert_eq!(result.unwrap(), None);
}

#[test]
fn resolve_file_as_directory_returns_none() {
    let Some(p) = provider() else { return };
    // hello.txt is a file, not a directory — trying to traverse into it
    // should return None, not an error
    let result = p.resolve_path(Path::new("hello.txt/something"));
    assert!(result.is_ok(), "traversing into a file should not error");
    assert_eq!(result.unwrap(), None);
}

#[test]
fn resolve_deep_nesting() {
    let Some(p) = provider() else { return };
    let result = p.resolve_path(Path::new("subdir/deep/level2.txt")).unwrap();
    assert!(result.is_some(), "subdir/deep/level2.txt should resolve");
}

#[test]
fn resolve_deep_nesting_with_backslash() {
    let Some(p) = provider() else { return };
    let result = p.resolve_path(Path::new("subdir\\deep\\level2.txt")).unwrap();
    assert!(result.is_some(), "subdir\\deep\\level2.txt should resolve");
}

#[test]
fn lookup_returns_correct_type_for_symlink() {
    let Some(p) = provider() else { return };
    let entry = p.lookup(ROOT_INODE, "link.txt").unwrap();
    assert!(entry.is_some());
    let entry = entry.unwrap();
    assert_eq!(entry.attributes.entry_type, EntryType::Symlink);
}

#[test]
fn all_root_children_resolvable_by_name() {
    let Some(p) = provider() else { return };
    let batch = p.list_directory(ROOT_INODE, 0).unwrap();
    
    for entry in &batch.entries {
        let resolved = p.resolve_path(Path::new(&entry.name)).unwrap();
        assert_eq!(
            resolved,
            Some(entry.attributes.inode),
            "resolve_path('{}') should return inode {}, got {:?}",
            entry.name,
            entry.attributes.inode,
            resolved
        );
    }
}

#[test]
fn all_subdir_children_resolvable_by_full_path() {
    let Some(p) = provider() else { return };
    let subdir_inode = p.resolve_path(Path::new("subdir")).unwrap()
        .expect("subdir should exist");
    let batch = p.list_directory(subdir_inode, 0).unwrap();
    
    for entry in &batch.entries {
        let full_path = format!("subdir/{}", entry.name);
        let resolved = p.resolve_path(Path::new(&full_path)).unwrap();
        assert_eq!(
            resolved,
            Some(entry.attributes.inode),
            "resolve_path('{}') should return inode {}, got {:?}",
            full_path,
            entry.attributes.inode,
            resolved
        );
    }
}

#[test]
fn read_file_every_root_file_matches_size() {
    let Some(p) = provider() else { return };
    let batch = p.list_directory(ROOT_INODE, 0).unwrap();
    
    for entry in &batch.entries {
        if entry.attributes.is_file() {
            let data = p.read_file(entry.attributes.inode, 0, entry.attributes.size).unwrap();
            assert_eq!(
                data.len() as u64,
                entry.attributes.size,
                "file '{}' read length should match attributes.size",
                entry.name
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════
// Ubuntu-specific path tests (feature-gated)
// ═══════════════════════════════════════════════════════════

#[cfg(feature = "ubuntu_stress")]
mod ubuntu_paths {
    use super::*;

    fn ubuntu_image_path() -> Option<PathBuf> {
        if let Ok(path) = std::env::var("SQUASHBOX_UBUNTU_IMAGE") {
            let p = PathBuf::from(path);
            if p.exists() { return Some(p); }
        }
        if let Ok(home) = std::env::var("USERPROFILE") {
            let p = PathBuf::from(home).join("Downloads").join("filesystem.squashfs");
            if p.exists() { return Some(p); }
        }
        None
    }

    fn ubuntu_provider() -> Option<SquashFsProvider> {
        let path = ubuntu_image_path()?;
        SquashFsProvider::open(&path).ok()
    }

    #[test]
    fn ubuntu_every_root_entry_resolvable() {
        let Some(p) = ubuntu_provider() else { return };
        let batch = p.list_directory(ROOT_INODE, 0).unwrap();
        
        for entry in &batch.entries {
            let resolved = p.resolve_path(Path::new(&entry.name)).unwrap();
            assert_eq!(
                resolved,
                Some(entry.attributes.inode),
                "root entry '{}' should be resolvable",
                entry.name
            );
        }
    }

    #[test]
    fn ubuntu_every_second_level_entry_resolvable() {
        let Some(p) = ubuntu_provider() else { return };
        let root_batch = p.list_directory(ROOT_INODE, 0).unwrap();
        let mut checked = 0u32;
        
        for dir_entry in &root_batch.entries {
            if !dir_entry.attributes.is_dir() { continue; }
            
            let mut cookie = 0u64;
            loop {
                let batch = p.list_directory(dir_entry.attributes.inode, cookie).unwrap();
                for child in &batch.entries {
                    let full_path = PathBuf::from(&dir_entry.name).join(&child.name);
                    let resolved = p.resolve_path(&full_path).unwrap();
                    assert_eq!(
                        resolved,
                        Some(child.attributes.inode),
                        "path '{}' should resolve to inode {}",
                        full_path.display(),
                        child.attributes.inode
                    );
                    checked += 1;
                }
                if batch.next_cookie == 0 { break; }
                cookie = batch.next_cookie;
            }
        }
        
        eprintln!("  Verified {} second-level path resolutions", checked);
        assert!(checked > 100, "should check many paths, got {}", checked);
    }

    #[test]
    fn ubuntu_deep_path_usr_share_doc() {
        let Some(p) = ubuntu_provider() else { return };
        // /usr/share/doc is very common in Ubuntu
        let result = p.resolve_path(Path::new("usr/share/doc")).unwrap();
        assert!(result.is_some(), "/usr/share/doc should exist");
        
        let inode = result.unwrap();
        let attrs = p.get_attributes(inode).unwrap();
        assert!(attrs.is_dir(), "/usr/share/doc should be a directory");
    }

    #[test]
    fn ubuntu_backslash_path_equivalence() {
        let Some(p) = ubuntu_provider() else { return };

        // Test many paths with both separator styles
        let test_paths = [
            ("usr/bin", "usr\\bin"),
            ("usr/share", "usr\\share"),
            ("etc/passwd", "etc\\passwd"),
            ("usr/lib", "usr\\lib"),
        ];
        
        for (forward, backward) in &test_paths {
            let fwd = p.resolve_path(Path::new(forward)).unwrap();
            let bwd = p.resolve_path(Path::new(backward)).unwrap();
            assert_eq!(
                fwd, bwd,
                "'{}' and '{}' should resolve to the same inode",
                forward, backward
            );
            assert!(fwd.is_some(), "'{}' should exist", forward);
        }
    }

    #[test]
    fn ubuntu_read_every_file_in_etc() {
        let Some(p) = ubuntu_provider() else { return };
        let etc = match p.resolve_path(Path::new("etc")).unwrap() {
            Some(id) => id,
            None => return,
        };
        
        let mut files_read = 0u32;
        let mut cookie = 0u64;
        loop {
            let batch = p.list_directory(etc, cookie).unwrap();
            for entry in &batch.entries {
                if entry.attributes.is_file() && entry.attributes.size > 0 && entry.attributes.size < 1_000_000 {
                    let data = p.read_file(entry.attributes.inode, 0, entry.attributes.size).unwrap();
                    assert_eq!(
                        data.len() as u64,
                        entry.attributes.size,
                        "/etc/{} size mismatch: got {} expected {}",
                        entry.name, data.len(), entry.attributes.size
                    );
                    files_read += 1;
                }
            }
            if batch.next_cookie == 0 { break; }
            cookie = batch.next_cookie;
        }
        
        eprintln!("  Successfully read {} files from /etc", files_read);
        assert!(files_read > 10, "/etc should have many readable files, got {}", files_read);
    }

    #[test]
    fn ubuntu_node_index_consistency() {
        // Verify that backhand_node_index correctly resolves the same data
        // that path-based lookup would find
        let Some(p) = ubuntu_provider() else { return };
        
        // Read /etc/passwd via both resolve_path+read_file
        let inode = p.resolve_path(Path::new("etc/passwd")).unwrap()
            .expect("/etc/passwd should exist");
        
        let data1 = p.read_file(inode, 0, 100).unwrap();
        // Read again - should get identical data (tests index stability)
        let data2 = p.read_file(inode, 0, 100).unwrap();
        assert_eq!(data1, data2, "repeated reads should be identical");
        
        // Read with offset should be consistent
        let full = p.read_file(inode, 0, 1_000_000).unwrap();
        let partial = p.read_file(inode, 10, 50).unwrap();
        assert_eq!(
            partial,
            &full[10..60],
            "offset read should match full read slice"
        );
    }
}
