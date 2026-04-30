use super::*;

async fn make_server(workspace: &Path) -> CodepalServer {
    let opts = Opts {
        workspace: workspace.to_path_buf(),
        read_path_allow_list: vec![],
        no_auto_path_allow: false,
        enable_compressed: false,
        dump_memory: false,
    };
    CodepalServer::new(&opts)
        .await
        .expect("server creation failed")
}

// ---- ls_dir tests ----

#[tokio::test]
async fn ls_dir_returns_sorted_entries() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("b.txt"), "").await.unwrap();
    fs::write(dir.path().join("a.txt"), "").await.unwrap();
    fs::create_dir(dir.path().join("subdir")).await.unwrap();

    let server = make_server(dir.path()).await;
    let result = server
        .ls_dir(Parameters(LsDirParams {
            path: dir.path().to_str().unwrap().to_string(),
        }))
        .await
        .unwrap();
    assert_eq!(result.0.entries, vec!["a.txt", "b.txt", "subdir/"]);
}

#[tokio::test]
async fn ls_dir_on_file_errors() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("file.txt");
    fs::write(&file, "hi").await.unwrap();

    let server = make_server(dir.path()).await;
    let err = server
        .ls_dir(Parameters(LsDirParams {
            path: file.to_str().unwrap().to_string(),
        }))
        .await
        .err()
        .expect("expected Err");
    assert!(err.message.contains("Not a directory"));
}

#[tokio::test]
async fn ls_dir_outside_allowlist_errors() {
    let dir = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();

    let server = make_server(dir.path()).await;
    let err = server
        .ls_dir(Parameters(LsDirParams {
            path: other.path().to_str().unwrap().to_string(),
        }))
        .await
        .err()
        .expect("expected Err");
    assert!(err.message.contains("EPERM"));
}

// ---- read_file tests ----

#[tokio::test]
async fn read_file_entire_content() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("hello.txt");
    fs::write(&file, "line1\nline2\nline3\n").await.unwrap();

    let server = make_server(dir.path()).await;
    let content = server
        .read_file(Parameters(ReadFileParams {
            path: file.to_str().unwrap().to_string(),
            start_line: None,
            end_line: None,
        }))
        .await
        .unwrap();
    assert_eq!(content, "line1\nline2\nline3");
}

#[tokio::test]
async fn read_file_with_start_line() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("hello.txt");
    fs::write(&file, "line1\nline2\nline3\n").await.unwrap();

    let server = make_server(dir.path()).await;
    let content = server
        .read_file(Parameters(ReadFileParams {
            path: file.to_str().unwrap().to_string(),
            start_line: Some(2),
            end_line: None,
        }))
        .await
        .unwrap();
    assert_eq!(content, "line2\nline3");
}

#[tokio::test]
async fn read_file_with_end_line() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("hello.txt");
    fs::write(&file, "line1\nline2\nline3\n").await.unwrap();

    let server = make_server(dir.path()).await;
    let content = server
        .read_file(Parameters(ReadFileParams {
            path: file.to_str().unwrap().to_string(),
            start_line: None,
            end_line: Some(2),
        }))
        .await
        .unwrap();
    assert_eq!(content, "line1\nline2");
}

#[tokio::test]
async fn read_file_with_line_range() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("hello.txt");
    fs::write(&file, "line1\nline2\nline3\nline4\n")
        .await
        .unwrap();

    let server = make_server(dir.path()).await;
    let content = server
        .read_file(Parameters(ReadFileParams {
            path: file.to_str().unwrap().to_string(),
            start_line: Some(2),
            end_line: Some(3),
        }))
        .await
        .unwrap();
    assert_eq!(content, "line2\nline3");
}

#[tokio::test]
async fn read_file_on_directory_errors() {
    let dir = tempfile::tempdir().unwrap();

    let server = make_server(dir.path()).await;
    let err = server
        .read_file(Parameters(ReadFileParams {
            path: dir.path().to_str().unwrap().to_string(),
            start_line: None,
            end_line: None,
        }))
        .await
        .unwrap_err();
    assert!(err.message.contains("Not a file"));
}

#[tokio::test]
async fn read_file_too_large_errors() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("big.bin");
    fs::write(&file, vec![b'x'; (READ_FILE_MAX_SIZE + 1) as usize])
        .await
        .unwrap();

    let server = make_server(dir.path()).await;
    let err = server
        .read_file(Parameters(ReadFileParams {
            path: file.to_str().unwrap().to_string(),
            start_line: None,
            end_line: None,
        }))
        .await
        .unwrap_err();
    assert!(err.message.contains("File too large"));
}

