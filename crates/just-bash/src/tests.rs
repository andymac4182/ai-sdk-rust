use super::*;
use std::collections::BTreeMap;

fn utf8(text: &str) -> Vec<u8> {
    text.as_bytes().to_vec()
}

#[test]
fn upstream_interface_contract_reads_writes_appends_stats_lists_and_removes_files() {
    let mut fs = VirtualFileSystem::new();
    fs.mkdir("/docs", MkdirOptions { recursive: true }).unwrap();
    fs.write_file("/docs/readme.md", "hello").unwrap();
    fs.append_file("/docs/readme.md", " world").unwrap();

    assert_eq!(fs.read_file("/docs/readme.md").unwrap(), "hello world");
    assert!(fs.exists("/docs/readme.md"));
    assert!(fs.stat("/docs/readme.md").unwrap().is_file);
    assert!(
        fs.readdir("/docs")
            .unwrap()
            .contains(&"readme.md".to_string())
    );

    fs.rm("/docs/readme.md", RmOptions::default()).unwrap();
    assert!(!fs.exists("/docs/readme.md"));
}

#[test]
fn upstream_interface_contract_copies_and_moves_without_content_changes() {
    let mut fs = VirtualFileSystem::new();
    fs.mkdir("/tmp", MkdirOptions { recursive: true }).unwrap();
    fs.write_file("/tmp/source.txt", "contents").unwrap();
    fs.cp("/tmp/source.txt", "/tmp/copy.txt", CpOptions::default())
        .unwrap();
    fs.mv("/tmp/copy.txt", "/tmp/moved.txt").unwrap();

    assert_eq!(fs.read_file("/tmp/source.txt").unwrap(), "contents");
    assert_eq!(fs.read_file("/tmp/moved.txt").unwrap(), "contents");
    assert!(!fs.exists("/tmp/copy.txt"));
}

#[test]
fn upstream_interface_contract_rejects_null_byte_paths() {
    let mut fs = VirtualFileSystem::new();
    assert!(
        fs.read_file("/evil\0.txt")
            .unwrap_err()
            .to_string()
            .contains("null byte")
    );
    assert!(
        fs.write_file("/evil\0.txt", "data")
            .unwrap_err()
            .to_string()
            .contains("null byte")
    );
    assert!(
        fs.mkdir("/evil\0dir", MkdirOptions::default())
            .unwrap_err()
            .to_string()
            .contains("null byte")
    );
    assert!(
        fs.rm("/evil\0.txt", RmOptions::default())
            .unwrap_err()
            .to_string()
            .contains("null byte")
    );
    assert!(!fs.exists("/evil\0.txt"));
}

#[test]
fn upstream_interface_contract_clamps_traversal_above_root() {
    let mut fs = VirtualFileSystem::new();
    fs.write_file("/root.txt", "root").unwrap();
    assert_eq!(fs.read_file("/../../root.txt").unwrap(), "root");
    assert!(fs.stat("/../../").unwrap().is_directory);
    assert_eq!(normalize_path("/a/../../../b"), "/b");
}

#[test]
fn upstream_interface_contract_resolves_relative_paths_consistently() {
    let fs = VirtualFileSystem::new();
    assert_eq!(fs.resolve_path("/work", "file.txt"), "/work/file.txt");
    assert_eq!(fs.resolve_path("/work", "../file.txt"), "/file.txt");
    assert_eq!(fs.resolve_path("/work", "/absolute.txt"), "/absolute.txt");
    assert_eq!(dirname("/work/file.txt"), "/work");
    assert_eq!(join_path("/", "file.txt"), "/file.txt");
}

