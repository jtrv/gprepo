use pico_args::Arguments;
use std::ffi::OsStr;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;


const DEFAULT_PREAMBLE: &str =
    "Below is a repository. Files are marked with @@@<path>@@@ and the repo ends with @@@END@@@.";

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = Arguments::from_env();
    let help = args.contains(["-h", "--help"]);

    if help {
        print_help();
        return Ok(());
    }

    let output_path: Option<String> = args.opt_value_from_str(["-o", "--output"])?;
    let repo_path: Option<String> = args.opt_value_from_str(["-r", "--repo-path"])?;
    let preamble_path: Option<String> = args.opt_value_from_str(["-p", "--preamble"])?;
    let excludes: Vec<String> = args
        .values_from_fn(["-e", "--exclude"], |arg: &str| Ok::<String, &'static str>(arg.to_owned()))
        .unwrap_or_default();
    let includes: Vec<String> = args
        .values_from_fn(["-i", "--include"], |arg: &str| Ok::<String, &'static str>(arg.to_owned()))
        .unwrap_or_default();
    let compress = args.contains(["-c", "--compress"]);

    if !args.finish().is_empty() {
        return Err("Invalid arguments".into());
    }

    let repo_path = match repo_path {
        Some(p) => PathBuf::from(p),
        None => std::env::current_dir()?,
    };

    // Find git repo root
    let repo_root = find_git_root(&repo_path)?;
    let process_start_time = SystemTime::now();

    // Get list of git-tracked files (much faster than walking everything)
    let tracked_files = get_tracked_files(&repo_root)?;

    // Get ignored files from git
    let ignored_files = get_ignored_files(&repo_root)?;

    let exclude_patterns: Vec<Pattern> = excludes
        .iter()
        .filter_map(|p| Pattern::new(p))
        .collect();

    let include_patterns: Vec<Pattern> = includes
        .iter()
        .filter_map(|p| Pattern::new(p))
        .collect();

    // Write output
    let mut writer: Box<dyn Write> = match &output_path {
        Some(p) => Box::new(BufWriter::new(File::create(p)?)),
        None => Box::new(BufWriter::new(io::stdout())),
    };

    // Write preamble
    if let Some(preamble_file) = preamble_path {
        let mut preamble = String::new();
        File::open(preamble_file)?.read_to_string(&mut preamble)?;
        writer.write_all(preamble.as_bytes())?;
        if !preamble.ends_with('\n') {
            writer.write_all(b"\n")?;
        }
    } else {
        writer.write_all(DEFAULT_PREAMBLE.as_bytes())?;
        writer.write_all(b"\n")?;
    }

    // Collect and sort files
    let mut files: Vec<PathBuf> = tracked_files
        .into_iter()
        .filter(|f| !ignored_files.contains(f))
        .filter(|f| {
            let rel = match f.strip_prefix(&repo_root) {
                Ok(r) => r,
                Err(_) => return true,
            };
            let rel_str = rel.to_string_lossy();

            // Check exclude patterns
            if exclude_patterns.iter().any(|p| p.matches(&rel_str)) {
                return false;
            }

            // Check include patterns (if any specified)
            if !include_patterns.is_empty()
                && !include_patterns.iter().any(|p| p.matches(&rel_str))
            {
                return false;
            }

            // Skip output file
            if let Some(ref out) = output_path {
                if out == rel {
                    return false;
                }
            }

            true
        })
        .collect();

    files.sort();

    // Write files
    for file_path in files {
        let rel_path = match file_path.strip_prefix(&repo_root) {
            Ok(r) => r,
            Err(_) => continue,
        };

        // Skip directories (already filtered by tracked files)
        if !file_path.is_file() {
            continue;
        }

        // Skip files modified during processing
        if let Ok(meta) = file_path.metadata() {
            if let Ok(modified) = meta.modified() {
                if modified >= process_start_time {
                    continue;
                }
            }
        }

        // Skip binary files
        if is_binary(&file_path)? {
            continue;
        }

        // Read and optionally compress content
        let mut content = String::new();
        File::open(&file_path)?.read_to_string(&mut content)?;

        let processed = if compress {
            compress_content(&content, rel_path)
        } else {
            content
        };

        writeln!(writer, "@@@{}@@@", rel_path.display())?;
        writer.write_all(processed.as_bytes())?;
        if !processed.ends_with('\n') {
            writer.write_all(b"\n")?;
        }
    }

    writer.write_all(b"@@@END@@@\n")?;
    Ok(())
}

fn print_help() {
    println!(
        r#"gprepo - Transform a Git repository into a minimal format for LLM queries

USAGE:
    gprepo [OPTIONS]

OPTIONS:
    -h, --help           Print help
    -r, --repo-path PATH Repository path (default: current directory)
    -o, --output PATH    Output file (default: stdout)
    -p, --preamble PATH  Preamble file
    -e, --exclude GLOB   Exclude paths matching glob (can repeat)
    -i, --include GLOB   Include only paths matching glob (can repeat)
    -c, --compress       Aggressive whitespace compression

EXAMPLES:
    gprepo -r . -o repo.txt
    gprepo -e "target/" -e "*.lock" -c
"#
    );
}

/// Find the root of a git repository
fn find_git_root(path: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(path)
        .output()?;

    if !output.status.success() {
        return Err("Not a git repository".into());
    }

    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(PathBuf::from(root))
}

/// Get list of all git-tracked files
fn get_tracked_files(repo_root: &Path) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .args(["ls-files", "--full-name"])
        .current_dir(repo_root)
        .output()?;

    if !output.status.success() {
        return Err("git ls-files failed".into());
    }

    let files: Vec<PathBuf> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|l| repo_root.join(l))
        .collect();

    Ok(files)
}