#[tokio::test]
async fn read_file_outside_allowlist_errors() {
    let dir = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let file = other.path().join("secret.txt");
    fs::write(&file, "secret").await.unwrap();

    let server = make_server(dir.path()).await;
    let err = server
        .read_file(Parameters(ReadFileParams {
            path: file.to_str().unwrap().to_string(),
            start_line: None,
            end_line: None,
        }))
        .await
        .unwrap_err();
    assert!(err.message.contains("EPERM"));
}

// ---- grep_file tests ----

#[tokio::test]
async fn grep_file_basic_match() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("code.txt");
    fs::write(&file, "foo bar\nbaz\nfoo qux\n").await.unwrap();

    let server = make_server(dir.path()).await;
    let result = server
        .grep_file(Parameters(GrepFileParams {
            path: file.to_str().unwrap().to_string(),
            pattern: "foo".to_string(),
            case_insensitive: None,
            dot_matches_newline: None,
            context_lines: None,
        }))
        .await
        .unwrap();
    assert!(result.contains("1:foo bar"));
    assert!(result.contains("3:foo qux"));
    assert!(!result.contains("baz"));
}

#[tokio::test]
async fn grep_file_case_insensitive() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("code.txt");
    fs::write(&file, "Hello World\nhello world\nGOODBYE\n")
        .await
        .unwrap();

    let server = make_server(dir.path()).await;
    let result = server
        .grep_file(Parameters(GrepFileParams {
            path: file.to_str().unwrap().to_string(),
            pattern: "hello".to_string(),
            case_insensitive: Some(true),
            dot_matches_newline: None,
            context_lines: None,
        }))
        .await
        .unwrap();
    assert!(result.contains("Hello World"));
    assert!(result.contains("hello world"));
    assert!(!result.contains("GOODBYE"));
}

#[tokio::test]
async fn grep_file_no_match_returns_empty() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("code.txt");
    fs::write(&file, "foo bar\nbaz\n").await.unwrap();

    let server = make_server(dir.path()).await;
    let result = server
        .grep_file(Parameters(GrepFileParams {
            path: file.to_str().unwrap().to_string(),
            pattern: "zzz".to_string(),
            case_insensitive: None,
            dot_matches_newline: None,
            context_lines: None,
        }))
        .await
        .unwrap();
    assert_eq!(result, "");
}

#[tokio::test]
async fn grep_file_context_lines() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("code.txt");
    fs::write(&file, "before\nmatch\nafter\n").await.unwrap();

    let server = make_server(dir.path()).await;
    let result = server
        .grep_file(Parameters(GrepFileParams {
            path: file.to_str().unwrap().to_string(),
            pattern: "match".to_string(),
            case_insensitive: None,
            dot_matches_newline: None,
            context_lines: Some(1),
        }))
        .await
        .unwrap();
    assert!(result.contains("1-before"));
    assert!(result.contains("2:match"));
    assert!(result.contains("3-after"));
}

#[tokio::test]
async fn grep_file_invalid_regex_errors() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("code.txt");
    fs::write(&file, "some content\n").await.unwrap();

    let server = make_server(dir.path()).await;
    let err = server
        .grep_file(Parameters(GrepFileParams {
            path: file.to_str().unwrap().to_string(),
            pattern: "[invalid".to_string(),
            case_insensitive: None,
            dot_matches_newline: None,
            context_lines: None,
        }))
        .await
        .unwrap_err();
    assert!(err.message.contains("Invalid regex"));
}

#[tokio::test]
async fn grep_file_outside_allowlist_errors() {
    let dir = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let file = other.path().join("secret.txt");
    fs::write(&file, "secret").await.unwrap();

    let server = make_server(dir.path()).await;
    let err = server
        .grep_file(Parameters(GrepFileParams {
            path: file.to_str().unwrap().to_string(),
            pattern: "secret".to_string(),
            case_insensitive: None,
            dot_matches_newline: None,
            context_lines: None,
        }))
        .await
        .unwrap_err();
    assert!(err.message.contains("EPERM"));
}

// ---- grep_dir tests ----

#[tokio::test]
async fn grep_dir_basic_match() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), "hello world\nfoo bar\n")
        .await
        .unwrap();
    fs::write(dir.path().join("b.txt"), "no match here\n")
        .await
        .unwrap();

    let server = make_server(dir.path()).await;
    let result = server
        .grep_dir(Parameters(GrepDirParams {
            path: dir.path().to_str().unwrap().to_string(),
            pattern: "hello".to_string(),
            case_insensitive: None,
            dot_matches_newline: None,
            context_lines: None,
        }))
        .await
        .unwrap();
    assert!(result.contains("a.txt"));
    assert!(result.contains("1:hello world"));
    assert!(!result.contains("b.txt"));
}