#[test]
fn upstream_real_fs_utils_normalize_path_cases() {
    assert_eq!(normalize_path(""), "/");
    assert_eq!(normalize_path("/"), "/");
    assert_eq!(normalize_path("/foo/"), "/foo");
    assert_eq!(normalize_path("/./foo/./bar/."), "/foo/bar");
    assert_eq!(normalize_path("/a/b/../c"), "/a/c");
    assert_eq!(normalize_path("/../../.."), "/");
    assert_eq!(normalize_path("///a///b///"), "/a/b");
    assert_eq!(normalize_path("foo/bar"), "/foo/bar");
    assert_eq!(normalize_path("/a/b/c/d/../../../../"), "/");
    assert_eq!(normalize_path("/a/./b/../c/./d/../e"), "/a/c/e");
    assert_eq!(normalize_path("/././././.."), "/");
    assert_eq!(normalize_path("/..."), "/...");
    assert_eq!(normalize_path("/..foo"), "/..foo");
    assert_eq!(normalize_path("/foo.."), "/foo..");
    assert_eq!(normalize_path("/a..b"), "/a..b");
    assert_eq!(
        normalize_path(&format!("/{}", vec!["a"; 200].join("/")))
            .split('/')
            .filter(|part| !part.is_empty())
            .count(),
        200
    );
    assert_eq!(normalize_path("////"), "/");
}

#[test]
fn upstream_real_fs_utils_is_path_within_root_cases() {
    assert!(is_path_within_root("/sandbox", "/sandbox"));
    assert!(is_path_within_root("/sandbox/file.txt", "/sandbox"));
    assert!(is_path_within_root("/sandbox/a/b/c/d", "/sandbox"));
    assert!(!is_path_within_root("/sandboxes", "/sandbox"));
    assert!(!is_path_within_root("/sandbox-evil", "/sandbox"));
    assert!(!is_path_within_root("/sandboxfoo", "/sandbox"));
    assert!(!is_path_within_root("/", "/sandbox"));
    assert!(!is_path_within_root("/etc/passwd", "/sandbox"));
    assert!(is_path_within_root("/", "/"));
    assert!(!is_path_within_root("/anything", "/"));
    assert!(!is_path_within_root("/tmp/datastore", "/tmp/data"));
    assert!(is_path_within_root("/tmp/data/file", "/tmp/data"));
    assert!(is_path_within_root("D:\\project\\file.txt", "D:\\project"));
    assert!(is_path_within_root("D:\\project\\a\\b", "D:\\project"));
    assert!(is_path_within_root("D:\\project", "D:\\project"));
    assert!(!is_path_within_root("D:\\projects", "D:\\project"));
    assert!(!is_path_within_root("D:\\project-evil", "D:\\project"));
}

#[test]
fn upstream_real_fs_utils_validate_path_cases() {
    validate_path("/foo/bar.txt", "open").unwrap();
    validate_path("/a/b/c", "stat").unwrap();
    validate_path("/", "readdir").unwrap();

    assert!(
        validate_path("\0/etc/passwd", "open")
            .unwrap_err()
            .to_string()
            .contains("null byte")
    );
    assert!(
        validate_path("/etc\0/passwd", "open")
            .unwrap_err()
            .to_string()
            .contains("null byte")
    );
    assert!(
        validate_path("/etc/passwd\0", "open")
            .unwrap_err()
            .to_string()
            .contains("null byte")
    );
    assert!(
        validate_path("/bad\0path", "chmod")
            .unwrap_err()
            .to_string()
            .contains("chmod")
    );
    assert!(
        validate_path("/a\0b", "open")
            .unwrap_err()
            .to_string()
            .contains("/a")
    );
}

#[test]
fn upstream_interface_contract_symlinks_keep_absolute_targets_virtual() {
    let mut fs = VirtualFileSystem::new();
    fs.write_file("/target.txt", "target").unwrap();
    fs.symlink("/target.txt", "/link.txt").unwrap();
    assert_eq!(fs.readlink("/link.txt").unwrap(), "/target.txt");
    assert!(fs.lstat("/link.txt").unwrap().is_symbolic_link);
    assert_eq!(fs.read_file("/link.txt").unwrap(), "target");
}

