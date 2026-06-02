use super::*;
use std::collections::BTreeMap;

fn utf8(text: &str) -> Vec<u8> {
    text.as_bytes().to_vec()
}

fn jbc26_bash_with_files(files: &[(&str, &str)]) -> Bash {
    Bash::with_options(BashOptions {
        cwd: Some("/home/user".to_string()),
        files: files
            .iter()
            .map(|(path, content)| ((*path).to_string(), (*content).to_string()))
            .collect::<BTreeMap<_, _>>(),
        ..BashOptions::default()
    })
}

fn assert_exec_success(result: &JustBashExecResult) {
    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
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
fn upstream_read_write_fs_virtual_backend_reads_writes_stats_and_mutates_paths() {
    let mut fs = ReadWriteFileSystem::with_text_files([
        ("/test.txt", "real content"),
        ("/subdir/file.txt", "nested"),
        ("/append.txt", "start"),
        ("/exists.txt", "content"),
        ("/dir/file.txt", "content"),
        ("/srcdir/file.txt", "content"),
        ("/source.txt", "content"),
        ("/original.txt", "content"),
    ]);

    assert_eq!(fs.read_file("/test.txt").unwrap(), "real content");
    assert_eq!(fs.read_file("/subdir/file.txt").unwrap(), "nested");
    assert_eq!(
        fs.read_file("/nonexistent.txt").unwrap_err().kind(),
        &JustBashErrorKind::NotFound
    );
    assert_eq!(
        fs.read_file("/dir").unwrap_err().kind(),
        &JustBashErrorKind::IsDirectory
    );

    fs.write_file("/new.txt", "new content").unwrap();
    fs.write_file("/deep/nested/file.txt", "content").unwrap();
    fs.write_file("/test.txt", "modified").unwrap();
    fs.write_file("/binary.bin", vec![0x00, 0x01, 0x02, 0xff])
        .unwrap();
    fs.append_file("/append.txt", "-end").unwrap();
    fs.append_file("/created-by-append.txt", "content").unwrap();

    assert_eq!(fs.read_file("/new.txt").unwrap(), "new content");
    assert_eq!(fs.read_file("/deep/nested/file.txt").unwrap(), "content");
    assert_eq!(fs.read_file("/test.txt").unwrap(), "modified");
    assert_eq!(
        fs.read_file_buffer("/binary.bin").unwrap(),
        vec![0x00, 0x01, 0x02, 0xff]
    );
    assert_eq!(fs.read_file("/append.txt").unwrap(), "start-end");
    assert_eq!(fs.read_file("/created-by-append.txt").unwrap(), "content");

    assert!(fs.exists("/exists.txt"));
    assert!(fs.exists("/dir"));
    assert!(!fs.exists("/missing"));
    assert!(fs.stat("/test.txt").unwrap().is_file);
    assert!(fs.stat("/dir").unwrap().is_directory);
    assert_eq!(
        fs.stat("/missing").unwrap_err().kind(),
        &JustBashErrorKind::NotFound
    );

    fs.symlink("target.txt", "/link").unwrap();
    assert!(fs.lstat("/link").unwrap().is_symbolic_link);
    assert_eq!(fs.readlink("/link").unwrap(), "target.txt");

    fs.mkdir("/newdir", MkdirOptions::default()).unwrap();
    fs.mkdir("/a/b/c", MkdirOptions { recursive: true })
        .unwrap();
    assert_eq!(
        fs.mkdir("/missing/dir", MkdirOptions::default())
            .unwrap_err()
            .kind(),
        &JustBashErrorKind::NotFound
    );
    assert_eq!(
        fs.mkdir("/newdir", MkdirOptions::default())
            .unwrap_err()
            .kind(),
        &JustBashErrorKind::AlreadyExists
    );

    fs.write_file("/list/a.txt", "a").unwrap();
    fs.write_file("/list/b.txt", "b").unwrap();
    fs.mkdir("/list/subdir", MkdirOptions::default()).unwrap();
    assert_eq!(
        fs.readdir("/list").unwrap(),
        vec!["a.txt", "b.txt", "subdir"]
    );
    assert_eq!(
        fs.readdir("/missing").unwrap_err().kind(),
        &JustBashErrorKind::NotFound
    );
    assert_eq!(
        fs.readdir("/list/a.txt").unwrap_err().kind(),
        &JustBashErrorKind::NotDirectory
    );

    fs.rm("/source.txt", RmOptions::default()).unwrap();
    assert!(!fs.exists("/source.txt"));
    fs.rm(
        "/dir",
        RmOptions {
            recursive: true,
            force: false,
        },
    )
    .unwrap();
    assert!(!fs.exists("/dir/file.txt"));
    assert_eq!(
        fs.rm("/missing", RmOptions::default()).unwrap_err().kind(),
        &JustBashErrorKind::NotFound
    );
    fs.rm(
        "/missing",
        RmOptions {
            recursive: false,
            force: true,
        },
    )
    .unwrap();

    fs.cp("/test.txt", "/copied.txt", CpOptions::default())
        .unwrap();
    fs.cp("/srcdir", "/destdir", CpOptions { recursive: true })
        .unwrap();
    assert_eq!(fs.read_file("/copied.txt").unwrap(), "modified");
    assert_eq!(fs.read_file("/destdir/file.txt").unwrap(), "content");
    assert_eq!(
        fs.cp("/missing", "/dest", CpOptions::default())
            .unwrap_err()
            .kind(),
        &JustBashErrorKind::NotFound
    );

    fs.mv("/copied.txt", "/moved.txt").unwrap();
    fs.mv("/destdir", "/moveddir").unwrap();
    assert!(!fs.exists("/copied.txt"));
    assert_eq!(fs.read_file("/moved.txt").unwrap(), "modified");
    assert_eq!(fs.read_file("/moveddir/file.txt").unwrap(), "content");

    fs.chmod("/moved.txt", 0o755).unwrap();
    assert_eq!(fs.stat("/moved.txt").unwrap().mode, 0o755);
    assert_eq!(
        fs.chmod("/missing", 0o755).unwrap_err().kind(),
        &JustBashErrorKind::NotFound
    );

    fs.link("/original.txt", "/hardlink.txt").unwrap();
    assert_eq!(fs.read_file("/hardlink.txt").unwrap(), "content");
    assert_eq!(
        fs.link("/missing", "/hardlink-missing").unwrap_err().kind(),
        &JustBashErrorKind::NotFound
    );
    assert_eq!(
        fs.link("/original.txt", "/hardlink.txt")
            .unwrap_err()
            .kind(),
        &JustBashErrorKind::AlreadyExists
    );
}

#[test]
fn upstream_read_write_fs_virtual_backend_encoding_readdir_and_path_inventory_cases() {
    let mut fs = ReadWriteFileSystem::new();
    fs.write_file_with_encoding("/base64.txt", "SGVsbG8gV29ybGQ=", BufferEncoding::Base64)
        .unwrap();
    fs.write_file_with_encoding("/hex.txt", "48656c6c6f", BufferEncoding::Hex)
        .unwrap();
    assert_eq!(fs.read_file("/base64.txt").unwrap(), "Hello World");
    assert_eq!(fs.read_file("/hex.txt").unwrap(), "Hello");

    fs.write_file("/typed/file.txt", "content").unwrap();
    fs.mkdir("/typed/subdir", MkdirOptions::default()).unwrap();
    fs.symlink("/typed/file.txt", "/typed/link.txt").unwrap();
    let entries = fs.readdir_with_file_types("/typed").unwrap();
    let names = entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["file.txt", "link.txt", "subdir"]);
    assert!(
        entries
            .iter()
            .any(|entry| entry.name == "file.txt" && entry.is_file)
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry.name == "subdir" && entry.is_directory)
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry.name == "link.txt" && entry.is_symbolic_link)
    );
    assert_eq!(fs.readdir("/typed").unwrap(), names);
    assert_eq!(
        fs.readdir_with_file_types("/missing").unwrap_err().kind(),
        &JustBashErrorKind::NotFound
    );
    assert_eq!(
        fs.readdir_with_file_types("/typed/file.txt")
            .unwrap_err()
            .kind(),
        &JustBashErrorKind::NotDirectory
    );

    let mut sorted = ReadWriteFileSystem::new();
    sorted.write_file("/Zebra.txt", "z").unwrap();
    sorted.write_file("/apple.txt", "a").unwrap();
    sorted.write_file("/Banana.txt", "b").unwrap();
    let sorted_names = sorted
        .readdir_with_file_types("/")
        .unwrap()
        .into_iter()
        .map(|entry| entry.name)
        .collect::<Vec<_>>();
    assert_eq!(sorted_names, vec!["Banana.txt", "Zebra.txt", "apple.txt"]);

    assert_eq!(fs.resolve_path("/dir", "file.txt"), "/dir/file.txt");
    assert_eq!(fs.resolve_path("/dir", "../file.txt"), "/file.txt");
    assert_eq!(
        fs.resolve_path("/dir", "/other/file.txt"),
        "/other/file.txt"
    );

    fs.write_file("/subdir/b.txt", "b").unwrap();
    let paths = fs.get_all_paths();
    assert!(paths.contains(&"/base64.txt".to_string()));
    assert!(paths.contains(&"/hex.txt".to_string()));
    assert!(paths.contains(&"/subdir/b.txt".to_string()));
}