/// Get set of git-ignored files
fn get_ignored_files(repo_root: &Path) -> Result<std::collections::HashSet<PathBuf>, Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .args(["ls-files", "--ignored", "--exclude-standard", "--others"])
        .current_dir(repo_root)
        .output()?;

    // git ls-files with --others returns exit 0 even with ignored files
    let files: std::collections::HashSet<PathBuf> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|l| repo_root.join(l))
        .collect();

    Ok(files)
}


/// Check if a file is binary
fn is_binary(file_path: &Path) -> Result<bool, Box<dyn std::error::Error>> {
    let mut buffer = [0u8; 8192];
    let mut reader = BufReader::new(File::open(file_path)?);
    let read = reader.read(&mut buffer)?;
    Ok(buffer[..read].contains(&0))
}

/// Simple glob pattern matching (supports: *, ?, [...])
#[derive(Clone)]
struct Pattern {
    pattern: String,
}

impl Pattern {
    fn new(s: &str) -> Option<Self> {
        // Only create if contains glob characters
        if s.contains('*') || s.contains('?') || s.contains('[') {
            Some(Self {
                pattern: s.to_string(),
            })
        } else {
            None
        }
    }

    fn matches(&self, text: &str) -> bool {
        fnmatch(&self.pattern, text)
    }
}

