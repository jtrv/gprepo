use clap::{App, Arg};
use git2::{Repository, StatusOptions, StatusShow};
use globset::GlobSetBuilder;
use std::fs::File;
use std::io::{self, stdout, BufReader, BufWriter, Read, Write};
use std::path::Path;
use walkdir::WalkDir;

fn is_binary(file_path: &Path) -> io::Result<bool> {
    let mut buffer = [0; 1024];
    let mut reader = BufReader::new(File::open(file_path)?);
    let mut total_read = 0;

    loop {
        let read = reader.read(&mut buffer)?;

        if read == 0 {
            break;
        }

        if buffer.iter().take(read).any(|&byte| byte == 0) {
            return Ok(true);
        }

        total_read += read;

        if total_read >= 1024 {
            break;
        }
    }

    Ok(false)
}

fn main() -> io::Result<()> {
    let matches = App::new("gptrepo")
        .version("0.1.0")
        .arg(
            Arg::new("repo_path")
                .short('r')
                .long("repo-path")
                .value_name("REPO_PATH")
                .help("Path to the repository")
                .takes_value(true),
        )
        .arg(
            Arg::new("preamble")
                .short('p')
                .long("preamble")
                .value_name("PREAMBLE_PATH")
                .help("Optional path to the preamble file")
                .takes_value(true),
        )
        .arg(
            Arg::new("output")
                .short('o')
                .long("output")
                .value_name("OUTPUT_PATH")
                .help("Output file path (default: stdout)")
                .takes_value(true),
        )
        .arg(
            Arg::new("ignore")
                .short('i')
                .long("ignore")
                .value_name("IGNORE_PATH")
                .help("File paths to ignore")
                .takes_value(true)
                .multiple_occurrences(true),
        )
        .get_matches();

    let repo = match matches.value_of("repo_path") {
        Some(path) => Repository::discover(path).map_err(|e| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("Could not find repository: {:?}", e),
            )
        })?,
        None => {
            let current_dir = std::env::current_dir()?;
            Repository::discover(current_dir).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("Could not find repository: {:?}", e),
                )
            })?
        }
    };

    let repo_path = repo.workdir().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "Could not find repository working directory",
        )
    })?;

    let mut _gitignore = repo
        .statuses(Some(
            StatusOptions::new()
                .include_ignored(false)
                .show(StatusShow::IndexAndWorkdir),
        ))
        .map_err(|e| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("Failed to read gitignore: {:?}", e),
            )
        })?;

    let ignore_list = {
        let mut builder = GlobSetBuilder::new();
        if let Some(ignore_paths) = matches.values_of("ignore") {
            for path in ignore_paths {
                builder.add(path.parse().unwrap());
            }
        }
        // Add patterns for LICENSE and .gitignore
        builder.add("LICENSE".parse().unwrap());
        builder.add(".gitignore".parse().unwrap());
        builder.build().unwrap()
    };

    let mut writer: Box<dyn Write> = match matches.value_of("output") {
        Some(output_path) => Box::new(BufWriter::new(File::create(output_path)?)),
        None => Box::new(BufWriter::new(stdout())),
    };

    if let Some(preamble_path) = matches.value_of("preamble") {
        let mut preamble = String::new();
        File::open(preamble_path)?.read_to_string(&mut preamble)?;
        writeln!(writer, "{}", preamble)?;
    } else {
        writeln!(writer, "The following text is a Git repository formatted in sections that begin with `@@@@ <file-location> @@@@`, followed by a variable amount of lines containing that file's contents, when the text `@@@@@ END @@@@@` is encountered that is the end of the git repository's files. Any further text beyond `@@@@@ END @@@@@` is meant to be interpreted as instructions using the aforementioned Git repository as context.")?;
    }

    for entry in WalkDir::new(&repo_path) {
        let entry = entry?;
        if entry.file_type().is_file() {
            let file_path = entry.path();
            let relative_file_path = file_path.strip_prefix(&repo_path).unwrap();

            let should_ignore = repo.status_should_ignore(relative_file_path).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("Failed to check if path should be ignored: {:?}", e),
                )
            })?;

            if should_ignore || ignore_list.is_match(relative_file_path) {
                continue;
            }

            if is_binary(file_path)? {
                continue;
            }

            writeln!(writer, "@@@@ {} @@@@", relative_file_path.display())?;

            let mut file_contents = String::new();
            File::open(file_path)?.read_to_string(&mut file_contents)?;
            writeln!(writer, "{}", file_contents)?;
        }
    }

    writeln!(writer, "@@@@@@ END @@@@@@")?;
    Ok(())
}
