use std::{io::Read, os::unix::fs::PermissionsExt, path::PathBuf};

use clap::{Parser, Subcommand};
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
        let progress_bar = multi_progress.add(ProgressBar::new(100));
        progress_bar.set_style(progress_style.clone());
        progress_bar.set_message(format!("Installing {}", package.name()));

        let loc = config.linux_x86_64.location.clone();
        let pkg = package.clone();
        let handle = std::thread::spawn(move || {
            install_pacakge(&loc, &pkg, progress_bar);
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }
}

fn install_pacakge(location: &PathBuf, package: &PackageConfig, pb: ProgressBar) {
    match package {
        PackageConfig::Archive { name, bin, archive } => {
            let bytes = download_with_progress(archive, pb).unwrap();

            let path_buf = PathBuf::from(archive);
            let ext = path_buf.extension().unwrap().to_str().unwrap();

            match ext {
                "tar.gz" => {
                    let tar = flate2::read::GzDecoder::new(std::io::Cursor::new(bytes));
                    let mut archive = tar::Archive::new(tar);
                    let entry = archive
                        .entries()
                        .unwrap()
                        .find(|entry| {
                            entry.as_ref().unwrap().path().unwrap().to_str().unwrap() == bin
                        })
                        .unwrap()
                        .unwrap();

                    let data: Vec<u8> = entry.bytes().map(|b| b.unwrap()).collect();

                    install(location, name, data.as_ref());
                }
                "zip" => {
                    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
                    let mut entry = archive.by_name(bin).unwrap();

                    let mut buff = vec![];
                    entry.read_to_end(&mut buff).unwrap();

                    install(location, name, buff.as_ref());
                }
                _ => panic!("Unsupported archive format"),
            }
        }
        PackageConfig::Binary { name, url } => {
            let bytes = download_with_progress(url, pb).unwrap();
            install(location, name, bytes.as_ref());
        }
    }
}

fn download_with_progress(url: &str, pb: ProgressBar) -> eyre::Result<Vec<u8>> {
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

    for chunk in response.bytes().into_iter() {
        buf.extend_from_slice(&chunk);
        downloaded += chunk.len() as u64;
        pb.set_position(downloaded);
    }

    pb.finish_with_message(format!("Downloaded {}", url));

    Ok(buf)
}

fn install(location: &PathBuf, name: &str, data: &[u8]) {
    std::fs::write(location.join(name), data).unwrap();

    // Add executable permissions
    std::fs::set_permissions(location.join(name), std::fs::Permissions::from_mode(0o755)).unwrap();
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
