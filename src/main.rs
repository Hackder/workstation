use std::{io::Read, os::unix::fs::PermissionsExt, path::PathBuf};

use clap::{Parser, Subcommand};
use eyre::Context;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use serde::Deserialize;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Sets a custom config file
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    // /// Turn debugging information on
    // #[arg(short, long, action = clap::ArgAction::Count)]
    // debug: u8,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Set up this computer with the workstation config
    Setup,
}

fn main() {
    let cli = Cli::parse();

    let config_path = cli
        .config
        .unwrap_or_else(|| PathBuf::from("workstation.toml"));

    match cli.command {
        Command::Setup => {
            let config: Config =
                toml::from_str(&std::fs::read_to_string(config_path).unwrap()).unwrap();

            setup(&config);
        }
    }
}

fn setup(config: &Config) {
    let multi_progress = MultiProgress::new();
    let progress_style = ProgressStyle::with_template(
        "[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}",
    )
    .unwrap()
    .progress_chars("##-");

    let mut handles = vec![];

    for package in config.linux_x86_64.packages.iter() {
        let progress_bar = multi_progress.add(ProgressBar::new(0));
        progress_bar.set_style(progress_style.clone());
        progress_bar.set_message(format!("Installing {}", package.name()));

        let loc = config.linux_x86_64.location.clone();
        let pkg = package.clone();
        let pb = progress_bar.clone();
        let handle = std::thread::spawn(move || {
            match install_package(&loc, &pkg, pb)
                .with_context(|| format!("Installing {}", pkg.name()))
            {
                Ok(_) => {}
                Err(e) => {
                    progress_bar.finish_with_message(format!(
                        "Error installing {}: {:?}",
                        pkg.name(),
                        e
                    ));
                }
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }
}

fn install_package(
    location: &PathBuf,
    package: &PackageConfig,
    pb: ProgressBar,
) -> eyre::Result<()> {
    match package {
        PackageConfig::Archive { name, bin, archive } => {
            let bytes = download_with_progress(archive, &pb)
                .with_context(|| format!("Failed to download {}", name))?;
            pb.finish_with_message(format!("Downloaded {}", name));

            if archive.ends_with(".tar.gz") {
                let tar = flate2::read::GzDecoder::new(std::io::Cursor::new(bytes));
                let mut archive = tar::Archive::new(tar);
                let entry = archive
                    .entries()?
                    .find(|entry| {
                        entry
                            .as_ref()
                            .expect("entry exists")
                            .path()
                            .expect("entry has path")
                            .to_str()
                            .expect("entry path is string")
                            == bin
                    })
                    .ok_or(eyre::eyre!("Entry not found"))
                    .with_context(|| "Searching for entry")??;

                let data: Vec<u8> = entry.bytes().map(|b| b.unwrap()).collect();

                install(location, name, data.as_ref()).with_context(|| format!("Installing"))?;
            } else if archive.ends_with(".zip") {
                let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
                let mut entry = archive.by_name(bin)?;

                let mut buff = vec![];
                entry.read_to_end(&mut buff)?;

                install(location, name, buff.as_ref()).with_context(|| "Installing")?;
            } else {
                eyre::bail!("Unsupported archive format");
            }
        }
        PackageConfig::Binary { name, url } => {
            let bytes = download_with_progress(url, &pb).with_context(|| "Downloading")?;
            pb.finish_with_message(format!("Downloaded {}", name));
            install(location, name, bytes.as_ref()).with_context(|| "Installing")?;
        }
    }

    Ok(())
}

fn download_with_progress(url: &str, pb: &ProgressBar) -> eyre::Result<Vec<u8>> {
    let client = reqwest::blocking::Client::new();
    let response = client.get(url).send()?;

    if !response.status().is_success() {
        eyre::bail!("Failed to download {}", url);
    }

    let total_length = response
        .content_length()
        .ok_or(eyre::eyre!("Failed to get content length"))?;

    let mut buf = Vec::with_capacity(total_length as usize);
    let mut downloaded = 0;

    pb.set_length(total_length);

    for chunk in response.bytes().into_iter() {
        buf.extend_from_slice(&chunk);
        downloaded += chunk.len() as u64;
        pb.set_position(downloaded);
    }

    Ok(buf)
}

fn get_install_path(location: &PathBuf, name: &str) -> eyre::Result<PathBuf> {
    let path = location.join(name);
    let path = expanduser::expanduser(path.to_str().expect("string path"))?;
    Ok(PathBuf::from(path))
}

fn install(location: &PathBuf, name: &str, data: &[u8]) -> eyre::Result<()> {
    let path = get_install_path(location, name)?;

    std::fs::write(&path, data)?;

    // Add executable permissions
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;

    Ok(())
}

#[derive(Deserialize, Debug)]
struct Config {
    linux_x86_64: ArchConfig,
}

#[derive(Deserialize, Debug)]
struct ArchConfig {
    location: PathBuf,
    packages: Vec<PackageConfig>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
enum PackageConfig {
    Archive {
        name: String,
        bin: String,
        archive: String,
    },
    Binary {
        name: String,
        url: String,
    },
}

impl PackageConfig {
    pub fn name(&self) -> &str {
        match self {
            PackageConfig::Archive { name, .. } => name,
            PackageConfig::Binary { name, .. } => name,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_install_path() {
        let location = PathBuf::from("~/.local/bin");
        let name = "test";
        let expected = PathBuf::from("/Users/jurajpetras/.local/bin/test");

        let path = get_install_path(&location, name);

        assert_eq!(path.unwrap(), expected);
    }
}
