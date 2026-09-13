use super::*;
use std::fs;
use std::time::Duration;

fn write_change(dir: &Path, name: &str) {
    let change_dir = dir.join(".op").join("changes").join(name);
    fs::create_dir_all(&change_dir).expect("mkdir");
    fs::write(
        change_dir.join("change.yaml"),
        format!("name: {name}\nphase: build\n"),
    )
    .expect("write change.yaml");
}

#[test]
fn newest_change_dir_picks_the_most_recently_touched_change() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    write_change(root, "older-change");
    std::thread::sleep(Duration::from_millis(25));
    write_change(root, "newer-change");

    let dir = newest_change_dir(root).expect("a change directory");
    assert_eq!(
        dir.file_name()
            .map(|name| name.to_string_lossy().into_owned()),
        Some("newer-change".to_string())
    );
}

#[test]
fn newest_change_dir_skips_directories_without_a_change_yaml() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    fs::create_dir_all(root.join(".op").join("changes").join("not-a-change")).expect("mkdir");
    assert!(newest_change_dir(root).is_none());
}

#[test]
fn newest_change_dir_without_an_op_directory_is_none() {
    let temp = tempfile::tempdir().expect("tempdir");
    assert!(newest_change_dir(temp.path()).is_none());
}

#[test]
fn active_change_dir_handles_a_missing_working_dir() {
    assert!(active_change_dir(None).is_none());
}

#[test]
fn read_text_optional_distinguishes_missing_from_present() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("tasks.md");
    assert_eq!(read_text_optional(&path), None);

    fs::write(&path, "hello").expect("write");
    assert_eq!(read_text_optional(&path), Some("hello".to_string()));
}