/// Simple fnmatch-style pattern matching
fn fnmatch(pattern: &str, text: &str) -> bool {
    let p = pattern.as_bytes();
    let mut t = text.as_bytes();
    let mut i = 0;

    while i < p.len() {
        match p[i] {
            b'*' => {
                i += 1;
                if i >= p.len() {
                    return true; // * at end matches everything
                }
                // Try matching rest of pattern at each position in text
                while !t.is_empty() {
                    if fnmatch_bytes(&p[i..], t) {
                        return true;
                    }
                    t = &t[1..];
                }
                return fnmatch_bytes(&p[i..], t);
            }
            b'?' => {
                if t.is_empty() {
                    return false;
                }
                t = &t[1..];
                i += 1;
            }
            b'[' => {
                // Find matching ]
                let mut j = i + 1;
                let negated = j < p.len() && p[j] == b'!';
                if negated {
                    j += 1;
                }
                while j < p.len() && p[j] != b']' {
                    j += 1;
                }
                if j >= p.len() {
                    // Literal [
                    if t.is_empty() || t[0] != b'[' {
                        return false;
                    }
                    t = &t[1..];
                    i = j;
                } else {
                    let negate = negated;
                    let mut matched = false;
                    let mut k = i + if negated { 2 } else { 1 };
                    while k < j {
                        if k + 2 <= j && p[k + 1] == b'-' {
                            // Range
                            if !t.is_empty()
                                && t[0] >= p[k]
                                && t[0] <= p[k + 2]
                            {
                                matched = true;
                            }
                            k += 3;
                        } else if !t.is_empty() && t[0] == p[k] {
                            matched = true;
                            k += 1;
                        } else {
                            k += 1;
                        }
                    }
                    if matched == negate {
                        return false;
                    }
                    t = &t[1..];
                    i = j + 1;
                }
            }
            b'\\' if i + 1 < p.len() => {
                i += 1;
                if t.is_empty() || t[0] != p[i] {
                    return false;
                }
                t = &t[1..];
                i += 1;
            }
            c => {
                if t.is_empty() || t[0] != c {
                    return false;
                }
                t = &t[1..];
                i += 1;
            }
        }
    }

    t.is_empty()
}

fn fnmatch_bytes(pattern: &[u8], text: &[u8]) -> bool {
    let pattern = match std::str::from_utf8(pattern) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let text = match std::str::from_utf8(text) {
        Ok(s) => s,
        Err(_) => return false,
    };
    fnmatch(pattern, text)
}

/// Extensions that preserve significant whitespace
const WHITESPACE_PRESERVED: &[&str] = &[
    "py", "pyw", "yaml", "yml", "toml", "ini", "cfg", "conf", "nim", "hs",
    "coffee", "jade", "pug", "slim", "sass", "haml", "less", " styl",
];

/// Extensions where indentation is insignificant
const NO_INDENTATION: &[&str] = &[
    "rs", "js", "jsx", "ts", "tsx", "c", "cpp", "h", "hpp", "java", "go",
    "cs", "rb", "php", "swift", "kt", "kts", "scala", "groovy", "fs", "fsx",
    "clj", "cljs", "edn", "lisp", "el", "scm", "ss", "rkt", "jl", "lua",
    "tcl", "pl", "pm", "elm", "erl", "hrl", "v", "sv", "svh", "html", "htm",
    "css", "scss", "json", "xml", "sql", "md", "sh", "bash", "zsh", "ps1",
    "awk", "sed", "dockerfile", "makefile", "cmakelists",
];

/// Compress content based on file type
fn compress_content(content: &str, path: &Path) -> String {
    let ext = path
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or("")
        .to_lowercase();

    let preserve_ws = WHITESPACE_PRESERVED.iter().any(|e| ext == *e);
    let no_indent = NO_INDENTATION.iter().any(|e| ext == *e);

    if preserve_ws {
        // Normalize tabs to spaces, remove empty lines
        content
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.replace('\t', "    "))
            .collect::<Vec<_>>()
            .join(" ")
    } else if no_indent {
        // Remove leading whitespace, collapse consecutive spaces, remove empty lines
        let mut result = String::with_capacity(content.len());
        let mut prev_space = false;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            for c in trimmed.chars() {
                if c.is_ascii_whitespace() {
                    if !prev_space && !result.is_empty() {
                        result.push(' ');
                    }
                    prev_space = true;
                } else {
                    result.push(c);
                    prev_space = false;
                }
            }
            prev_space = false;
        }
        result
    } else {
        // Collapse all whitespace, remove empty lines
        let mut result = String::with_capacity(content.len());
        let mut prev_space = false;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            for c in trimmed.chars() {
                if c.is_ascii_whitespace() {
                    if !prev_space && !result.is_empty() {
                        result.push(' ');
                    }
                    prev_space = true;
                } else {
                    result.push(c);
                    prev_space = false;
                }
            }
            prev_space = false;
        }
        result
    }
}
