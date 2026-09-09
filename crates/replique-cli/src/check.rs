use std::fs;

use anyhow::{Context, Result};
use replique::parser::diagnostic::Color;

use crate::{CheckArgs, Format, Outcome, discover::discover};

pub fn check(args: &CheckArgs) -> Result<Outcome> {
    let paths = discover(&args.paths, args.recursive)?;

    let mut errors = 0;
    let mut warnings = 0;
    let mut reported = false;

    for path in &paths {
        let src = fs::read_to_string(path).with_context(|| format!("file: {}", path.display()))?;

        let parsed = replique::parser::parse(&src);

        let diagnostics = parsed.diagnostics.with_color(Color::Auto).with_path(path);
        errors += diagnostics.errors();
        warnings += diagnostics.warnings();

        if args.quiet {
            continue;
        }

        // Diagnostics go to stderr, like every other linter: stdout stays free
        // for whatever a future command wants to pipe.
        let rendered = match args.format {
            Format::Pretty => diagnostics.render(&src),
            Format::Short => diagnostics.render_short(&src),
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
