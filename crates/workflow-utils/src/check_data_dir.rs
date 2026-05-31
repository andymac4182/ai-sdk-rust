use std::{
    env, fs,
    path::{Component, Path, PathBuf},
};

pub const POSSIBLE_WORKFLOW_DATA_PATHS: [&str; 3] =
    [".next/workflow-data", ".workflow-data", "workflow-data"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowDataDirInfo {
    pub data_dir: Option<PathBuf>,
    pub project_dir: PathBuf,
    pub short_name: String,
    pub error: Option<String>,
}

pub fn get_dir_short_name(project_dir: impl AsRef<Path>) -> String {
    let path = project_dir.as_ref();
    let parts = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>();

    match parts.len() {
        0 => std::path::MAIN_SEPARATOR.to_string(),
        1 => parts[0].clone(),
        length => parts[(length - 2)..].join("/"),
    }
}

pub fn find_workflow_data_dir(cwd: &str) -> WorkflowDataDirInfo {
    let absolute_cwd = to_absolute_path(cwd);

    if !directory_exists(&absolute_cwd) {
        return WorkflowDataDirInfo {
            project_dir: absolute_cwd.clone(),
            data_dir: None,
            short_name: get_dir_short_name(&absolute_cwd),
            error: Some("Folder does not exist".to_owned()),
        };
    }

    if let Some(project_dir) = workflow_data_project_dir(&absolute_cwd) {
        return WorkflowDataDirInfo {
            project_dir: project_dir.clone(),
            data_dir: Some(absolute_cwd),
            short_name: get_dir_short_name(project_dir),
            error: None,
        };
    }

    for data_path in POSSIBLE_WORKFLOW_DATA_PATHS {
        let full_path = absolute_cwd.join(data_path);
        if directory_exists(&full_path) {
            return WorkflowDataDirInfo {
                project_dir: absolute_cwd.clone(),
                data_dir: Some(normalize_path(full_path)),
                short_name: get_dir_short_name(&absolute_cwd),
                error: None,
            };
        }
    }

    let root = PathBuf::from(std::path::MAIN_SEPARATOR.to_string());
    let mut current_dir = absolute_cwd.clone();
    while current_dir != root {
        for data_path in POSSIBLE_WORKFLOW_DATA_PATHS {
            let full_path = current_dir.join(data_path);
            if directory_exists(&full_path) {
                return WorkflowDataDirInfo {
                    project_dir: current_dir.clone(),
                    data_dir: Some(normalize_path(full_path)),
                    short_name: get_dir_short_name(&current_dir),
                    error: None,
                };
            }
        }

        if !current_dir.pop() {
            break;
        }
    }

    WorkflowDataDirInfo {
        project_dir: absolute_cwd.clone(),
        data_dir: None,
        short_name: get_dir_short_name(absolute_cwd),
        error: None,
    }
}

fn workflow_data_project_dir(absolute_path: &Path) -> Option<PathBuf> {
    for suffix in POSSIBLE_WORKFLOW_DATA_PATHS {
        let suffix_parts = suffix.split('/').collect::<Vec<_>>();
        if path_ends_with(absolute_path, &suffix_parts) {
            let mut project_dir = absolute_path.to_path_buf();
            for _ in suffix_parts {
                project_dir.pop();
            }
            return Some(project_dir);
        }
    }
    None
}

fn path_ends_with(path: &Path, suffix_parts: &[&str]) -> bool {
    let path_parts = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>();

    path_parts.len() >= suffix_parts.len()
        && path_parts[(path_parts.len() - suffix_parts.len())..]
            .iter()
            .map(String::as_str)
            .eq(suffix_parts.iter().copied())
}

fn directory_exists(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|metadata| metadata.is_dir())
}

