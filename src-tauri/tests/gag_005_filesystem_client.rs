use grok_acp_gui_lib::adapters::filesystem::WorkspaceFilesystem;

fn fixture_root(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("gag005-{label}-{}", uuid::Uuid::new_v4()))
}

#[test]
fn workspace_filesystem_reads_only_requested_lines_inside_root() {
    let root = fixture_root("fs-read");
    std::fs::create_dir_all(&root).expect("create fixture root");
    let file = root.join("README.txt");
    std::fs::write(&file, "one\ntwo\nthree\n").expect("write fixture");

    let filesystem = WorkspaceFilesystem::new(&root).expect("valid workspace root");
    let content = filesystem
        .read_text_file(&file, Some(2), Some(1))
        .expect("read inside workspace");

    assert_eq!(content, "two");
    std::fs::remove_dir_all(&root).expect("remove fixture root");
}

#[test]
fn workspace_filesystem_rejects_paths_outside_root() {
    let root = fixture_root("fs-root");
    let sibling = fixture_root("fs-sibling");
    std::fs::create_dir_all(&root).expect("create fixture root");
    std::fs::create_dir_all(&sibling).expect("create sibling root");
    let outside = sibling.join("secret.txt");
    std::fs::write(&outside, "must not be read").expect("write sibling fixture");

    let filesystem = WorkspaceFilesystem::new(&root).expect("valid workspace root");
    let result = filesystem.read_text_file(&outside, None, None);

    assert!(result.is_err(), "workspace escape must fail closed");
    std::fs::remove_dir_all(&root).expect("remove fixture root");
    std::fs::remove_dir_all(&sibling).expect("remove sibling root");
}

#[test]
fn workspace_filesystem_rejects_oversized_files() {
    let root = fixture_root("fs-large");
    std::fs::create_dir_all(&root).expect("create fixture root");
    let file = root.join("large.txt");
    std::fs::write(&file, vec![b'x'; 1_048_577]).expect("write oversized fixture");

    let filesystem = WorkspaceFilesystem::new(&root).expect("valid workspace root");
    let result = filesystem.read_text_file(&file, None, None);

    assert!(result.is_err(), "oversized reads must fail closed");
    std::fs::remove_dir_all(&root).expect("remove fixture root");
}
