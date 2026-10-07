use std::fs;

use anyhow::{Context, Result};
use replique::parser::{diagnostic::Color, validation::RepliqueSchema};

use crate::{CheckArgs, Format, Outcome, discover::discover};

pub fn check(args: &CheckArgs) -> Result<Outcome> {
    let paths = discover(&args.paths, args.recursive)?;

    let maybe_schema = if let Some(path) = &args.schema {
        let data = fs::read_to_string(path)?;
        Some(RepliqueSchema::from_toml(&data)?)
    } else {
        None
    };

    let mut errors = 0;
    let mut warnings = 0;
    let mut reported = false;

    for path in &paths {
        let src = fs::read_to_string(path).with_context(|| format!("file: {}", path.display()))?;

        let mut parsed = replique::parser::parse(&src);

        if let Some(schema) = &maybe_schema {
            parsed.validate(&schema);
        }

        let diagnostics = parsed.diagnostics.with_path(path);
        errors += diagnostics.errors();
        warnings += diagnostics.warnings();

        if args.quiet {
            continue;
        }

        // Diagnostics go to stderr, like every other linter: stdout stays free
        // for whatever a future command wants to pipe.
        let rendered = match args.format {
            Format::Pretty => diagnostics.render(&src, Color::Auto),
            Format::Short => diagnostics.render_short(&src, Color::Auto),
        };
        if !rendered.is_empty() {
            eprint!("{rendered}");
            reported = true;
        }
    }

    if !args.quiet {
        // Blank line only when it separates the summary from the diagnostics.
        if reported {
            eprintln!();
        }
        eprintln!(
            "{errors} error(s), {warnings} warning(s) in {} file(s)",
            paths.len()
        );
    }

    if args.warnings_as_errors {
        errors += warnings;
    }

    if errors > 0 {
        Ok(Outcome::Errors)
    } else {
        Ok(Outcome::Clean)
    }
}
