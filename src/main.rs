use structopt::StructOpt;
use std::fs::File;
use std::error::Error;
use std::{fmt, fs};
use std::fmt::Formatter;
use std::io::{ErrorKind, BufReader, BufRead, Read, BufWriter, Write};
use dialoguer::Select;
use std::path::Path;
use chrono::Local;
use std::collections::HashMap;

const URL: &'static str = "https://raw.githubusercontent.com/ghacksuserjs/ghacks-user.js/master/user.js";

#[derive(Debug, StructOpt)]
#[structopt(name = "args", about = "The inputs for the script")]
struct Arguments {
    #[structopt(short, long)]
    unattended: bool,
    #[structopt(short, long)]
    minify: bool,
    #[structopt(long = "singlebackup")]
    single_backup: bool,
}

#[derive(Debug)]
enum UpdaterError {
    MissingScript,
    MissingOverrides,
    ParseError(String),
    IoError(std::io::Error),
    NetworkError(reqwest::Error)
}

impl From<std::io::Error> for UpdaterError {
    fn from(e: std::io::Error) -> Self {
        UpdaterError::IoError(e)
    }
}

impl Error for UpdaterError { }
impl fmt::Display for UpdaterError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            UpdaterError::MissingScript => write!(f, "user.js not detected in the current directory."),
            UpdaterError::MissingOverrides => write!(f, "user-overrides.js not detected in the current directory."),
            UpdaterError::ParseError(context) => write!(f, "Error parsing input: {}", context),
            UpdaterError::IoError(e) => write!(f, "IO Error: {}", e),
            UpdaterError::NetworkError(e) => write!(f, "Network error: {}", e)
        }
    }
}

#[derive(Debug, PartialEq)]
struct Version {
    name: String,
    version: String,
    date: String
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {} from {}", self.name, self.version, self.date)
    }
}

fn get_version_info(file: &mut BufReader<File>) -> Result<Version, UpdaterError> {
    let mut void = String::new();
    file.read_line(&mut void)?; // Start of comment

    let mut name = String::new();
    file.read_line(&mut name)?; // Name in format '* name: ghacks user.js'
    let name = name.split("name: ")
        .skip(1).next().unwrap();
    let mut date = String::new();
    file.read_line(&mut date)?; // Date in format '* date: 14 February 2020'
    let date = date
        .split("date: ")
        .skip(1).next().unwrap();
    let mut version = String::new();
    file.read_line(&mut version)?;
    let version = version.split("version ")
        .skip(1).next().unwrap();

    if !name.contains("ghacks") {
        return Err(UpdaterError::ParseError("Version not recognized".to_string()));
    }

    Ok(Version {
        name: name.to_string().replace("\n", ""),
        version: version.to_string().replace("\n", ""),
        date: date.to_string().replace("\n", "")
    })
}

async fn fetch_script() -> Result<String, UpdaterError> {
    println!("Retrieving latest user.js file from github repository...");
    let res = reqwest::get(URL).await
        .map_err(UpdaterError::NetworkError)?;

    Ok(res.text().await.unwrap())
}

fn extract_pref(line: &String) -> (String, String) {
    let pref: &str = line[10..].split(")").next().unwrap();
    let mut pref_iter = pref.split(",");
    let key: String = pref_iter.next().unwrap().replace("\"", "");
    let key = key.trim();
    let value: &str = pref_iter.next().unwrap().trim();
    (key.to_string(), value.to_string())
}

fn minify(original: String, overrides: String) -> Result<String, UpdaterError> {
    let original_reader = BufReader::new(original.as_bytes());
    let overrides_reader = BufReader::new(overrides.as_bytes());

    let original_lines: Vec<String> = original_reader.lines()
        .filter_map(Result::ok)
        .collect();
    let overrides_lines: Vec<String> = overrides_reader.lines()
        .filter_map(Result::ok)
        .collect();

    let header: Vec<String> = original_lines
        .iter()
        .take(76)
        .map(Clone::clone)
        .collect();
    let header = header.join("\n");

    let mut entries: HashMap<String, String> = original_lines.iter()
        .filter(|line| line.starts_with("user_pref("))
        .map(extract_pref)
        .collect();

    let override_entries = overrides_lines.iter()
        .filter(|line| line.starts_with("user_pref("))
        .map(extract_pref);

    for (key, value) in override_entries {
        entries.insert(key, value);
    }

    let prefs: Vec<String> = entries.into_iter()
        .map(|(key, value)| format!("user_pref(\"{}\", {});", key, value))
        .collect();
    let prefs = prefs.join("\n");

    Ok(format!("{}\n\n{}", header, prefs))
}