#[test]
fn upstream_overlay_fs_virtual_backend_mount_copy_on_write_deletion_and_read_only_cases() {
    let lower = VirtualFileSystem::with_text_files([
        ("/test.txt", "real content"),
        ("/subdir/file.txt", "nested"),
        ("/append.txt", "start"),
        ("/delete.txt", "content"),
        ("/a.txt", "a"),
        ("/b.txt", "b"),
        ("/recreate.txt", "original"),
        ("/dir/file.txt", "content"),
        ("/real.txt", "real"),
        ("/source.txt", "source content"),
        ("/srcdir/file.txt", "nested"),
    ]);
    let default_overlay = OverlayFileSystem::new(lower.clone());
    assert_eq!(
        default_overlay.get_mount_point(),
        DEFAULT_OVERLAY_MOUNT_POINT
    );
    assert_eq!(
        default_overlay
            .read_file("/home/user/project/test.txt")
            .unwrap(),
        "real content"
    );
    assert!(
        default_overlay
            .readdir(DEFAULT_OVERLAY_MOUNT_POINT)
            .unwrap()
            .contains(&"test.txt".to_string())
    );
    assert_eq!(
        default_overlay.read_file("/test.txt").unwrap_err().kind(),
        &JustBashErrorKind::NotFound
    );
    assert!(default_overlay.exists("/home"));
    assert!(default_overlay.exists("/home/user"));
    assert!(default_overlay.exists("/home/user/project"));

    let mut overlay = OverlayFileSystem::with_mount_point(lower, "/").unwrap();
    assert_eq!(overlay.read_file("/test.txt").unwrap(), "real content");
    assert_eq!(overlay.read_file("/subdir/file.txt").unwrap(), "nested");
    assert!(overlay.stat("/test.txt").unwrap().is_file);
    assert_eq!(overlay.stat("/test.txt").unwrap().size, 12);

    overlay.write_file("/new.txt", "memory content").unwrap();
    overlay.write_file("/test.txt", "modified").unwrap();
    overlay.mkdir("/newdir", MkdirOptions::default()).unwrap();
    overlay.append_file("/append.txt", "-end").unwrap();
    assert_eq!(overlay.read_file("/new.txt").unwrap(), "memory content");
    assert_eq!(overlay.read_file("/test.txt").unwrap(), "modified");
    assert!(overlay.stat("/newdir").unwrap().is_directory);
    assert_eq!(overlay.read_file("/append.txt").unwrap(), "start-end");

    overlay.rm("/delete.txt", RmOptions::default()).unwrap();
    assert!(!overlay.exists("/delete.txt"));
    assert!(overlay.is_deleted("/delete.txt"));
    overlay.rm("/a.txt", RmOptions::default()).unwrap();
    let entries = overlay.readdir("/").unwrap();
    assert!(!entries.contains(&"a.txt".to_string()));
    assert!(entries.contains(&"b.txt".to_string()));
    overlay.rm("/recreate.txt", RmOptions::default()).unwrap();
    overlay.write_file("/recreate.txt", "new content").unwrap();
    assert_eq!(overlay.read_file("/recreate.txt").unwrap(), "new content");

    overlay
        .rm(
            "/dir",
            RmOptions {
                recursive: true,
                force: false,
            },
        )
        .unwrap();
    assert!(!overlay.exists("/dir"));
    assert!(!overlay.exists("/dir/file.txt"));

    overlay.write_file("/memory.txt", "memory").unwrap();
    let entries = overlay.readdir("/").unwrap();
    assert!(entries.contains(&"real.txt".to_string()));
    assert!(entries.contains(&"memory.txt".to_string()));
    assert_eq!(
        entries.iter().filter(|entry| *entry == "real.txt").count(),
        1
    );

    assert_eq!(
        overlay
            .read_file("/../../../etc/passwd")
            .unwrap_err()
            .kind(),
        &JustBashErrorKind::NotFound
    );
    assert_eq!(overlay.read_file("/subdir/../real.txt").unwrap(), "real");
    overlay.symlink("/target.txt", "/escape-link").unwrap();
    assert_eq!(
        overlay.read_file("/escape-link").unwrap_err().kind(),
        &JustBashErrorKind::NotFound
    );

    overlay.write_file("/target.txt", "target content").unwrap();
    overlay.symlink("/target.txt", "/link").unwrap();
    assert_eq!(overlay.read_file("/link").unwrap(), "target content");
    assert_eq!(overlay.readlink("/link").unwrap(), "/target.txt");
    assert!(overlay.lstat("/link").unwrap().is_symbolic_link);

    overlay
        .cp("/source.txt", "/copy.txt", CpOptions::default())
        .unwrap();
    overlay.mv("/source.txt", "/moved.txt").unwrap();
    overlay
        .cp("/srcdir", "/destdir", CpOptions { recursive: true })
        .unwrap();
    assert_eq!(overlay.read_file("/copy.txt").unwrap(), "source content");
    assert!(!overlay.exists("/source.txt"));
    assert_eq!(overlay.read_file("/moved.txt").unwrap(), "source content");
    assert_eq!(overlay.read_file("/destdir/file.txt").unwrap(), "nested");

    overlay.chmod("/real.txt", 0o755).unwrap();
    assert_eq!(overlay.stat("/real.txt").unwrap().mode, 0o755);
    overlay.link("/real.txt", "/hardlink.txt").unwrap();
    assert_eq!(overlay.read_file("/hardlink.txt").unwrap(), "real");
    assert!(overlay.exists("/real.txt"));
    assert!(overlay.exists("/memory.txt"));
    assert!(!overlay.exists("/deleted.txt"));
    assert!(!overlay.exists("/nonexistent.txt"));

    let mut read_only = OverlayFileSystem::with_mount_point(
        VirtualFileSystem::with_text_files([
            ("/existing.txt", "content"),
            ("/delete.txt", "content"),
            ("/source.txt", "content"),
            ("/file.txt", "content"),
        ]),
        "/",
    )
    .unwrap()
    .with_read_only(true);
    assert_eq!(
        read_only
            .write_file("/test.txt", "content")
            .unwrap_err()
            .kind(),
        &JustBashErrorKind::ReadOnly
    );
    assert_eq!(
        read_only
            .append_file("/existing.txt", "more")
            .unwrap_err()
            .kind(),
        &JustBashErrorKind::ReadOnly
    );
    assert_eq!(
        read_only
            .mkdir("/newdir", MkdirOptions::default())
            .unwrap_err()
            .kind(),
        &JustBashErrorKind::ReadOnly
    );
    assert_eq!(
        read_only
            .rm("/delete.txt", RmOptions::default())
            .unwrap_err()
            .kind(),
        &JustBashErrorKind::ReadOnly
    );
    assert_eq!(
        read_only
            .cp("/source.txt", "/dest.txt", CpOptions::default())
            .unwrap_err()
            .kind(),
        &JustBashErrorKind::ReadOnly
    );
    assert_eq!(
        read_only.mv("/source.txt", "/dest.txt").unwrap_err().kind(),
        &JustBashErrorKind::ReadOnly
    );
    assert_eq!(
        read_only.chmod("/file.txt", 0o755).unwrap_err().kind(),
        &JustBashErrorKind::ReadOnly
    );
    assert_eq!(
        read_only.symlink("/target", "/link").unwrap_err().kind(),
        &JustBashErrorKind::ReadOnly
    );
    assert_eq!(
        read_only
            .link("/source.txt", "/hardlink.txt")
            .unwrap_err()
            .kind(),
        &JustBashErrorKind::ReadOnly
    );
    assert_eq!(read_only.read_file("/existing.txt").unwrap(), "content");
    assert!(read_only.exists("/existing.txt"));
    assert!(read_only.stat("/existing.txt").is_ok());
    assert!(
        read_only
            .readdir("/")
            .unwrap()
            .contains(&"existing.txt".to_string())
    );

    let mut writable = OverlayFileSystem::with_mount_point(VirtualFileSystem::new(), "/").unwrap();
    writable.write_file("/test.txt", "content").unwrap();
    assert_eq!(writable.read_file("/test.txt").unwrap(), "content");
}

