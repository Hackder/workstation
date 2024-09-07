use std::{io::Read, os::unix::fs::PermissionsExt, path::PathBuf};

use clap::{Parser, Subcommand};
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
    for package in &config.linux_x86_64.packages {
        install_pacakge(&config.linux_x86_64.location, package);
    }
}

fn install_pacakge(location: &PathBuf, package: &PackageConfig) {
    match package {
        PackageConfig::Archive { name, bin, archive } => {
            let bytes = reqwest::blocking::get(archive).unwrap().bytes().unwrap();

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
            let bytes = reqwest::blocking::get(url).unwrap().bytes().unwrap();
            install(location, name, bytes.as_ref());
        }
    }
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

#[derive(Deserialize, Debug)]
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