#[tokio::main]
async fn main() -> Result<(), ()> {
    let res = run().await;
    match res {
        Err(e) => {
            eprintln!("An error occurred during execution:\n{}", e);
            Err(())
        }
        Ok(_) => Ok(())
    }
}

async fn run() -> Result<(), UpdaterError> {
    let args: Arguments = Arguments::from_args();

    let version = {
        let file = File::open("user.js")
            .map_err(|e| match e.kind() {
                ErrorKind::NotFound => UpdaterError::MissingScript,
                _ => panic!("Unknown error occurred: {}", e)
            })?;
        let mut file = BufReader::new(file);

        get_version_info(&mut file)?
    };
    println!("Found version: {}", version);

    if !args.unattended {
        println!(r#"
        This batch should be run from your Firefox profile directory.
        It will download the lates version of ghacks user.js from github and then
        append any of your own changes from user-overrides.js to it.
        Visit the wiki for more detailed information.
        "#);
    }

    if !args.unattended {
        let select = Select::new()
            .item("Start")
            .item("Help")
            .item("Exit")
            .interact_opt()?;

        match select {
            None | Some(2) => return Ok(()),
            Some(1) => return show_help(),
            _ => {}
        }
    }

    let new_script = fetch_script().await?;

    let mut user_overrides = String::from("\n");
    let user_overrides_path = Path::new("user-overrides.js");
    if user_overrides_path.exists() {
        let mut user_overrides_file = BufReader::new(File::open(user_overrides_path)?);
        user_overrides_file.read_to_string(&mut user_overrides)?;
    } else {
        return Err(UpdaterError::MissingOverrides);
    }

    {
        let mut new_file = BufWriter::new(File::create("user.js.new")?);
        if args.minify {
            let new_string = minify(new_script, user_overrides)?;
            new_file.write_all(new_string.as_bytes())?;
        } else {
            new_file.write_all(new_script.as_bytes())?;
            new_file.write_all(user_overrides.as_bytes())?;
        }
    }

    let new_version = {
        let file = File::open("user.js.new")?;
        let mut file = BufReader::new(file);

        get_version_info(&mut file)?
    };
    let changed = version == new_version;
    if changed {
        println!(r#"
            Version changed
            Old version: {},
            New version: {}
        "#, version, new_version);
    }

    if changed {
        let current_time = Local::now();
        let time = current_time.format("%Y-%m-%d_%H-%M-%S");
        let backup_name = format!("user-backup-{}.js", time);
        println!("Backing up to {}", backup_name);
        fs::rename("user.js", backup_name)?;
        println!("Renaming new file...");
        fs::rename("user.js.new", "user.js")?;
        println!("Update complete!")
    } else {
        fs::remove_file("user.js.new")?;
        println!("Update completed without any changes");
    }

    Ok(())
}

fn show_help() -> Result<(), UpdaterError> {
    println!(r#"
    Avaliable arguments (case-insensitive):
        -merge

    Merge overrides instead of appending them. Single-line comments and
    _user.js.parrot lines are appended normally. Overrides for inactive
    user.js prefs will be appended. When -Merge and -MultiOverrides are used
    together, a user-overrides-merged.js file is also generated in the root
    directory for quick reference. It contains only the merged data from
    override files and can be safely discarded after updating, or used as the
    new user-overrides.js. When there are conflicting records for the same
    pref, the value of the last one declared will be used. Visit the wiki
    for usage examples and more detailed information.

        -unattended

    Run without user input
    "#);

    Ok(())
}