#[tokio::test]
async fn grep_dir_case_insensitive() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), "Hello World\n")
        .await
        .unwrap();

    let server = make_server(dir.path()).await;
    let result = server
        .grep_dir(Parameters(GrepDirParams {
            path: dir.path().to_str().unwrap().to_string(),
            pattern: "hello".to_string(),
            case_insensitive: Some(true),
            dot_matches_newline: None,
            context_lines: None,
        }))
        .await
        .unwrap();
    assert!(result.contains("Hello World"));
}

#[tokio::test]
async fn grep_dir_no_match_returns_empty() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), "foo bar\n").await.unwrap();

    let server = make_server(dir.path()).await;
    let result = server
        .grep_dir(Parameters(GrepDirParams {
            path: dir.path().to_str().unwrap().to_string(),
            pattern: "zzz".to_string(),
            case_insensitive: None,
            dot_matches_newline: None,
            context_lines: None,
        }))
        .await
        .unwrap();
    assert_eq!(result, "");
}

#[tokio::test]
async fn grep_dir_context_lines() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), "before\nmatch\nafter\n")
        .await
        .unwrap();

    let server = make_server(dir.path()).await;
    let result = server
        .grep_dir(Parameters(GrepDirParams {
            path: dir.path().to_str().unwrap().to_string(),
            pattern: "match".to_string(),
            case_insensitive: None,
            dot_matches_newline: None,
            context_lines: Some(1),
        }))
        .await
        .unwrap();
    assert!(result.contains("1-before"));
    assert!(result.contains("2:match"));
    assert!(result.contains("3-after"));
}

#[tokio::test]
async fn grep_dir_invalid_regex_errors() {
    let dir = tempfile::tempdir().unwrap();

    let server = make_server(dir.path()).await;
    let err = server
        .grep_dir(Parameters(GrepDirParams {
            path: dir.path().to_str().unwrap().to_string(),
            pattern: "[invalid".to_string(),
            case_insensitive: None,
            dot_matches_newline: None,
            context_lines: None,
        }))
        .await
        .unwrap_err();
    assert!(err.message.contains("Invalid regex"));
}

#[tokio::test]
async fn grep_dir_on_file_errors() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("file.txt");
    fs::write(&file, "content\n").await.unwrap();

    let server = make_server(dir.path()).await;
    let err = server
        .grep_dir(Parameters(GrepDirParams {
            path: file.to_str().unwrap().to_string(),
            pattern: "content".to_string(),
            case_insensitive: None,
            dot_matches_newline: None,
            context_lines: None,
        }))
        .await
        .unwrap_err();
    assert!(err.message.contains("Not a directory"));
}

#[tokio::test]
async fn grep_dir_outside_allowlist_errors() {
    let dir = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();

    let server = make_server(dir.path()).await;
    let err = server
        .grep_dir(Parameters(GrepDirParams {
            path: other.path().to_str().unwrap().to_string(),
            pattern: "foo".to_string(),
            case_insensitive: None,
            dot_matches_newline: None,
            context_lines: None,
        }))
        .await
        .unwrap_err();
    assert!(err.message.contains("EPERM"));
}

// ---- find_files tests ----

#[tokio::test]
async fn find_files_basic() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("main.rs"), "").await.unwrap();
    fs::write(dir.path().join("lib.rs"), "").await.unwrap();
    fs::write(dir.path().join("readme.txt"), "").await.unwrap();

    let server = make_server(dir.path()).await;
    let result = server
        .find_files(Parameters(FindFilesParams {
            path: dir.path().to_str().unwrap().to_string(),
            pattern: r"\.rs$".to_string(),
            case_insensitive: Some(false),
        }))
        .await
        .unwrap();
    assert_eq!(result.0.files.len(), 2);
    assert!(result.0.files.iter().any(|f| f.contains("main.rs")));
    assert!(result.0.files.iter().any(|f| f.contains("lib.rs")));
    assert!(!result.0.files.iter().any(|f| f.contains("readme.txt")));
}