#[test]
fn upstream_in_memory_binary_and_encoding_cases() {
    let mut fs = VirtualFileSystem::new();
    let hello = vec![0x48, 0x65, 0x6c, 0x6c, 0x6f];
    fs.write_file("/binary.bin", hello.clone()).unwrap();
    assert_eq!(fs.read_file_buffer("/binary.bin").unwrap(), hello);
    assert_eq!(fs.read_file("/binary.bin").unwrap(), "Hello");

    fs.write_file("/test.txt", "Hello").unwrap();
    assert_eq!(fs.read_file_buffer("/test.txt").unwrap(), utf8("Hello"));

    let binary = vec![0x00, 0x01, 0x00, 0xff, 0x00];
    fs.write_file("/nulls.bin", binary.clone()).unwrap();
    assert_eq!(fs.read_file_buffer("/nulls.bin").unwrap(), binary);
    assert_eq!(fs.stat("/nulls.bin").unwrap().size, 5);

    fs.write_file_with_encoding("/utf8.txt", "Hello 世界", BufferEncoding::Utf8)
        .unwrap();
    assert_eq!(
        fs.read_file_with_encoding("/utf8.txt", BufferEncoding::Utf8)
            .unwrap(),
        "Hello 世界"
    );
    fs.write_file_with_encoding("/b64.txt", "SGVsbG8=", BufferEncoding::Base64)
        .unwrap();
    assert_eq!(fs.read_file("/b64.txt").unwrap(), "Hello");
    assert_eq!(
        fs.read_file_with_encoding("/b64.txt", BufferEncoding::Base64)
            .unwrap(),
        "SGVsbG8="
    );
    fs.write_file_with_encoding("/hex.txt", "48656c6c6f", BufferEncoding::Hex)
        .unwrap();
    assert_eq!(fs.read_file("/hex.txt").unwrap(), "Hello");
    assert_eq!(
        fs.read_file_with_encoding("/hex.txt", BufferEncoding::Hex)
            .unwrap(),
        "48656c6c6f"
    );
    fs.write_file_with_encoding("/latin1.txt", "café", BufferEncoding::Latin1)
        .unwrap();
    assert_eq!(
        fs.read_file_buffer("/latin1.txt").unwrap(),
        vec![0x63, 0x61, 0x66, 0xe9]
    );

    fs.write_file("/append.txt", "Hello").unwrap();
    fs.append_file("/append.txt", vec![0x20, 0x57, 0x6f, 0x72, 0x6c, 0x64])
        .unwrap();
    fs.append_file_with_encoding("/append.txt", "IQ==", BufferEncoding::Base64)
        .unwrap();
    assert_eq!(fs.read_file("/append.txt").unwrap(), "Hello World!");

    fs.write_file("/empty.bin", Vec::<u8>::new()).unwrap();
    assert!(fs.read_file_buffer("/empty.bin").unwrap().is_empty());

    let large: Vec<u8> = (0..(1024 * 1024))
        .map(|index| (index % 256) as u8)
        .collect();
    fs.write_file("/large.bin", large.clone()).unwrap();
    let read = fs.read_file_buffer("/large.bin").unwrap();
    assert_eq!(read.len(), large.len());
    assert_eq!(read[255], 255);

    fs.cp("/nulls.bin", "/copy.bin", CpOptions::default())
        .unwrap();
    assert_eq!(
        fs.read_file_buffer("/copy.bin").unwrap(),
        vec![0x00, 0x01, 0x00, 0xff, 0x00]
    );
}

#[test]
fn upstream_in_memory_readdir_with_file_types_cases() {
    let mut fs = VirtualFileSystem::new();
    fs.write_file("/dir/b.txt", "b").unwrap();
    fs.write_file("/dir/A.txt", "a").unwrap();
    fs.mkdir("/dir/sub", MkdirOptions::default()).unwrap();
    fs.symlink("/dir/b.txt", "/dir/link").unwrap();

    let entries = fs.readdir_with_file_types("/dir").unwrap();
    let names: Vec<String> = entries.iter().map(|entry| entry.name.clone()).collect();
    assert_eq!(names, vec!["A.txt", "b.txt", "link", "sub"]);
    assert!(
        entries
            .iter()
            .any(|entry| entry.name == "b.txt" && entry.is_file)
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry.name == "sub" && entry.is_directory)
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry.name == "link" && entry.is_symbolic_link)
    );
    assert_eq!(fs.readdir("/dir").unwrap(), names);
    assert_eq!(
        fs.readdir("/missing").unwrap_err().kind(),
        &JustBashErrorKind::NotFound
    );
    assert_eq!(
        fs.readdir("/dir/b.txt").unwrap_err().kind(),
        &JustBashErrorKind::NotDirectory
    );
}

