use std::{
    collections::HashSet,
    ffi::OsString,
    path::{Path, PathBuf},
};

pub fn discover_plugin_libraries(plugin_dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut libraries = Vec::new();
    let mut seen_names = HashSet::<OsString>::new();
    for dir in plugin_dirs {
        libraries.extend(discover_plugin_libraries_in_dir(dir, &mut seen_names));
    }
    libraries
}

fn discover_plugin_libraries_in_dir(dir: &Path, seen_names: &mut HashSet<OsString>) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut discovered =
        entries.filter_map(Result::ok).map(|entry| entry.path()).filter(|path| path.is_file()).filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("so")).collect::<Vec<_>>();
    discovered.sort();
    discovered
        .into_iter()
        .filter(|path| {
            let Some(name) = path.file_name() else {
                return false;
            };
            seen_names.insert(name.to_os_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_only_returns_shared_objects() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(temp.path().join("libdemo.so"), b"x").expect("so");
        std::fs::write(temp.path().join("ignored.txt"), b"x").expect("txt");
        std::fs::create_dir_all(temp.path().join("nested")).expect("dir");
        std::fs::write(temp.path().join("nested").join("libnested.so"), b"x").expect("nested so");

        let discovered = discover_plugin_libraries(&[temp.path().to_path_buf()]);
        assert_eq!(discovered, vec![temp.path().join("libdemo.so")]);
    }

    #[test]
    fn discovery_prefers_earlier_directories_for_same_library_name() {
        let first = tempfile::tempdir().expect("first tempdir");
        let second = tempfile::tempdir().expect("second tempdir");

        let first_path = first.path().join("cv-plugin.so");
        let second_path = second.path().join("cv-plugin.so");
        let second_unique = second.path().join("nt4-plugin.so");

        std::fs::write(&first_path, b"first").expect("first plugin");
        std::fs::write(&second_path, b"second").expect("second plugin");
        std::fs::write(&second_unique, b"unique").expect("unique plugin");

        let discovered = discover_plugin_libraries(&[first.path().to_path_buf(), second.path().to_path_buf()]);

        assert_eq!(discovered, vec![first_path, second_unique]);
    }
}