#[tokio::test]
async fn find_files_case_insensitive_default() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("Main.RS"), "").await.unwrap();

    let server = make_server(dir.path()).await;
    let result = server
        .find_files(Parameters(FindFilesParams {
            path: dir.path().to_str().unwrap().to_string(),
            pattern: r"\.rs$".to_string(),
            case_insensitive: None, // defaults to true
        }))
        .await
        .unwrap();
    assert_eq!(result.0.files.len(), 1);
}

#[tokio::test]
async fn find_files_no_match() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("main.rs"), "").await.unwrap();

    let server = make_server(dir.path()).await;
    let result = server
        .find_files(Parameters(FindFilesParams {
            path: dir.path().to_str().unwrap().to_string(),
            pattern: r"\.py$".to_string(),
            case_insensitive: None,
        }))
        .await
        .unwrap();
    assert!(result.0.files.is_empty());
}

#[tokio::test]
async fn find_files_invalid_regex_errors() {
    let dir = tempfile::tempdir().unwrap();

    let server = make_server(dir.path()).await;
    let err = server
        .find_files(Parameters(FindFilesParams {
            path: dir.path().to_str().unwrap().to_string(),
            pattern: "[invalid".to_string(),
            case_insensitive: None,
        }))
        .await
        .err()
        .expect("expected Err");
    assert!(err.message.contains("Invalid regex"));
}

#[tokio::test]
async fn find_files_on_file_errors() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("file.txt");
    fs::write(&file, "").await.unwrap();

    let server = make_server(dir.path()).await;
    let err = server
        .find_files(Parameters(FindFilesParams {
            path: file.to_str().unwrap().to_string(),
            pattern: ".*".to_string(),
            case_insensitive: None,
        }))
        .await
        .err()
        .expect("expected Err");
    assert!(err.message.contains("Not a directory"));
}

#[tokio::test]
async fn find_files_outside_allowlist_errors() {
    let dir = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();

    let server = make_server(dir.path()).await;
    let err = server
        .find_files(Parameters(FindFilesParams {
            path: other.path().to_str().unwrap().to_string(),
            pattern: ".*".to_string(),
            case_insensitive: None,
        }))
        .await
        .err()
        .expect("expected Err");
    assert!(err.message.contains("EPERM"));
}

// ---- memory tests ----

#[tokio::test]
async fn memory_store_and_load_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let server = make_server(dir.path()).await;

    let store_result = server
        .memory_store(Parameters(MemoryStoreParams {
            keys: vec!["mykey".to_string()],
            value: "myvalue".to_string(),
        }))
        .await
        .unwrap();
    assert!(store_result.0.success);

    let load_result = server
        .memory_load(Parameters(MemoryLoadParams {
            keys: vec!["mykey".to_string()],
        }))
        .await
        .unwrap();
    assert_eq!(load_result.0.value, Some("myvalue".to_string()));
}

#[tokio::test]
async fn memory_load_returns_none_when_empty() {
    let dir = tempfile::tempdir().unwrap();
    let server = make_server(dir.path()).await;

    let result = server
        .memory_load(Parameters(MemoryLoadParams {
            keys: vec!["nonexistent".to_string()],
        }))
        .await
        .unwrap();
    assert_eq!(result.0.value, None);
}

#[tokio::test]
async fn memory_load_first_key_wins() {
    let dir = tempfile::tempdir().unwrap();
    let server = make_server(dir.path()).await;

    server
        .memory_store(Parameters(MemoryStoreParams {
            keys: vec!["key-a".to_string()],
            value: "value-a".to_string(),
        }))
        .await
        .unwrap();
    server
        .memory_store(Parameters(MemoryStoreParams {
            keys: vec!["key-b".to_string()],
            value: "value-b".to_string(),
        }))
        .await
        .unwrap();

    let result = server
        .memory_load(Parameters(MemoryLoadParams {
            keys: vec!["key-a".to_string(), "key-b".to_string()],
        }))
        .await
        .unwrap();
    assert_eq!(result.0.value, Some("value-a".to_string()));
}

#[tokio::test]
async fn memory_store_empty_keys_errors() {
    let dir = tempfile::tempdir().unwrap();
    let server = make_server(dir.path()).await;

    let err = server
        .memory_store(Parameters(MemoryStoreParams {
            keys: vec![],
            value: "value".to_string(),
        }))
        .await
        .err()
        .expect("expected Err");
    assert!(err.message.contains("keys must not be empty"));
}

#[tokio::test]
async fn memory_store_too_many_keys_errors() {
    let dir = tempfile::tempdir().unwrap();
    let server = make_server(dir.path()).await;

    let keys: Vec<String> = (0..=MEMORY_MAX_KEYS).map(|i| format!("key{i}")).collect();
    let err = server
        .memory_store(Parameters(MemoryStoreParams {
            keys,
            value: "value".to_string(),
        }))
        .await
        .err()
        .expect("expected Err");
    assert!(err.message.contains("too many keys"));
}