#[test]
fn upstream_in_memory_symlink_path_resolution_cases() {
    let mut fs = VirtualFileSystem::new();
    fs.symlink("/self", "/self").unwrap();
    assert_eq!(
        fs.read_file("/self").unwrap_err().kind(),
        &JustBashErrorKind::SymlinkLoop
    );

    let mut fs = VirtualFileSystem::new();
    fs.symlink("/link2", "/link1").unwrap();
    fs.symlink("/link1", "/link2").unwrap();
    assert_eq!(
        fs.read_file("/link1").unwrap_err().kind(),
        &JustBashErrorKind::SymlinkLoop
    );
    assert_eq!(
        fs.stat("/link1").unwrap_err().kind(),
        &JustBashErrorKind::SymlinkLoop
    );
    assert!(!fs.exists("/link1"));

    let mut fs = VirtualFileSystem::new();
    fs.symlink("/b", "/a").unwrap();
    fs.symlink("/c", "/b").unwrap();
    fs.symlink("/a", "/c").unwrap();
    assert_eq!(
        fs.realpath("/a").unwrap_err().kind(),
        &JustBashErrorKind::SymlinkLoop
    );

    let mut fs = VirtualFileSystem::new();
    fs.write_file("/dir/target.txt", "content").unwrap();
    fs.symlink("target.txt", "/dir/link").unwrap();
    assert_eq!(fs.read_file("/dir/link").unwrap(), "content");
    fs.symlink("../dir/target.txt", "/link-up").unwrap();
    assert_eq!(fs.read_file("/link-up").unwrap(), "content");
    fs.symlink("/missing", "/broken").unwrap();
    assert_eq!(
        fs.read_file("/broken").unwrap_err().kind(),
        &JustBashErrorKind::NotFound
    );
    fs.symlink("/dir", "/dir-link").unwrap();
    assert_eq!(fs.readdir("/dir-link").unwrap(), vec!["link", "target.txt"]);
    fs.symlink("../../../dir/target.txt", "/excessive").unwrap();
    assert_eq!(fs.read_file("/excessive").unwrap(), "content");
    fs.symlink("/dir/link", "/chain").unwrap();
    assert_eq!(fs.read_file("/chain").unwrap(), "content");
}

#[test]
fn upstream_in_memory_lstat_stat_realpath_symlink_cases() {
    let mut fs = VirtualFileSystem::new();
    fs.write_file("/target.txt", "content").unwrap();
    fs.symlink("/target.txt", "/link").unwrap();
    assert!(fs.lstat("/link").unwrap().is_symbolic_link);
    assert!(fs.stat("/link").unwrap().is_file);
    assert_eq!(fs.realpath("/link").unwrap(), "/target.txt");

    fs.mkdir("/real-dir", MkdirOptions::default()).unwrap();
    fs.write_file("/real-dir/nested.txt", "nested").unwrap();
    fs.symlink("/real-dir", "/dir-link").unwrap();
    assert!(fs.lstat("/dir-link/nested.txt").unwrap().is_file);

    assert_eq!(fs.readlink("/link").unwrap(), "/target.txt");
    fs.symlink("target.txt", "/relative-link").unwrap();
    assert_eq!(fs.readlink("/relative-link").unwrap(), "target.txt");
    assert_eq!(
        fs.readlink("/target.txt").unwrap_err().kind(),
        &JustBashErrorKind::InvalidInput
    );
    assert_eq!(
        fs.readlink("/missing").unwrap_err().kind(),
        &JustBashErrorKind::NotFound
    );
}

