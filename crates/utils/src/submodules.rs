use std::{
    fs::read_to_string,
    io,
    path::{Path, PathBuf},
};

pub const PORTAL_SPEC_TESTS_SUBMODULE_PATH: [&str; 2] =
    ["../../portal-spec-tests", "../../../portal-spec-tests"];

pub fn portal_spec_tests_file_path<P: AsRef<Path>>(path: P) -> PathBuf {
    for submodule_path in PORTAL_SPEC_TESTS_SUBMODULE_PATH {
        if std::fs::exists(submodule_path).expect("to check whether submodule path exists") {
            return PathBuf::from(submodule_path).join(path);
        }
    }

    panic!("Submodule directory not found")
}

/// Reads a file from a "portal-spec-tests" submodule.
pub fn read_portal_spec_tests_file<P: AsRef<Path>>(path: P) -> io::Result<String> {
    read_to_string(portal_spec_tests_file_path(path))
}