#[test]
fn upstream_mountable_fs_routes_mounts_cross_mount_ops_and_virtual_dirs() {
    let mut fs = MountableFileSystem::new();
    fs.mount("/mnt/data", VirtualFileSystem::new()).unwrap();
    assert!(fs.is_mount_point("/mnt/data"));
    assert_eq!(fs.get_mounts(), vec!["/mnt/data"]);
    fs.unmount("/mnt/data").unwrap();
    assert!(!fs.is_mount_point("/mnt/data"));
    assert!(fs.get_mounts().is_empty());
    assert_eq!(
        fs.unmount("/mnt/data").unwrap_err().kind(),
        &JustBashErrorKind::NotFound
    );

    fs.mount(
        "/mnt/data",
        VirtualFileSystem::with_text_files([("/file1.txt", "first")]),
    )
    .unwrap();
    fs.mount(
        "/mnt/data",
        VirtualFileSystem::with_text_files([("/file2.txt", "second")]),
    )
    .unwrap();
    assert_eq!(fs.get_mounts(), vec!["/mnt/data"]);
    assert_eq!(fs.read_file("/mnt/data/file2.txt").unwrap(), "second");

    let base = VirtualFileSystem::with_text_files([("/base.txt", "base content")]);
    let mut fs = MountableFileSystem::with_base(base);
    fs.mount(
        "/mnt/data",
        VirtualFileSystem::with_text_files([("/test.txt", "mounted content")]),
    )
    .unwrap();
    assert_eq!(fs.read_file("/base.txt").unwrap(), "base content");
    assert_eq!(
        fs.read_file("/mnt/data/test.txt").unwrap(),
        "mounted content"
    );
    fs.write_file("/mnt/data/written.txt", "hello").unwrap();
    assert_eq!(fs.read_file("/mnt/data/written.txt").unwrap(), "hello");
    assert!(fs.exists("/mnt/data"));
    assert!(fs.stat("/mnt/data").unwrap().is_directory);

    assert_eq!(
        fs.mount("/", VirtualFileSystem::new()).unwrap_err().kind(),
        &JustBashErrorKind::InvalidInput
    );
    assert_eq!(
        fs.mount("/mnt/data/sub", VirtualFileSystem::new())
            .unwrap_err()
            .kind(),
        &JustBashErrorKind::InvalidInput
    );
    assert_eq!(
        fs.mount("/mnt/../bad", VirtualFileSystem::new())
            .unwrap_err()
            .kind(),
        &JustBashErrorKind::InvalidInput
    );
    fs.mount("/mnt/other", VirtualFileSystem::new()).unwrap();
    assert_eq!(fs.get_mounts(), vec!["/mnt/data", "/mnt/other"]);

    assert_eq!(fs.readdir("/mnt").unwrap(), vec!["data", "other"]);
    fs.write_file("/mnt/base.txt", "base").unwrap();
    let entries = fs.readdir("/mnt").unwrap();
    assert!(entries.contains(&"base.txt".to_string()));
    assert!(entries.contains(&"data".to_string()));
    assert!(
        fs.readdir("/mnt/data")
            .unwrap()
            .contains(&"test.txt".to_string())
    );
    fs.mkdir("/mnt/data/subdir", MkdirOptions::default())
        .unwrap();
    assert!(fs.exists("/mnt/data/subdir"));
    fs.mkdir("/mnt/data", MkdirOptions { recursive: true })
        .unwrap();
    assert_eq!(
        fs.mkdir("/mnt/data", MkdirOptions::default())
            .unwrap_err()
            .kind(),
        &JustBashErrorKind::AlreadyExists
    );

    fs.rm("/mnt/data/written.txt", RmOptions::default())
        .unwrap();
    assert!(!fs.exists("/mnt/data/written.txt"));
    assert_eq!(
        fs.rm(
            "/mnt/data",
            RmOptions {
                recursive: true,
                force: false,
            },
        )
        .unwrap_err()
        .kind(),
        &JustBashErrorKind::Busy
    );
    assert_eq!(
        fs.rm(
            "/mnt",
            RmOptions {
                recursive: true,
                force: false,
            },
        )
        .unwrap_err()
        .kind(),
        &JustBashErrorKind::Busy
    );

    let mut fs = MountableFileSystem::new();
    fs.mount(
        "/mnt/a",
        VirtualFileSystem::with_text_files([
            ("/src.txt", "content"),
            ("/dir/a.txt", "a"),
            ("/dir/b.txt", "b"),
        ]),
    )
    .unwrap();
    fs.mount("/mnt/b", VirtualFileSystem::new()).unwrap();
    fs.cp("/mnt/a/src.txt", "/dest.txt", CpOptions::default())
        .unwrap();
    fs.cp("/dest.txt", "/mnt/b/dest.txt", CpOptions::default())
        .unwrap();
    fs.cp("/mnt/a/src.txt", "/mnt/b/dest2.txt", CpOptions::default())
        .unwrap();
    fs.cp("/mnt/a/dir", "/mnt/b/dir", CpOptions { recursive: true })
        .unwrap();
    assert_eq!(fs.read_file("/dest.txt").unwrap(), "content");
    assert_eq!(fs.read_file("/mnt/b/dest.txt").unwrap(), "content");
    assert_eq!(fs.read_file("/mnt/b/dest2.txt").unwrap(), "content");
    assert_eq!(fs.read_file("/mnt/b/dir/a.txt").unwrap(), "a");
    assert_eq!(fs.read_file("/mnt/b/dir/b.txt").unwrap(), "b");

    fs.chmod("/mnt/a/src.txt", 0o755).unwrap();
    fs.cp("/mnt/a/src.txt", "/mnt/b/script.sh", CpOptions::default())
        .unwrap();
    assert_eq!(fs.stat("/mnt/b/script.sh").unwrap().mode, 0o755);
    fs.cp(
        "/mnt/b/script.sh",
        "/mnt/b/script-copy.sh",
        CpOptions::default(),
    )
    .unwrap();
    assert_eq!(fs.read_file("/mnt/b/script-copy.sh").unwrap(), "content");

    fs.mv("/mnt/a/src.txt", "/mnt/b/moved.txt").unwrap();
    assert_eq!(fs.read_file("/mnt/b/moved.txt").unwrap(), "content");
    assert!(!fs.exists("/mnt/a/src.txt"));
    assert_eq!(
        fs.mv("/mnt/a", "/mnt/c").unwrap_err().kind(),
        &JustBashErrorKind::Busy
    );
    fs.mv("/mnt/b/script-copy.sh", "/mnt/b/script-moved.sh")
        .unwrap();
    assert_eq!(fs.read_file("/mnt/b/script-moved.sh").unwrap(), "content");

    let paths = fs.get_all_paths();
    assert!(paths.contains(&"/mnt".to_string()));
    assert!(paths.contains(&"/mnt/a".to_string()));
    assert!(paths.contains(&"/mnt/b/moved.txt".to_string()));

    fs.write_file("/mnt/a/target.txt", "target").unwrap();
    fs.symlink("/target.txt", "/mnt/a/link.txt").unwrap();
    assert_eq!(fs.read_file("/mnt/a/link.txt").unwrap(), "target");
    assert_eq!(fs.readlink("/mnt/a/link.txt").unwrap(), "/target.txt");
    fs.symlink("/mnt/a/target.txt", "/mnt/b/cross-link.txt")
        .unwrap();
    let target = fs.readlink("/mnt/b/cross-link.txt").unwrap();
    assert_eq!(target, "/mnt/a/target.txt");
    assert_eq!(fs.read_file(&target).unwrap(), "target");
    fs.cp(
        "/mnt/a/link.txt",
        "/mnt/b/copied-link.txt",
        CpOptions::default(),
    )
    .unwrap();
    assert_eq!(
        fs.readlink("/mnt/b/copied-link.txt").unwrap(),
        "/target.txt"
    );

    fs.link("/mnt/b/moved.txt", "/mnt/b/hardlink.txt").unwrap();
    assert_eq!(fs.read_file("/mnt/b/hardlink.txt").unwrap(), "content");
    assert_eq!(
        fs.link("/mnt/b/moved.txt", "/mnt/a/hardlink.txt")
            .unwrap_err()
            .kind(),
        &JustBashErrorKind::CrossDevice
    );

    assert!(fs.stat("/mnt").unwrap().is_directory);
    assert!(fs.exists("/mnt"));
    assert!(fs.exists("/mnt/b"));
    assert!(fs.stat("/mnt/b/moved.txt").unwrap().is_file);
    assert_eq!(fs.stat("/mnt/b/moved.txt").unwrap().size, 7);
    assert!(fs.lstat("/mnt/a/link.txt").unwrap().is_symbolic_link);
    fs.append_file("/mnt/b/moved.txt", " world").unwrap();
    assert_eq!(fs.read_file("/mnt/b/moved.txt").unwrap(), "content world");
    fs.chmod("/mnt/b", 0o700).unwrap();
    assert_eq!(fs.stat("/mnt/b").unwrap().mode, 0o700);
    assert_eq!(
        fs.resolve_path("/some/base", "/absolute/path"),
        "/absolute/path"
    );
    assert_eq!(
        fs.resolve_path("/some/base", "relative/path"),
        "/some/base/relative/path"
    );
    assert_eq!(fs.resolve_path("/a/b", "../c"), "/a/c");

    let mut edge = MountableFileSystem::new();
    edge.mount(
        "/mnt/data/",
        VirtualFileSystem::with_text_files([("/test.txt", "content")]),
    )
    .unwrap();
    assert!(edge.is_mount_point("/mnt/data"));
    assert!(edge.is_mount_point("/mnt/data/"));
    assert_eq!(edge.read_file("mnt/data/test.txt").unwrap(), "content");
    assert_eq!(
        edge.read_file("/mnt/data/../data/./test.txt").unwrap(),
        "content"
    );
    assert_eq!(edge.readdir("/mnt").unwrap(), vec!["data"]);
}

