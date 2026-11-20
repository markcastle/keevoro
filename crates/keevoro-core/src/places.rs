//! Is one place inside another? Asked of real paths (as the computer gives
//! them, after following links), by their parts, never by their letters:
//! `Photos 2` is not inside `Photos`, and `Pictures` is not inside `Pic`.
//! On Windows, where `C:\Photos` and `c:\photos` are the same folder, case
//! is ignored; elsewhere it is not, since there they can be two folders.
//!
//! Pure, so both ways are tested on every computer.

use std::path::{Component, Path};

/// True when `path` is `folder` itself or anything beneath it, as this
/// computer's file system compares names.
pub fn within(path: &Path, folder: &Path) -> bool {
    within_by(path, folder, cfg!(windows))
}

/// True when `a` and `b` are the same place, as this computer's file
/// system compares names.
pub fn same_place(a: &Path, b: &Path) -> bool {
    within(a, b) && within(b, a)
}

/// [`within`], told whether case counts.
pub fn within_by(path: &Path, folder: &Path, ignore_case: bool) -> bool {
    let parts = |p: &'_ Path| -> Vec<String> {
        p.components()
            // A trailing separator or a `.` changes nothing.
            .filter(|c| !matches!(c, Component::CurDir))
            .map(|c| {
                let part = c.as_os_str().to_string_lossy();
                if ignore_case {
                    part.to_lowercase()
                } else {
                    part.into_owned()
                }
            })
            .collect()
    };
    let (path, folder) = (parts(path), parts(folder));
    !folder.is_empty() && path.len() >= folder.len() && path[..folder.len()] == folder[..]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn p(text: &str) -> PathBuf {
        PathBuf::from(text)
    }

    #[test]
    fn a_folder_holds_itself_and_what_is_beneath_it() {
        for ignore_case in [false, true] {
            assert!(within_by(&p("/home/mark/Photos"), &p("/home/mark/Photos"), ignore_case));
            assert!(within_by(
                &p("/home/mark/Photos/2019/a.jpg"),
                &p("/home/mark/Photos"),
                ignore_case
            ));
            assert!(within_by(
                &p("/home/mark/Photos/"),
                &p("/home/mark/Photos"),
                ignore_case
            ));
            assert!(
                !within_by(&p("/home/mark"), &p("/home/mark/Photos"), ignore_case),
                "not its parent"
            );
        }
    }

    #[test]
    fn a_sibling_with_a_similar_name_is_never_inside() {
        for ignore_case in [false, true] {
            assert!(!within_by(
                &p("/home/mark/Photos 2"),
                &p("/home/mark/Photos"),
                ignore_case
            ));
            assert!(!within_by(
                &p("/home/mark/Photos 2/a.jpg"),
                &p("/home/mark/Photos"),
                ignore_case
            ));
            assert!(!within_by(
                &p("/home/mark/Photosynthesis"),
                &p("/home/mark/Photos"),
                ignore_case
            ));
            assert!(!within_by(&p("/home/mark/Pic"), &p("/home/mark/Pictures"), ignore_case));
        }
        assert!(
            !within_by(&p("/anything"), &p(""), false),
            "nothing is inside no folder"
        );
    }

    #[test]
    fn case_is_ignored_only_where_the_file_system_ignores_it() {
        assert!(within_by(&p("/home/mark/PHOTOS/a.jpg"), &p("/home/mark/Photos"), true));
        assert!(!within_by(
            &p("/home/mark/PHOTOS/a.jpg"),
            &p("/home/mark/Photos"),
            false
        ));
        assert_eq!(
            within(&p("/home/mark/PHOTOS"), &p("/home/mark/Photos")),
            cfg!(windows),
            "Windows ignores case; Macs and Linux, as Keevoro sees them, do not"
        );
        assert!(same_place(&p("/a/b/"), &p("/a/b")));
        assert!(!same_place(&p("/a/b/c"), &p("/a/b")));
    }
}
