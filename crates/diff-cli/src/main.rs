use diff_ex_core::{
    batch::{self, BatchManifest, BatchTask},
    structured::StructuredFormat,
};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::AtomicBool;

const HELP:&str="Diff EX CLI\n\nUsage:\n  diff-ex-cli compare <text|structured|binary|folder|archive|document|git> <left> <right> [options]\n  diff-ex-cli batch <manifest.json> [options]\n\nOptions:\n  --format json|html      Report format (default json)\n  --output PATH           Create a new report; existing files are never replaced\n  --structured-format json|yaml|xml\n  --encoding UTF-8|UTF-16LE|UTF-16BE|GBK|GB18030\n  --left-revision REV     Git left revision (default HEAD)\n  --right-revision REV    Git right revision (default WORKTREE)\n  --help, --version\n\nExit codes: 0 equal, 1 differences, 2 error or incomplete.\nReports may contain source paths and comparison data. No network or write-back operations.\n";
fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("diff-ex-cli: {e}");
            std::process::exit(2)
        }
    }
}
fn run() -> Result<i32, String> {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        print!("{HELP}");
        return Ok(0);
    };
    if command == "--help" || command == "-h" {
        print!("{HELP}");
        return Ok(0);
    }
    if command == "--version" {
        println!("diff-ex-cli {}", env!("CARGO_PKG_VERSION"));
        return Ok(0);
    }
    let mut manifest = match command.as_str() {
        "compare" => {
            let kind = args.next().ok_or("Missing comparison kind")?;
            let left = args.next().ok_or("Missing left source")?;
            let right = args.next().ok_or("Missing right source")?;
            BatchManifest {
                version: 1,
                tasks: vec![BatchTask {
                    id: "compare".into(),
                    kind,
                    left,
                    right,
                    format: None,
                    text_options: None,
                    structured_options: None,
                    directory_options: None,
                    encoding: None,
                    left_revision: None,
                    right_revision: None,
                }],
            }
        }
        "batch" => {
            let path = args.next().ok_or("Missing manifest path")?;
            serde_json::from_slice(&std::fs::read(path).map_err(error)?).map_err(error)?
        }
        _ => return Err(format!("Unknown command: {command}")),
    };
    let mut format = "json".to_owned();
    let mut output = None;
    while let Some(arg) = args.next() {
        let val = args
            .next()
            .ok_or_else(|| format!("Missing value for {arg}"))?;
        match arg.as_str() {
            "--format" if val == "json" || val == "html" => format = val,
            "--output" => output = Some(val),
            "--structured-format" if command == "compare" => {
                manifest.tasks[0].format = Some(match val.as_str() {
                    "json" => StructuredFormat::Json,
                    "yaml" => StructuredFormat::Yaml,
                    "xml" => StructuredFormat::Xml,
                    _ => return Err("Unknown structured format".into()),
                })
            }
            "--encoding" if command == "compare" => manifest.tasks[0].encoding = Some(val),
            "--left-revision" if command == "compare" => {
                manifest.tasks[0].left_revision = Some(val)
            }
            "--right-revision" if command == "compare" => {
                manifest.tasks[0].right_revision = Some(val)
            }
            _ => return Err(format!("Unknown or invalid option: {arg}")),
        }
    }
    let report = batch::run_manifest(&manifest, &AtomicBool::new(false))?;
    let code = batch::exit_code(&report);
    let body = if format == "html" {
        batch::html(&report)?
    } else {
        serde_json::to_string_pretty(&report).map_err(error)?
    };
    if let Some(output) = output {
        let path = Path::new(&output);
        let parent = path
            .parent()
            .filter(|v| !v.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut file = tempfile::NamedTempFile::new_in(parent).map_err(error)?;
        file.write_all(body.as_bytes()).map_err(error)?;
        file.as_file().sync_all().map_err(error)?;
        file.persist_noclobber(path)
            .map_err(|e| format!("Cannot create report without overwriting: {}", e.error))?;
    } else {
        let mut stdout = io::stdout().lock();
        writeln!(stdout, "{body}").map_err(error)?;
    }
    Ok(code)
}