#[test]
fn upstream_in_memory_write_append_rm_cp_link_symlink_policy_cases() {
    let mut fs = VirtualFileSystem::new();
    fs.write_file("/target.txt", "target").unwrap();
    fs.symlink("/target.txt", "/link").unwrap();
    fs.write_file("/link", "replacement").unwrap();
    assert!(!fs.lstat("/link").unwrap().is_symbolic_link);
    assert_eq!(fs.read_file("/target.txt").unwrap(), "target");
    assert_eq!(fs.read_file("/link").unwrap(), "replacement");

    fs.symlink("/target.txt", "/append-link").unwrap();
    fs.append_file("/append-link", "new").unwrap();
    assert_eq!(fs.read_file("/append-link").unwrap(), "new");
    assert_eq!(fs.read_file("/target.txt").unwrap(), "target");

    fs.symlink("/target.txt", "/rm-link").unwrap();
    fs.rm("/rm-link", RmOptions::default()).unwrap();
    assert!(fs.exists("/target.txt"));
    assert!(!fs.exists("/rm-link"));
    fs.symlink("/missing", "/broken-rm-link").unwrap();
    fs.rm("/broken-rm-link", RmOptions::default()).unwrap();
    assert!(!fs.exists("/broken-rm-link"));

    fs.symlink("/target.txt", "/copy-link").unwrap();
    fs.cp("/copy-link", "/copied-link", CpOptions::default())
        .unwrap();
    assert!(fs.lstat("/copied-link").unwrap().is_symbolic_link);
    assert_eq!(fs.readlink("/copied-link").unwrap(), "/target.txt");

    fs.mkdir("/tree", MkdirOptions::default()).unwrap();
    fs.symlink("/target.txt", "/tree/link").unwrap();
    fs.cp("/tree", "/tree-copy", CpOptions { recursive: true })
        .unwrap();
    assert!(fs.lstat("/tree-copy/link").unwrap().is_symbolic_link);

    assert_eq!(
        fs.link("/tree", "/tree-hardlink").unwrap_err().kind(),
        &JustBashErrorKind::PermissionDenied
    );
    assert_eq!(
        fs.link("/missing", "/hardlink").unwrap_err().kind(),
        &JustBashErrorKind::NotFound
    );
    fs.link("/target.txt", "/hardlink").unwrap();
    assert_eq!(fs.read_file("/hardlink").unwrap(), "target");

    fs.write_file("/   ", "spaces").unwrap();
    assert_eq!(fs.read_file("/   ").unwrap(), "spaces");

    let mut deny = VirtualFileSystem::new().with_symlink_policy(SymlinkPolicy::DenyCreation);
    assert_eq!(
        deny.symlink("/target", "/link").unwrap_err().kind(),
        &JustBashErrorKind::PermissionDenied
    );
}

#[test]
fn upstream_virtual_fs_copy_move_remove_list_stat_cases() {
    let mut fs = VirtualFileSystem::new();
    fs.write_file("/src/file.txt", "one").unwrap();
    fs.write_file("/src/nested/two.txt", "two").unwrap();
    fs.mkdir("/dest", MkdirOptions::default()).unwrap();

    fs.cp("/src/file.txt", "/dest/file.txt", CpOptions::default())
        .unwrap();
    assert_eq!(fs.read_file("/dest/file.txt").unwrap(), "one");
    fs.cp("/src", "/backup", CpOptions { recursive: true })
        .unwrap();
    assert_eq!(fs.read_file("/backup/nested/two.txt").unwrap(), "two");
    assert_eq!(
        fs.cp("/src", "/fail", CpOptions::default())
            .unwrap_err()
            .kind(),
        &JustBashErrorKind::IsDirectory
    );

    fs.mv("/dest/file.txt", "/dest/moved.txt").unwrap();
    assert!(!fs.exists("/dest/file.txt"));
    assert_eq!(fs.read_file("/dest/moved.txt").unwrap(), "one");

    assert_eq!(
        fs.rm("/src", RmOptions::default()).unwrap_err().kind(),
        &JustBashErrorKind::DirectoryNotEmpty
    );
    fs.rm(
        "/src",
        RmOptions {
            recursive: true,
            force: false,
        },
    )
    .unwrap();
    assert!(!fs.exists("/src/nested/two.txt"));
    fs.rm(
        "/missing",
        RmOptions {
            recursive: false,
            force: true,
        },
    )
    .unwrap();

    fs.chmod("/dest/moved.txt", 0o755).unwrap();
    let stat = fs.stat("/dest/moved.txt").unwrap();
    assert_eq!(stat.size, 3);
    assert_eq!(stat.mode, 0o755);
    assert_eq!(fs.readdir("/dest").unwrap(), vec!["moved.txt"]);
}

