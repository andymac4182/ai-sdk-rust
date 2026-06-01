use crate::encoding::ByteString;
use crate::fs::VirtualFileSystem;
use crate::path::resolve_path;

pub struct ReadFilesOptions {
    pub cmd_name: String,
    pub allow_stdin_marker: bool,
    pub stop_on_error: bool,
}

impl ReadFilesOptions {
    /// Creates options with upstream defaults.
    pub fn new(cmd_name: impl Into<String>) -> Self {
        Self {
            cmd_name: cmd_name.into(),
            allow_stdin_marker: true,
            stop_on_error: false,
        }
    }
}

/// Content read by command file-reader helpers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadFileContent {
    pub filename: String,
    pub content: ByteString,
}

/// Result of command file-reader helpers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadFilesResult {
    pub files: Vec<ReadFileContent>,
    pub stderr: String,
    pub exit_code: i32,
}

/// Reads files or stdin in byte-preserving form.
pub fn read_files(
    fs: &VirtualFileSystem,
    cwd: &str,
    files: &[String],
    stdin: ByteString,
    options: &ReadFilesOptions,
) -> ReadFilesResult {
    if files.is_empty() {
        return ReadFilesResult {
            files: vec![ReadFileContent {
                filename: String::new(),
                content: stdin,
            }],
            stderr: String::new(),
            exit_code: 0,
        };
    }

    let mut result = Vec::new();
    let mut stderr = String::new();
    let mut exit_code = 0;

    for file in files {
        if options.allow_stdin_marker && file == "-" {
            result.push(ReadFileContent {
                filename: "-".to_string(),
                content: stdin.clone(),
            });
            continue;
        }
        let file_path = resolve_path(cwd, file);
        match fs.read_file_bytes(&file_path) {
            Ok(content) => result.push(ReadFileContent {
                filename: file.clone(),
                content,
            }),
            Err(_) => {
                stderr.push_str(&format!(
                    "{}: {}: No such file or directory\n",
                    options.cmd_name, file
                ));
                exit_code = 1;
                if options.stop_on_error {
                    return ReadFilesResult {
                        files: result,
                        stderr,
                        exit_code,
                    };
                }
            }
        }
    }

    ReadFilesResult {
        files: result,
        stderr,
        exit_code,
    }
}

/// Concatenates files or stdin in byte-preserving form.
pub fn read_and_concat(
    fs: &VirtualFileSystem,
    cwd: &str,
    files: &[String],
    stdin: ByteString,
    cmd_name: &str,
) -> Result<ByteString, ReadFilesResult> {
    let mut options = ReadFilesOptions::new(cmd_name);
    options.stop_on_error = true;
    let result = read_files(fs, cwd, files, stdin, &options);
    if result.exit_code != 0 {
        return Err(result);
    }
    let mut joined = Vec::new();
    for file in result.files {
        joined.extend_from_slice(file.content.as_bytes());
    }
    Ok(ByteString::from_bytes(joined))
}
