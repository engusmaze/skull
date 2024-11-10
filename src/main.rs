use std::fs;

use anyhow::Result;
use clap::Parser;
use skull_editor::SkullEditor;

// Command line arguments structure
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg()]
    file_path: String,
}

fn main() -> Result<()> {
    let args = Args::parse();
    // Read file contents or use empty string if file doesn't exist
    let input = fs::read_to_string(&args.file_path).unwrap_or_default();
    let result = SkullEditor::new(input).run()?;
    if result.save {
        fs::write(&args.file_path, result.content)?;
    }
    Ok(())
}