#[test]
fn upstream_glob_path_matching_cases() {
    let mut fs = VirtualFileSystem::new();
    fs.write_file("/project/a.txt", "a").unwrap();
    fs.write_file("/project/b.md", "b").unwrap();
    fs.write_file("/project/src/main.rs", "main").unwrap();
    fs.write_file("/project/src/lib.rs", "lib").unwrap();
    fs.write_file("/project/src/nested/mod.rs", "mod").unwrap();
    fs.write_file("/project/src/test1.js", "js").unwrap();
    fs.write_file("/project/.hidden", "h").unwrap();

    let options = GlobOptions::default();
    assert_eq!(glob_paths(&fs, "/project", "*.txt", options), vec!["a.txt"]);
    assert_eq!(
        glob_paths(&fs, "/project", "src/*.rs", options),
        vec!["src/lib.rs", "src/main.rs"]
    );
    assert_eq!(
        glob_paths(&fs, "/project", "src/**/*.rs", options),
        vec!["src/lib.rs", "src/main.rs", "src/nested/mod.rs"]
    );
    assert_eq!(
        glob_paths(&fs, "/project", "src/test?.js", options),
        vec!["src/test1.js"]
    );
    assert_eq!(
        glob_paths(&fs, "/project", "src/[lm]*.rs", options),
        vec!["src/lib.rs", "src/main.rs"]
    );
    assert!(glob_paths(&fs, "/project", "*.nope", options).is_empty());
    assert!(
        glob_paths(&fs, "/project", "*", options)
            .iter()
            .all(|path| !path.contains(".hidden"))
    );
    assert_eq!(
        glob_paths(
            &fs,
            "/project",
            "*",
            GlobOptions {
                include_hidden: true,
                ..GlobOptions::default()
            },
        ),
        vec![".hidden", "a.txt", "b.md", "src"]
    );
    assert!(match_glob(
        "File.TXT",
        "*.txt",
        GlobOptions {
            ignore_case: true,
            ..GlobOptions::default()
        }
    ));
}

#[test]
fn upstream_redirection_binary_and_text_write_cases() {
    let mut fs = VirtualFileSystem::new();
    fs.write_redirection(
        "/",
        "/output.bin",
        OutputPayload::Bytes(ByteString::from_bytes(vec![0x80, 0x90, 0xa0, 0xb0, 0xff])),
        false,
    )
    .unwrap();
    assert_eq!(
        fs.read_file_buffer("/output.bin").unwrap(),
        vec![0x80, 0x90, 0xa0, 0xb0, 0xff]
    );

    fs.write_redirection(
        "/",
        "/output.bin",
        OutputPayload::Bytes(ByteString::from_bytes(vec![0x00, 0x01])),
        true,
    )
    .unwrap();
    assert_eq!(
        fs.read_file_buffer("/output.bin").unwrap(),
        vec![0x80, 0x90, 0xa0, 0xb0, 0xff, 0x00, 0x01]
    );

    fs.write_redirection(
        "/",
        "/utf8.txt",
        OutputPayload::Text("café\n".to_string()),
        false,
    )
    .unwrap();
    assert_eq!(
        fs.read_file_buffer("/utf8.txt").unwrap(),
        "café\n".as_bytes()
    );

    fs.write_redirection(
        "/",
        "/allbytes.bin",
        OutputPayload::Bytes(ByteString::from_bytes((0..=255).collect::<Vec<u8>>())),
        false,
    )
    .unwrap();
    assert_eq!(fs.read_file_buffer("/allbytes.bin").unwrap().len(), 256);
}

#[test]
fn upstream_here_doc_write_cases_preserve_whitespace_and_utf8() {
    let mut fs = VirtualFileSystem::new();
    let content = "  alpha\n\tbeta\n\n한\nEOF inside text\n";
    fs.write_here_doc("/", "/heredoc.txt", content, false)
        .unwrap();
    assert_eq!(fs.read_file("/heredoc.txt").unwrap(), content);
    assert_eq!(fs.stat("/heredoc.txt").unwrap().size, content.len());

    fs.write_here_doc("/", "/heredoc.txt", "tail", true)
        .unwrap();
    assert_eq!(
        fs.read_file("/heredoc.txt").unwrap(),
        format!("{content}tail")
    );
}

