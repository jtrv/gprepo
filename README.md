# gprepo <img src="https://www.pngall.com/wp-content/uploads/9/Star-Wars-C-3PO-Vector-Transparent.png" style="width:3.5rem;">

[`/dʒiːpiːˈɹi:pi:oʊ/`](http://ipa-reader.xyz/?text=%2Fd%CA%92i%CB%90pi%CB%90%CB%88%C9%B9i%3Api%3Ao%CA%8A%2F&voice=Joey)

a command-line tool that transforms a Git repository into a minimal format for ChatGPT queries.

## Features

- Excludes LICENSE and files in the .gitignore
- Can exclude specific files or directories
- Reduces whitespace for files with non-significant whitespace
- Optional preamble file for adding custom instructions
- Uses the R-word (R*st)

## Usage

```
gprepo [FLAGS] [OPTIONS]
```

### Flags

- `-h, --help`: Prints help information
- `-V, --version`: Prints version information

### Options

- `-i, --ignore <IGNORE_PATH>`: File paths to ignore (can be specified multiple times)
- `-o, --output <OUTPUT_PATH>`: Output file path (default: stdout)
- `-p, --preamble <PREAMBLE_PATH>`: Optional path to the preamble file
- `-r, --repo-path <REPO_PATH>`: Path to the repository

## Example

Translate a Git repository and output the result to a file:

```
gprepo -r /path/to/repo -o output.txt
```

Exclude specific files or directories:

```
gprepo -i "target/" -i "node_modules/"
```

Add a custom preamble file:

```
gprepo -p my_preamble.txt
```

## Installation
```
cargo install --git https://github.com/jtrv/gprepo
```

## License

This project is licensed under the MIT License.