#[tokio::test]
async fn memory_store_key_too_long_errors() {
    let dir = tempfile::tempdir().unwrap();
    let server = make_server(dir.path()).await;

    let long_key = "k".repeat(MEMORY_MAX_KEY_LEN + 1);
    let err = server
        .memory_store(Parameters(MemoryStoreParams {
            keys: vec![long_key],
            value: "value".to_string(),
        }))
        .await
        .err()
        .expect("expected Err");
    assert!(err.message.contains("key too long"));
}

#[tokio::test]
async fn memory_store_value_too_long_errors() {
    let dir = tempfile::tempdir().unwrap();
    let server = make_server(dir.path()).await;

    let long_value = "v".repeat(MEMORY_MAX_VALUE_LEN + 1);
    let err = server
        .memory_store(Parameters(MemoryStoreParams {
            keys: vec!["mykey".to_string()],
            value: long_value,
        }))
        .await
        .err()
        .expect("expected Err");
    assert!(err.message.contains("value too long"));
}

#[tokio::test]
async fn memory_store_overwrites_existing() {
    let dir = tempfile::tempdir().unwrap();
    let server = make_server(dir.path()).await;

    server
        .memory_store(Parameters(MemoryStoreParams {
            keys: vec!["k".to_string()],
            value: "old".to_string(),
        }))
        .await
        .unwrap();
    server
        .memory_store(Parameters(MemoryStoreParams {
            keys: vec!["k".to_string()],
            value: "new".to_string(),
        }))
        .await
        .unwrap();

    let result = server
        .memory_load(Parameters(MemoryLoadParams {
            keys: vec!["k".to_string()],
        }))
        .await
        .unwrap();
    assert_eq!(result.0.value, Some("new".to_string()));
}

#[tokio::test]
async fn memory_store_multi_key_all_loadable() {
    let dir = tempfile::tempdir().unwrap();
    let server = make_server(dir.path()).await;

    server
        .memory_store(Parameters(MemoryStoreParams {
            keys: vec!["alpha".to_string(), "beta".to_string()],
            value: "shared".to_string(),
        }))
        .await
        .unwrap();

    for key in ["alpha", "beta"] {
        let result = server
            .memory_load(Parameters(MemoryLoadParams {
                keys: vec![key.to_string()],
            }))
            .await
            .unwrap();
        assert_eq!(result.0.value, Some("shared".to_string()), "key={key}");
    }
}

#[tokio::test]
async fn memory_list_empty_store() {
    let dir = tempfile::tempdir().unwrap();
    let server = make_server(dir.path()).await;

    let result = server.memory_list().await.unwrap();
    assert!(result.0.keys.is_empty());
}

#[tokio::test]
async fn memory_list_after_storing() {
    let dir = tempfile::tempdir().unwrap();
    let server = make_server(dir.path()).await;

    server
        .memory_store(Parameters(MemoryStoreParams {
            keys: vec!["k1".to_string(), "k2".to_string()],
            value: "val".to_string(),
        }))
        .await
        .unwrap();

    let result = server.memory_list().await.unwrap();
    let mut keys = result.0.keys.clone();
    keys.sort();
    assert_eq!(keys, vec!["k1", "k2"]);
}

#[tokio::test]
async fn memory_delete_existing_key() {
    let dir = tempfile::tempdir().unwrap();
    let server = make_server(dir.path()).await;

    server
        .memory_store(Parameters(MemoryStoreParams {
            keys: vec!["mykey".to_string()],
            value: "myvalue".to_string(),
        }))
        .await
        .unwrap();

    let del_result = server
        .memory_delete(Parameters(MemoryDeleteParams {
            key: "mykey".to_string(),
        }))
        .await
        .unwrap();
    assert!(del_result.0.found);

    let load_result = server
        .memory_load(Parameters(MemoryLoadParams {
            keys: vec!["mykey".to_string()],
        }))
        .await
        .unwrap();
    assert_eq!(load_result.0.value, None);
}

#[tokio::test]
async fn memory_delete_nonexistent_key() {
    let dir = tempfile::tempdir().unwrap();
    let server = make_server(dir.path()).await;

    let result = server
        .memory_delete(Parameters(MemoryDeleteParams {
            key: "ghost".to_string(),
        }))
        .await
        .unwrap();
    assert!(!result.0.found);
}