#[test]
fn upstream_file_reader_fallback_and_concat_cases() {
    let mut fs = VirtualFileSystem::new();
    fs.write_file("/work/a.bin", vec![0x00, 0x7f, 0x80, 0xff])
        .unwrap();
    fs.write_file("/work/b.txt", "text").unwrap();
    let stdin = ByteString::from_bytes(vec![b's', b't', b'd', b'i', b'n']);

    let files = vec![
        "a.bin".to_string(),
        "-".to_string(),
        "missing".to_string(),
        "b.txt".to_string(),
    ];
    let result = read_files(
        &fs,
        "/work",
        &files,
        stdin.clone(),
        &ReadFilesOptions::new("cat"),
    );
    assert_eq!(result.exit_code, 1);
    assert_eq!(
        result.files[0].content.as_bytes(),
        &[0x00, 0x7f, 0x80, 0xff]
    );
    assert_eq!(result.files[1].content, stdin);
    assert!(
        result
            .stderr
            .contains("cat: missing: No such file or directory")
    );
    assert_eq!(result.files[2].content.decode_utf8_or_latin1(), "text");

    let joined = read_and_concat(
        &fs,
        "/work",
        &["a.bin".to_string(), "b.txt".to_string()],
        ByteString::default(),
        "cat",
    )
    .unwrap();
    assert_eq!(
        joined.as_bytes(),
        &[0x00, 0x7f, 0x80, 0xff, b't', b'e', b'x', b't']
    );

    let bytes = ByteString::from_bytes(vec![0x00, 0x7f, 0x80, 0xc3, 0xa9, 0xff]);
    assert_eq!(ByteString::from_latin1(&bytes.to_latin1()).unwrap(), bytes);
}

#[test]
fn upstream_session_persists_fs_and_resets_scoped_env_cwd() {
    let mut session = VirtualSession::new();
    session
        .fs_mut()
        .mkdir("/tmp/test", MkdirOptions { recursive: true })
        .unwrap();
    session
        .with_exec_scope(
            ExecOptions {
                cwd: Some("/tmp/test".to_string()),
                env: BTreeMap::from([("MODE".to_string(), "temporary".to_string())]),
                replace_env: false,
            },
            |scoped| {
                assert_eq!(scoped.cwd(), "/tmp/test");
                assert_eq!(
                    scoped.env().get("MODE").map(String::as_str),
                    Some("temporary")
                );
                scoped.fs_mut().write_file("/tmp/test/persisted.txt", "yes")
            },
        )
        .unwrap();

    assert_eq!(session.cwd(), "/home/user/project");
    assert_eq!(session.env().get("MODE"), None);
    assert_eq!(
        session.fs().read_file("/tmp/test/persisted.txt").unwrap(),
        "yes"
    );

    session
        .with_exec_scope(
            ExecOptions {
                cwd: None,
                env: BTreeMap::from([("ONLY".to_string(), "value".to_string())]),
                replace_env: true,
            },
            |scoped| {
                assert_eq!(scoped.env().get("PWD"), None);
                assert_eq!(scoped.env().get("ONLY").map(String::as_str), Some("value"));
                Ok(())
            },
        )
        .unwrap();
    assert!(session.env().contains_key("PWD"));
    assert_eq!(session.env().get("ONLY"), None);
}

#[test]
fn upstream_error_sanitization_cases() {
    assert_eq!(
        sanitize_error_message("open /Users/alice/project/secret.txt failed"),
        "open <path> failed"
    );
    assert_eq!(
        sanitize_error_message("at /home/runner/work/file.rs\n    at stack"),
        "at <path>"
    );
    assert_eq!(
        sanitize_error_message("ENOENT: open /virtual/file"),
        "ENOENT: open /virtual/file"
    );
    assert_eq!(sanitize_error_message(""), "");
    assert_eq!(
        sanitize_error_message("paths /tmp/a and /etc/passwd and C:\\Users\\Alice\\secret.txt"),
        "paths <path> and <path> and <path>"
    );
    assert_eq!(
        sanitize_host_error_message("file:///workspace/app/index.js at \\\\server\\share\\file"),
        "<path> at <path>"
    );
}
