#![feature(once_cell_try)]

use std::env;
use std::fs::{DirEntry, File};
use std::io::{BufRead as _, BufReader, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use anyhow::{Context as _, Result, bail};
use env_logger::Env;
use log::*;
use regex::Regex;

// Path will be different on other platforms
#[cfg(target_os = "windows")]
const LOCAL_ICON_DIR: &str = r"C:\Program Files (x86)\Steam\steam\games\";

#[tokio::main]
async fn main() -> Result<()> {
    // Set up logging
    let env = Env::default()
        .default_filter_or("info")
        .default_write_style_or("always");
    env_logger::try_init_from_env(env)?;

    // Set up SIGINT monitoring
    let check_sigint = setup_sigint_checker()?;

    // Log the directory being processed
    let dir_with_shortcuts = env::current_dir()?;
    info!(
        "Processing shortcuts in {}",
        dir_with_shortcuts.as_path().to_string_lossy()
    );

    // Make sure the icon directory exists
    let local_icon_dir = PathBuf::from(LOCAL_ICON_DIR);
    if !local_icon_dir.is_dir() {
        bail!("Specified local icon directory is not actually a directory");
    }

    // Loop through the shortcut directory and process all shortcuts
    for entry in dir_with_shortcuts.read_dir()? {
        // Check if the script needs to exit
        check_sigint()?;

        let entry = entry?;

        // Extract the game ID and icon filename from the shortcut
        let Some((game_id, icon_filename)) = extract_game_id_and_icon_filename(entry)? else {
            continue;
        };

        // Make sure the icon doesn't already exist
        let icon_path = local_icon_dir.join(&icon_filename);
        if icon_path.exists() {
            info!("Icon already exists for game #{game_id}");
            continue;
        }

        // Build the CDN URL for the icon
        let icon_url = format!("https://cdn.cloudflare.steamstatic.com/steamcommunity/public/images/apps/{game_id}/{icon_filename}");

        // Download the icon
        let body = reqwest::get(icon_url).await?.bytes().await?;

        // Save the icon locally
        let mut file = File::create_new(icon_path).context("Failed to save icon file")?;
        file.write_all(&body)
            .context("Failed to write ICO contents to the newly created file")?;
    }

    Ok(())
}

/// Extract steam game ID and icon filename from `.url` shortcut files.
fn extract_game_id_and_icon_filename(entry: DirEntry) -> Result<Option<(String, String)>> {
    // Bail on unexpected data in the filename
    let Ok(filename) = entry.file_name().into_string() else {
        bail!("Filename contains invalid unicode data");
    };

    // Skip non-shortcut files
    #[cfg(not(target_os = "windows"))]
    bail!("Other platforms won't have `.url` files");
    let metadata = entry.metadata().context("Failed to read metadata")?;
    if metadata.is_dir() {
        warn!("Skipping directory `{filename}`");
        return Ok(None);
    } else if metadata.is_symlink() {
        warn!("Skipping symlink `{filename}`");
        return Ok(None);
    } else if !metadata.is_file() {
        warn!("Skipping non-file `{filename}`");
        return Ok(None);
    } else if !filename.ends_with(".url") {
        warn!("Skipping non-shortcut file `{filename}`");
        return Ok(None);
    }

    // Build the regex for extracting the steam game ID from the shortcut URL
    static GAME_ID_REGEX: OnceLock<Regex> = OnceLock::new();
    #[cfg(not(target_os = "windows"))]
    bail!("Format of entry may be different on other platforms");
    let game_id_regex =
        GAME_ID_REGEX.get_or_try_init(|| Regex::new(r"^URL=steam://rungameid/(\d+)$"))?;

    // Build the regex for extracting the icon path from the shortcut IconFile
    static ICON_PATH_REGEX: OnceLock<Regex> = OnceLock::new();
    #[cfg(not(target_os = "windows"))]
    bail!("Format of entry may be different on other platforms");
    let icon_path_regex =
        ICON_PATH_REGEX.get_or_try_init(|| Regex::new(r"^IconFile=(.*\\)([^.\\]+\.ico)$"))?;

    // Parse (naively) the shortcut file
    let file = File::open(entry.path()).context("Failed to open file")?;
    let lines = BufReader::new(file).lines();
    let mut game_id: Option<String> = None;
    let mut icon_filename: Option<String> = None;
    let mut in_shortcut_section = false;
    for line in lines {
        let line = line.context("Failed to read line")?;

        #[cfg(not(target_os = "windows"))]
        bail!("Parsing the file will be different on other platforms");

        // Find and extract the game ID and icon path
        // from the "InternetShortcut" section within the shortcut file
        if &line == "[InternetShortcut]" {
            in_shortcut_section = true;
        } else if !in_shortcut_section {
            continue;
        } else if line.starts_with("[") {
            in_shortcut_section = false;
        } else if let Some(captures) = game_id_regex.captures(&line) {
            if game_id.is_some() {
                bail!("Game ID already set for shortcut: {filename}");
            }

            game_id = Some(
                captures
                    .get(1)
                    .context("Failed to extract icon path")?
                    .as_str()
                    .to_owned(),
            );
        } else if let Some(captures) = icon_path_regex.captures(&line) {
            if icon_filename.is_some() {
                bail!("Icon path and/or name already set for shortcut: {filename}");
            }

            // Make sure the specified icon directory matches the one being written to
            let icon_dir = captures
                .get(1)
                .context("Failed to extract icon path")?
                .as_str()
                .to_owned();
            if icon_dir != LOCAL_ICON_DIR {
                bail!("Unrecognized icon directory `{icon_dir}` for shortcut: {filename}");
            }

            icon_filename = Some(
                captures
                    .get(2)
                    .context("Failed to extract icon path")?
                    .as_str()
                    .to_owned(),
            );
        }
    }

    let (Some(game_id), Some(icon_filename)) = (game_id, icon_filename) else {
        bail!("Shortcut could not be parsed or was not a Steam shortcut file: {filename}");
    };

    Ok(Some((game_id, icon_filename)))
}

/// Basic SIGINT handling.
/// The returned callback will return an error if the script needs to bail.
///
/// Setup:
///
/// ```rust
/// let check_sigint = setup_sigint_checker()?;
/// ```
///
/// Usage (anywhere exiting is ideal):
///
/// ```rust
/// check_sigint()?;
/// ```
fn setup_sigint_checker() -> Result<impl Fn() -> Result<()>> {
    info!("Press `Ctrl` + `c` at any time to exit");

    let sigint_received: Arc<AtomicBool> = AtomicBool::new(false).into();

    let sigint_received_write = sigint_received.clone();
    ctrlc::set_handler(move || {
        info!("SIGINT (`Ctrl` + `c`) received, exiting...");
        sigint_received_write.store(true, Ordering::Relaxed);
    })
    .context("Error setting Ctrl-C handler")?;

    let sigint_checker = move || -> Result<()> {
        if sigint_received.load(Ordering::Relaxed) {
            bail!("Stopping script due to SIGINT")
        } else {
            Ok(())
        }
    };

    Ok(sigint_checker)
}