fn to_absolute_path(path: &str) -> PathBuf {
    let expanded = expand_tilde(path);
    let raw_path = PathBuf::from(&expanded);
    let absolute = if expanded.is_empty() {
        env::current_dir().expect("current directory must be readable")
    } else if raw_path.is_absolute() {
        raw_path
    } else {
        env::current_dir()
            .expect("current directory must be readable")
            .join(raw_path)
    };
    normalize_path(absolute)
}

fn expand_tilde(path: &str) -> String {
    let Some(rest) = path.strip_prefix('~') else {
        return path.to_owned();
    };
    let home = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("~"));
    let rest = rest.trim_start_matches(['/', '\\']);
    home.join(rest).to_string_lossy().into_owned()
}

fn normalize_path(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(Path::new(std::path::MAIN_SEPARATOR_STR)),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn path_string(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn upstream_check_data_dir_cases() {
        let temp = TempDir::new().unwrap();
        let test_dir = temp.path();

        for (relative, expected) in [
            (".next/workflow-data", ".next/workflow-data"),
            (".workflow-data", ".workflow-data"),
            ("workflow-data", "workflow-data"),
        ] {
            let project_dir = test_dir.join(format!("contains-{}", relative.replace('/', "_")));
            let data_path = project_dir.join(relative);
            fs::create_dir_all(&data_path).unwrap();
            let result = find_workflow_data_dir(&path_string(&project_dir));
            assert_eq!(result.data_dir, Some(project_dir.join(expected)));
            assert_eq!(result.project_dir, project_dir);
            assert!(!result.short_name.is_empty());
        }

        let preferred_project = test_dir.join("preferred");
        for path in POSSIBLE_WORKFLOW_DATA_PATHS {
            fs::create_dir_all(preferred_project.join(path)).unwrap();
        }
        let result = find_workflow_data_dir(&path_string(&preferred_project));
        assert_eq!(
            result.data_dir,
            Some(preferred_project.join(".next/workflow-data"))
        );

        for relative in [".next/workflow-data", ".workflow-data", "workflow-data"] {
            let project_dir = test_dir.join(format!("itself-{relative}"));
            let data_path = project_dir.join(relative);
            fs::create_dir_all(&data_path).unwrap();
            let result = find_workflow_data_dir(&path_string(&data_path));
            assert_eq!(result.data_dir, Some(data_path));
            assert_eq!(result.project_dir, project_dir);
        }

        let parent_project = test_dir.join("myproject");
        let parent_data_path = parent_project.join(".next/workflow-data");
        let sub_dir = parent_project.join("src/components");
        fs::create_dir_all(&parent_data_path).unwrap();
        fs::create_dir_all(&sub_dir).unwrap();
        let result = find_workflow_data_dir(&path_string(&sub_dir));
        assert_eq!(result.data_dir, Some(parent_data_path));
        assert_eq!(result.project_dir, parent_project);

        let deep_project = test_dir.join("deep-project");
        let deep_data_path = deep_project.join(".workflow-data");
        let deep_dir = deep_project.join("src/app/api/workflows");
        fs::create_dir_all(&deep_data_path).unwrap();
        fs::create_dir_all(&deep_dir).unwrap();
        let result = find_workflow_data_dir(&path_string(&deep_dir));
        assert_eq!(result.data_dir, Some(deep_data_path));
        assert_eq!(result.project_dir, deep_project);

        let empty = test_dir.join("empty");
        fs::create_dir_all(&empty).unwrap();
        let result = find_workflow_data_dir(&path_string(&empty));
        assert_eq!(result.data_dir, None);
        assert_eq!(result.project_dir, empty);
        assert!(!result.short_name.is_empty());

        let unrelated = test_dir.join("unrelated");
        fs::create_dir_all(unrelated.join("src")).unwrap();
        fs::create_dir_all(unrelated.join("node_modules")).unwrap();
        let result = find_workflow_data_dir(&path_string(&unrelated));
        assert_eq!(result.data_dir, None);
        assert_eq!(result.project_dir, unrelated);

        let relative_project = test_dir.join("relative-test");
        let relative_data = relative_project.join(".workflow-data");
        fs::create_dir_all(&relative_data).unwrap();
        let current_dir = env::current_dir().unwrap();
        let relative_input = relative_project
            .strip_prefix(&current_dir)
            .unwrap_or(&relative_project);
        let result = find_workflow_data_dir(&path_string(relative_input));
        assert_eq!(result.data_dir, Some(relative_data));
        assert_eq!(result.project_dir, relative_project);

        let absolute_project = test_dir.join("absolute-test");
        let absolute_data = absolute_project.join(".next/workflow-data");
        fs::create_dir_all(&absolute_data).unwrap();
        let result = find_workflow_data_dir(&path_string(&absolute_project));
        assert_eq!(result.data_dir, Some(absolute_data.clone()));
        assert_eq!(result.project_dir, absolute_project.clone());
        assert!(result.project_dir.is_absolute());
        assert!(result.data_dir.unwrap().is_absolute());

        let home_test_dir = PathBuf::from(env::var_os("HOME").unwrap())
            .join(format!(".workflow-test-{}", std::process::id()));
        let home_data = home_test_dir.join(".workflow-data");
        fs::create_dir_all(&home_data).unwrap();
        let tilde_result =
            find_workflow_data_dir(&format!("~/.workflow-test-{}", std::process::id()));
        fs::remove_dir_all(&home_test_dir).unwrap();
        assert_eq!(tilde_result.data_dir, Some(home_data));
        assert_eq!(tilde_result.project_dir, home_test_dir);

        let normalize_project = test_dir.join("normalize-test");
        let normalize_data = normalize_project.join(".workflow-data");
        fs::create_dir_all(&normalize_data).unwrap();
        let weird_path = normalize_project.join("subdir/.././.");
        let result = find_workflow_data_dir(&path_string(&weird_path));
        assert_eq!(result.data_dir, Some(normalize_data));
        assert_eq!(result.project_dir, normalize_project);

        let named_project = test_dir.join("code/myproject");
        fs::create_dir_all(named_project.join(".workflow-data")).unwrap();
        let result = find_workflow_data_dir(&path_string(&named_project));
        assert_eq!(result.short_name, "code/myproject");

        let shallow_project = test_dir.join("myproject");
        fs::create_dir_all(shallow_project.join(".workflow-data")).unwrap();
        let result = find_workflow_data_dir(&path_string(&shallow_project));
        let parts = result.short_name.split('/').collect::<Vec<_>>();
        assert!(parts.len() <= 2);
        assert_eq!(parts[parts.len() - 1], "myproject");

        let nested_project = test_dir.join("a/b/c/d/project");
        fs::create_dir_all(nested_project.join(".workflow-data")).unwrap();
        let result = find_workflow_data_dir(&path_string(&nested_project));
        assert_eq!(result.short_name, "d/project");

        let result = find_workflow_data_dir("/this/path/does/not/exist");
        assert_eq!(result.data_dir, None);
        assert_eq!(result.error.as_deref(), Some("Folder does not exist"));

        let result = find_workflow_data_dir("");
        assert!(result.project_dir.is_absolute());
        assert!(!result.short_name.is_empty());

        let trailing_project = test_dir.join("trailing");
        fs::create_dir_all(trailing_project.join(".workflow-data")).unwrap();
        let result = find_workflow_data_dir(
            &(path_string(&trailing_project) + std::path::MAIN_SEPARATOR_STR),
        );
        assert_eq!(
            result.data_dir,
            Some(trailing_project.join(".workflow-data"))
        );
        assert_eq!(result.project_dir, trailing_project);

        let fake_path = test_dir.join("fake/.next/workflow-data");
        let result = find_workflow_data_dir(&path_string(&fake_path));
        assert_eq!(result.data_dir, None);
        assert_eq!(result.error.as_deref(), Some("Folder does not exist"));
    }
}
