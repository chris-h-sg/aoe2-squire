use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use rfd::FileDialog;

/// Attempts to find the most recently modified .aoe2record file that was saved
/// after the provided `session_start` time.
/// An optional `base_path_override` can be provided for testing.
pub fn find_latest_replay(
    session_start: SystemTime,
    base_path_override: Option<PathBuf>,
) -> Option<PathBuf> {
    // 1. Determine the base path for AoE2 DE saves
    let is_override = base_path_override.is_some();
    let games_path = match base_path_override {
        Some(path) => path,
        None => {
            let user_profile = std::env::var("USERPROFILE").ok()?;
            Path::new(&user_profile)
                .join("Games")
                .join("Age of Empires 2 DE")
        }
    };

    if !games_path.exists() {
        if !is_override {
            eprintln!(
                "[Discovery] AoE2 DE Games directory not found at: {:?}",
                games_path
            );
        }
        return None;
    }

    // 2. Find the profile directory (it's a long numeric string)
    let savegame_dirs = get_savegame_dirs(&games_path);

    if savegame_dirs.is_empty() {
        eprintln!(
            "[Discovery] No 'savegame' directories found in {:?}",
            games_path
        );
        return None;
    }

    // 3. Scan all found savegame directories for the most recent .aoe2record
    let mut latest_file: Option<(PathBuf, SystemTime)> = None;

    for dir in savegame_dirs {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "aoe2record") {
                    if let Ok(metadata) = fs::metadata(&path) {
                        if let Ok(modified) = metadata.modified() {
                            // Only consider files modified AFTER our capture session started
                            if modified > session_start
                                && (latest_file.is_none()
                                    || modified > latest_file.as_ref().unwrap().1)
                            {
                                latest_file = Some((path, modified));
                            }
                        }
                    }
                }
            }
        }
    }

    latest_file.map(|(path, _)| path)
}

fn get_savegame_dirs(games_path: &Path) -> Vec<PathBuf> {
    let mut savegame_dirs = Vec::new();
    if let Ok(entries) = fs::read_dir(games_path) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                let savegame_path = path.join("savegame");
                if savegame_path.exists() {
                    if let Ok(metadata) = fs::metadata(&savegame_path) {
                        if let Ok(modified) = metadata.modified() {
                            savegame_dirs.push((savegame_path, modified));
                        }
                    }
                }
            }
        }
    }
    // Sort by modification time, descending (most recent first)
    savegame_dirs.sort_by(|a, b| b.1.cmp(&a.1));
    savegame_dirs.into_iter().map(|(path, _)| path).collect()
}

pub fn pick_replay_manually() -> Option<PathBuf> {
    let user_profile = std::env::var("USERPROFILE").ok()?;
    let games_path = Path::new(&user_profile)
        .join("Games")
        .join("Age of Empires 2 DE");

    let savegame_dirs = get_savegame_dirs(&games_path);
    let starting_dir = savegame_dirs.first().cloned().unwrap_or(games_path);

    println!("[Discovery] Opening file picker...");
    FileDialog::new()
        .set_title("Select the recorded game file")
        .add_filter("AoE2 Recorded Game", &["aoe2record"])
        .set_directory(starting_dir)
        .pick_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Local;
    use std::fs;
    use std::io::Write;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_replay_discovery_logic() {
        // 1. Setup a temporary test root
        let test_root = std::env::temp_dir().join(format!(
            "aoe2_test_{}",
            Local::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        if test_root.exists() {
            fs::remove_dir_all(&test_root).unwrap();
        }
        fs::create_dir_all(&test_root).unwrap();

        // 2. Create the mock AoE2 directory structure with two profiles
        let profile_1 = test_root.join("12345");
        let profile_2 = test_root.join("67890");
        let save_dir_1 = profile_1.join("savegame");
        let save_dir_2 = profile_2.join("savegame");

        fs::create_dir_all(&save_dir_1).unwrap();
        fs::create_dir_all(&save_dir_2).unwrap();

        // 3. Create an "Old" replay in Profile 1
        let old_replay = save_dir_1.join("old.aoe2record");
        let mut file = fs::File::create(&old_replay).unwrap();
        file.write_all(b"old").unwrap();

        // Wait a bit to ensure timestamp difference
        thread::sleep(Duration::from_millis(50));
        let session_start = SystemTime::now();
        thread::sleep(Duration::from_millis(50));

        // 4. Create a "New" replay in Profile 2 (the latest one)
        let new_replay = save_dir_2.join("new.aoe2record");
        let mut file = fs::File::create(&new_replay).unwrap();
        file.write_all(b"new").unwrap();

        // 5. Run discovery
        let result = find_latest_replay(session_start, Some(test_root.clone()));

        // 6. Assertions
        assert!(result.is_some(), "Should have found a replay");
        let found_path = result.unwrap();
        assert_eq!(
            found_path.file_name().unwrap(),
            "new.aoe2record",
            "Should have picked the newest file created after session_start"
        );

        // 7. Test "No New Files" scenario
        let future_start = SystemTime::now() + Duration::from_secs(3600);
        let result_none = find_latest_replay(future_start, Some(test_root.clone()));
        assert!(
            result_none.is_none(),
            "Should return None if no files were modified after the start time"
        );

        // Cleanup
        let _ = fs::remove_dir_all(&test_root);
    }
}