#[test]
fn jbc36_read_write_piping_large_virtual_data_rows() {
    let large = (1..=2_000)
        .map(|line| format!("row-{line}"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let mut files = BTreeMap::new();
    files.insert("/home/user/large.txt".to_string(), large);
    files.insert(
        "/home/user/dupes.txt".to_string(),
        "banana\napple\nbanana\ncarrot\napple\n".to_string(),
    );
    files.insert(
        "/home/user/log.txt".to_string(),
        "skip\nMATCH one\nskip\nMATCH two\n".to_string(),
    );
    let bash = Bash::with_options(BashOptions {
        cwd: Some("/home/user".to_string()),
        files,
        ..BashOptions::default()
    });

    assert_eq!(bash.exec("cat large.txt | wc -l").stdout.trim(), "2000");
    let direct_wc = bash.exec("wc -l large.txt");
    assert_exec_success(&direct_wc);
    assert_eq!(direct_wc.stdout, "2000 /home/user/large.txt\n");
    assert_eq!(
        bash.exec("cat dupes.txt | sort | uniq | wc -l")
            .stdout
            .trim(),
        "3"
    );
    assert_eq!(bash.exec("grep MATCH log.txt | wc -l").stdout.trim(), "2");

    let mut fs = VirtualFileSystem::new();
    fs.write_file("/binary.bin", vec![0x00, 0x01, 0x00, 0xff, 0x00])
        .unwrap();
    assert_eq!(
        fs.read_file_buffer("/binary.bin").unwrap(),
        vec![0x00, 0x01, 0x00, 0xff, 0x00]
    );
    assert_eq!(fs.stat("/binary.bin").unwrap().size, 5);
}

#[test]
fn jbc36_mountable_construction_time_mount_rows_are_virtual() {
    let base = VirtualFileSystem::with_text_files([("/base.txt", "base")]);
    let mut fs = MountableFileSystem::with_base(base);
    fs.mount(
        "/mnt/data",
        VirtualFileSystem::with_text_files([("/input.txt", "mounted")]),
    )
    .unwrap();

    assert_eq!(fs.read_file("/base.txt").unwrap(), "base");
    assert_eq!(fs.read_file("/mnt/data/input.txt").unwrap(), "mounted");
    fs.write_file("/mnt/data/output.txt", "written").unwrap();
    assert_eq!(fs.read_file("/mnt/data/output.txt").unwrap(), "written");
    assert_eq!(fs.readdir("/mnt").unwrap(), vec!["data"]);
    assert_eq!(
        fs.mount("/", VirtualFileSystem::new()).unwrap_err().kind(),
        &JustBashErrorKind::InvalidInput
    );
}

#[test]
fn jbc36_real_fs_utils_virtual_root_boundary_rows() {
    for (path, root, expected) in [
        ("/workspace", "/workspace", true),
        ("/workspace/file.txt", "/workspace", true),
        ("/workspace/nested/child.txt", "/workspace", true),
        ("/workspace-evil/file.txt", "/workspace", false),
        ("/workspaces/file.txt", "/workspace", false),
        ("/", "/workspace", false),
        ("/tmp/datastore/file.txt", "/tmp/data", false),
        ("D:\\project\\file.txt", "D:\\project", true),
        ("D:\\projects\\file.txt", "D:\\project", false),
    ] {
        assert_eq!(
            is_path_within_root(path, root),
            expected,
            "path {path:?} root {root:?}"
        );
    }
    assert_eq!(
        normalize_path("/workspace/../workspace/file.txt"),
        "/workspace/file.txt"
    );
    validate_path("/workspace/file.txt", "validateRealPath").unwrap();
    assert!(
        validate_path("/workspace/bad\0path", "validateRealPath")
            .unwrap_err()
            .to_string()
            .contains("validateRealPath")
    );
}

#[test]
fn jbc26_file_operation_comparison_rows_are_virtual_and_stateful() {
    let bash = jbc26_bash_with_files(&[]);
    let mkdir = bash.exec("mkdir newdir && mkdir -p a/b/c && mkdir -p a/b/c");
    assert_exec_success(&mkdir);
    assert!(bash.file_exists("/home/user/newdir"));
    assert!(bash.file_exists("/home/user/a/b/c"));

    let bash = jbc26_bash_with_files(&[
        ("/home/user/file.txt", "content"),
        ("/home/user/a.txt", ""),
        ("/home/user/b.txt", ""),
        ("/home/user/c.txt", ""),
        ("/home/user/dir/file.txt", "content"),
    ]);
    assert_exec_success(&bash.exec("rm file.txt"));
    assert!(!bash.file_exists("/home/user/file.txt"));
    assert_exec_success(&bash.exec("rm a.txt b.txt"));
    assert!(!bash.file_exists("/home/user/a.txt"));
    assert!(!bash.file_exists("/home/user/b.txt"));
    assert!(bash.file_exists("/home/user/c.txt"));
    assert_exec_success(&bash.exec("rm -r dir"));
    assert!(!bash.file_exists("/home/user/dir/file.txt"));
    assert_exec_success(&bash.exec("rm -f nonexistent.txt"));

    let bash = jbc26_bash_with_files(&[
        ("/home/user/source.txt", "hello world\n"),
        ("/home/user/file.txt", "content\n"),
        ("/home/user/dir/.gitkeep", ""),
        ("/home/user/src/a.txt", "a content\n"),
        ("/home/user/src/b.txt", "b content\n"),
        ("/home/user/a.txt", "a\n"),
        ("/home/user/b.txt", "b\n"),
    ]);
    assert_exec_success(&bash.exec("cp source.txt dest.txt"));
    assert_eq!(
        bash.read_file("/home/user/dest.txt").unwrap(),
        bash.read_file("/home/user/source.txt").unwrap()
    );
    assert_exec_success(&bash.exec("cp file.txt dir/"));
    assert_eq!(
        bash.read_file("/home/user/dir/file.txt").unwrap(),
        "content\n"
    );
    assert_exec_success(&bash.exec("cp -r src dest"));
    assert!(bash.file_exists("/home/user/dest/a.txt"));
    assert!(bash.file_exists("/home/user/dest/b.txt"));
    assert_exec_success(&bash.exec("cp a.txt b.txt dir/"));
    assert_eq!(bash.read_file("/home/user/dir/a.txt").unwrap(), "a\n");
    assert_eq!(bash.read_file("/home/user/dir/b.txt").unwrap(), "b\n");

    let bash = jbc26_bash_with_files(&[
        ("/home/user/old.txt", "content\n"),
        ("/home/user/file.txt", "content\n"),
        ("/home/user/dir/.gitkeep", ""),
        ("/home/user/olddir/file.txt", "content\n"),
        ("/home/user/a.txt", "a\n"),
        ("/home/user/b.txt", "b\n"),
    ]);
    assert_exec_success(&bash.exec("mv old.txt new.txt"));
    assert!(!bash.file_exists("/home/user/old.txt"));
    assert_eq!(bash.read_file("/home/user/new.txt").unwrap(), "content\n");
    assert_exec_success(&bash.exec("mv file.txt dir/"));
    assert!(!bash.file_exists("/home/user/file.txt"));
    assert_eq!(
        bash.read_file("/home/user/dir/file.txt").unwrap(),
        "content\n"
    );
    assert_exec_success(&bash.exec("mv olddir newdir"));
    assert!(!bash.file_exists("/home/user/olddir/file.txt"));
    assert_eq!(
        bash.read_file("/home/user/newdir/file.txt").unwrap(),
        "content\n"
    );
    assert_exec_success(&bash.exec("mv a.txt b.txt dir/"));
    assert!(!bash.file_exists("/home/user/a.txt"));
    assert!(!bash.file_exists("/home/user/b.txt"));
    assert_eq!(bash.read_file("/home/user/dir/a.txt").unwrap(), "a\n");
    assert_eq!(bash.read_file("/home/user/dir/b.txt").unwrap(), "b\n");

    let bash = jbc26_bash_with_files(&[("/home/user/existing.txt", "original content\n")]);
    assert_exec_success(&bash.exec("touch newfile.txt"));
    assert_eq!(bash.read_file("/home/user/newfile.txt").unwrap(), "");
    assert_exec_success(&bash.exec("touch a.txt b.txt c.txt"));
    assert!(bash.file_exists("/home/user/a.txt"));
    assert!(bash.file_exists("/home/user/b.txt"));
    assert!(bash.file_exists("/home/user/c.txt"));
    assert_exec_success(&bash.exec("touch existing.txt"));
    assert_eq!(
        bash.read_file("/home/user/existing.txt").unwrap(),
        "original content\n"
    );

    let pwd = bash.exec("pwd");
    assert_exec_success(&pwd);
    assert_eq!(pwd.stdout.trim(), "/home/user");
}

#[test]
fn jbc26_virtual_fs_path_security_encoding_and_error_shape_rows_are_sanitized() {
    let mut fs = VirtualFileSystem::new();
    fs.write_file("/file.txt", "safe content").unwrap();
    fs.write_file("/root.txt", "root").unwrap();

    assert_eq!(fs.read_file("/../file.txt").unwrap(), "safe content");
    assert_eq!(
        fs.read_file("/a/b/c/../../../file.txt").unwrap(),
        "safe content"
    );
    assert_eq!(fs.read_file("//file.txt").unwrap(), "safe content");
    assert_eq!(fs.read_file("/./file.txt").unwrap(), "safe content");
    assert_eq!(fs.read_file("/../../../root.txt").unwrap(), "root");
    assert!(
        fs.readdir("/../../")
            .unwrap()
            .contains(&"file.txt".to_string())
    );
    assert!(!fs.readdir("/../../").unwrap().contains(&"etc".to_string()));

    for name in [
        "..foo",
        "foo..",
        "...",
        "a..b",
        "%2e%2e",
        "%2f",
        ".hidden",
        "..hidden",
        "with spaces",
        "quote'file",
        "carriage\rreturn",
        "tab\tfile",
        "cafe\u{301}.txt",
        "caf\u{e9}.txt",
        "emoji-file",
    ] {
        let path = format!("/{name}");
        fs.write_file(&path, name).unwrap();
        assert_eq!(fs.read_file(&path).unwrap(), name);
    }
    let long_path = format!("/{}.txt", "a".repeat(250));
    fs.write_file(&long_path, "long").unwrap();
    assert_eq!(fs.read_file(&long_path).unwrap(), "long");

    fs.write_file_with_encoding("/large-b64.txt", "x".repeat(200_000), BufferEncoding::Utf8)
        .unwrap();
    assert!(
        !fs.read_file_with_encoding("/large-b64.txt", BufferEncoding::Base64)
            .unwrap()
            .is_empty()
    );
    fs.write_file_with_encoding("/binary.bin", vec![0x00, 0x80, 0xff], BufferEncoding::Utf8)
        .unwrap();
    assert_eq!(
        fs.read_file_buffer("/binary.bin").unwrap(),
        vec![0x00, 0x80, 0xff]
    );

    assert_eq!(
        fs.read_file("/bad\0path").unwrap_err().kind(),
        &JustBashErrorKind::NotFound
    );
    assert_eq!(
        fs.write_file("/bad\0path", "data").unwrap_err().kind(),
        &JustBashErrorKind::NotFound
    );
    assert_eq!(
        fs.append_file("/bad\0path", "data").unwrap_err().kind(),
        &JustBashErrorKind::NotFound
    );
    assert_eq!(
        fs.stat("/bad\0path").unwrap_err().kind(),
        &JustBashErrorKind::NotFound
    );
    assert_eq!(
        fs.lstat("/bad\0path").unwrap_err().kind(),
        &JustBashErrorKind::NotFound
    );
    assert_eq!(
        fs.mkdir("/bad\0path", MkdirOptions::default())
            .unwrap_err()
            .kind(),
        &JustBashErrorKind::NotFound
    );
    assert_eq!(
        fs.rm("/bad\0path", RmOptions::default())
            .unwrap_err()
            .kind(),
        &JustBashErrorKind::NotFound
    );
    assert_eq!(
        fs.chmod("/bad\0path", 0o755).unwrap_err().kind(),
        &JustBashErrorKind::NotFound
    );
    assert_eq!(
        fs.symlink("/target", "/bad\0link").unwrap_err().kind(),
        &JustBashErrorKind::NotFound
    );
    assert_eq!(
        fs.link("/file.txt", "/bad\0link").unwrap_err().kind(),
        &JustBashErrorKind::NotFound
    );
    assert_eq!(
        fs.readlink("/bad\0link").unwrap_err().kind(),
        &JustBashErrorKind::NotFound
    );
    assert_eq!(
        fs.realpath("/bad\0path").unwrap_err().kind(),
        &JustBashErrorKind::NotFound
    );
    assert_eq!(
        fs.utimes("/bad\0path", 42).unwrap_err().kind(),
        &JustBashErrorKind::NotFound
    );
    assert_eq!(
        fs.cp("/file.txt", "/bad\0copy", CpOptions::default())
            .unwrap_err()
            .kind(),
        &JustBashErrorKind::NotFound
    );
    assert!(!fs.exists("/bad\0path"));

    let missing = fs.read_file("/missing").unwrap_err().to_string();
    assert!(missing.contains("ENOENT"));
    assert!(!missing.contains("/private/tmp"));
    assert!(!missing.contains("/Users/"));
    assert_eq!(
        sanitize_host_error_message(
            "open /private/tmp/root/secret failed at file:///Users/alice/project"
        ),
        "open <path> failed at <path>"
    );
}

#[test]
fn jbc26_symlink_policy_mount_routing_and_overlay_precedence_rows_are_virtual() {
    let mut deny = VirtualFileSystem::new();
    deny.write_file("/target.txt", "target").unwrap();
    deny.symlink("/target.txt", "/link").unwrap();
    let mut deny = deny.with_symlink_policy(SymlinkPolicy::DenyCreation);
    assert_eq!(
        deny.symlink("/target.txt", "/new-link").unwrap_err().kind(),
        &JustBashErrorKind::PermissionDenied
    );
    assert_eq!(deny.readlink("/link").unwrap(), "/target.txt");
    assert!(deny.lstat("/link").unwrap().is_symbolic_link);
    assert_eq!(
        deny.read_file("/link").unwrap_err().kind(),
        &JustBashErrorKind::PermissionDenied
    );
    assert_eq!(
        deny.stat("/link").unwrap_err().kind(),
        &JustBashErrorKind::PermissionDenied
    );
    assert_eq!(
        deny.write_file("/link", "pwned").unwrap_err().kind(),
        &JustBashErrorKind::PermissionDenied
    );
    assert_eq!(
        deny.append_file("/link", "pwned").unwrap_err().kind(),
        &JustBashErrorKind::PermissionDenied
    );
    assert_eq!(
        deny.rm("/link", RmOptions::default()).unwrap_err().kind(),
        &JustBashErrorKind::PermissionDenied
    );
    assert_eq!(deny.read_file("/target.txt").unwrap(), "target");
    assert!(!deny.exists("/link"));

    let mut overlay = OverlayFileSystem::with_mount_point(
        VirtualFileSystem::with_text_files([
            ("/real.txt", "real"),
            ("/delete.txt", "delete"),
            ("/dir/file.txt", "lower"),
            ("/af-target.txt", "base"),
            ("/dev/null", "device"),
        ]),
        "/",
    )
    .unwrap();
    overlay.write_file("/real.txt", "upper").unwrap();
    assert_eq!(overlay.read_file("/real.txt").unwrap(), "upper");
    overlay.rm("/delete.txt", RmOptions::default()).unwrap();
    assert!(!overlay.exists("/delete.txt"));
    assert!(overlay.is_deleted("/delete.txt"));
    overlay.write_file("/delete.txt", "recreated").unwrap();
    assert_eq!(overlay.read_file("/delete.txt").unwrap(), "recreated");
    overlay.symlink("/af-target.txt", "/af-link").unwrap();
    overlay.append_file("/af-link", " appended").unwrap();
    assert_eq!(overlay.read_file("/af-link").unwrap(), "base appended");
    assert_eq!(overlay.read_file("/af-target.txt").unwrap(), "base");
    overlay.write_file("/dev/null", "injected").unwrap();
    assert_eq!(overlay.read_file("/dev/null").unwrap(), "injected");
    let entries = overlay.readdir_with_file_types("/").unwrap();
    assert!(
        entries
            .iter()
            .any(|entry| entry.name == "real.txt" && entry.is_file)
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry.name == "dir" && entry.is_directory)
    );

    let mut read_only = OverlayFileSystem::with_mount_point(
        VirtualFileSystem::with_text_files([("/existing.txt", "content")]),
        "/",
    )
    .unwrap()
    .with_read_only(true);
    assert_eq!(
        read_only
            .write_file("/blocked.txt", "nope")
            .unwrap_err()
            .kind(),
        &JustBashErrorKind::ReadOnly
    );
    assert_eq!(read_only.read_file("/existing.txt").unwrap(), "content");

    let mut mountable =
        MountableFileSystem::with_base(VirtualFileSystem::with_text_files([("/base.txt", "base")]));
    mountable
        .mount(
            "/mnt/a/",
            VirtualFileSystem::with_text_files([("/target.txt", "target")]),
        )
        .unwrap();
    mountable.mount("/mnt/b", VirtualFileSystem::new()).unwrap();
    assert!(mountable.is_mount_point("/mnt/a"));
    assert_eq!(mountable.readdir("/mnt").unwrap(), vec!["a", "b"]);
    assert_eq!(mountable.read_file("/mnt/a/target.txt").unwrap(), "target");
    mountable.symlink("/target.txt", "/mnt/a/link.txt").unwrap();
    assert_eq!(mountable.read_file("/mnt/a/link.txt").unwrap(), "target");
    assert_eq!(
        mountable.readlink("/mnt/a/link.txt").unwrap(),
        "/target.txt"
    );
    mountable.symlink("/loop", "/mnt/a/loop").unwrap();
    assert_eq!(
        mountable.read_file("/mnt/a/loop").unwrap_err().kind(),
        &JustBashErrorKind::SymlinkLoop
    );
    mountable.symlink("/missing", "/mnt/a/broken").unwrap();
    assert_eq!(
        mountable.realpath("/mnt/a/broken").unwrap_err().kind(),
        &JustBashErrorKind::NotFound
    );
    mountable.write_file("/mnt/b/secret.txt", "secret").unwrap();
    mountable
        .symlink("/mnt/b/secret.txt", "/mnt/a/cross.txt")
        .unwrap();
    assert_eq!(
        mountable.read_file("/mnt/a/cross.txt").unwrap_err().kind(),
        &JustBashErrorKind::NotFound
    );
    assert_eq!(mountable.realpath("/mnt/a").unwrap(), "/mnt/a");
    assert_eq!(
        mountable.readlink("/mnt/a/target.txt").unwrap_err().kind(),
        &JustBashErrorKind::InvalidInput
    );
    assert_eq!(
        mountable
            .link("/mnt/a/target.txt", "/mnt/b/hardlink.txt")
            .unwrap_err()
            .kind(),
        &JustBashErrorKind::CrossDevice
    );
    assert_eq!(
        mountable
            .rm(
                "/mnt",
                RmOptions {
                    recursive: true,
                    force: false,
                },
            )
            .unwrap_err()
            .kind(),
        &JustBashErrorKind::Busy
    );
    assert_eq!(
        mountable.read_file("/mnt/a/../../base.txt").unwrap(),
        "base"
    );
    let paths = mountable.get_all_paths();
    assert!(paths.contains(&"/mnt".to_string()));
    assert!(paths.contains(&"/mnt/a".to_string()));
    assert!(paths.contains(&"/mnt/a/target.txt".to_string()));
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
