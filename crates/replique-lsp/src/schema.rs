//! The `replique.toml` of a document: where it is, and what it holds.
//!
//! A dialogue is checked against the schema of its project, which is the
//! nearest `replique.toml` above it. The search stops at the workspace folder
//! the file is in, so a schema lying further up the disk does not leak into
//! a project that has none.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
    time::SystemTime,
};

use dashmap::DashMap;
use replique::parser::validation::RepliqueSchema;

pub const SCHEMA_FILE: &str = "replique.toml";

/// The nearest schema file above `file`, its own directory included.
///
/// When one of `roots` holds `file`, nothing above that root is looked at.
/// A file outside every root is searched all the way up, which is what a
/// client that opens a lone file expects.
pub fn find(file: &Path, roots: &[PathBuf]) -> Option<PathBuf> {
    // The innermost root, should two of them be nested.
    let root = roots
        .iter()
        .filter(|root| file.starts_with(root))
        .max_by_key(|root| root.components().count());

    for dir in file.ancestors().skip(1) {
        let candidate = dir.join(SCHEMA_FILE);
        if candidate.is_file() {
            return Some(candidate);
        }
        if root.is_some_and(|root| dir == root) {
            break;
        }
    }

    None
}

/// What the schema of a document turned out to be.
#[derive(Debug, Clone)]
pub enum Lookup {
    /// No `replique.toml` above the document: nothing to check it against.
    Missing,
    Found(Arc<RepliqueSchema>),
    /// A `replique.toml` that does not read. `fresh` is set the first time
    /// this content is seen, so that the error is told once and not on every
    /// keystroke.
    Invalid {
        path: PathBuf,
        error: String,
        fresh: bool,
    },
}

#[derive(Debug)]
struct Loaded {
    /// When the file was last written, as of the read. `None` when the
    /// platform does not say, in which case the file is read every time.
    modified: Option<SystemTime>,
    schema: Result<Arc<RepliqueSchema>, String>,
}

/// The schemas read so far, by the path of their file.
///
/// A schema is read again whenever its file has changed since, which is
/// checked each time it is asked for: a `replique.toml` edited on disk is
/// taken into account at the next change of a dialogue, without the client
/// having to watch it.
#[derive(Debug, Default)]
pub struct Schemas {
    roots: RwLock<Vec<PathBuf>>,
    loaded: DashMap<PathBuf, Loaded>,
}

impl Schemas {
    /// Sets the workspace folders, which bound the search of [`find`].
    pub fn set_roots(&self, roots: Vec<PathBuf>) {
        *self.roots.write().unwrap_or_else(|e| e.into_inner()) = roots;
    }

    /// The schema the document at `file` is checked against.
    pub fn for_file(&self, file: &Path) -> Lookup {
        let path = {
            let roots = self.roots.read().unwrap_or_else(|e| e.into_inner());
            find(file, &roots)
        };
        let Some(path) = path else {
            return Lookup::Missing;
        };

        let modified = fs::metadata(&path).and_then(|meta| meta.modified()).ok();

        let cached = self
            .loaded
            .get(&path)
            .filter(|loaded| modified.is_some() && loaded.modified == modified)
            .map(|loaded| loaded.schema.clone());

        let (schema, fresh) = match cached {
            Some(schema) => (schema, false),
            None => {
                let schema = fs::read_to_string(&path)
                    .map_err(|err| err.to_string())
                    .and_then(|src| RepliqueSchema::from_toml(&src).map_err(|err| err.to_string()))
                    .map(Arc::new);

                self.loaded.insert(
                    path.clone(),
                    Loaded {
                        modified,
                        schema: schema.clone(),
                    },
                );
                (schema, true)
            }
        };

        match schema {
            Ok(schema) => Lookup::Found(schema),
            Err(error) => Lookup::Invalid { path, error, fresh },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// A directory of its own under the temporary one, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);

            let dir = std::env::temp_dir().join(format!(
                "replique-lsp-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        /// Writes `content` at `path`, relative to the directory, creating
        /// the directories on the way.
        fn write(&self, path: &str, content: &str) -> PathBuf {
            let path = self.0.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, content).unwrap();
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_schema_next_to_a_file_is_found() {
        let dir = TempDir::new();
        let schema = dir.write("replique.toml", "");
        let file = dir.write("start.rep", "");

        assert_eq!(find(&file, std::slice::from_ref(&dir.0)), Some(schema));
    }

    #[test]
    fn the_schema_is_looked_for_up_the_directories() {
        let dir = TempDir::new();
        let schema = dir.write("replique.toml", "");
        let file = dir.write("assets/dialogue/act1/start.rep", "");

        assert_eq!(find(&file, std::slice::from_ref(&dir.0)), Some(schema));
    }

    #[test]
    fn the_nearest_schema_wins() {
        let dir = TempDir::new();
        dir.write("replique.toml", "");
        let nearest = dir.write("assets/replique.toml", "");
        let file = dir.write("assets/dialogue/start.rep", "");

        assert_eq!(find(&file, std::slice::from_ref(&dir.0)), Some(nearest));
    }

    #[test]
    fn a_schema_above_the_workspace_is_not_used() {
        let dir = TempDir::new();
        dir.write("replique.toml", "");
        let file = dir.write("project/dialogue/start.rep", "");
        let project = dir.0.join("project");

        assert_eq!(find(&file, &[project]), None);
    }

    /// A lone file, opened outside every workspace folder, still gets the
    /// schema above it.
    #[test]
    fn a_file_outside_every_workspace_is_searched_all_the_way_up() {
        let dir = TempDir::new();
        let schema = dir.write("replique.toml", "");
        let file = dir.write("dialogue/start.rep", "");
        let elsewhere = dir.0.join("another_project");

        assert_eq!(find(&file, &[elsewhere]), Some(schema.clone()));
        assert_eq!(find(&file, &[]), Some(schema));
    }

    #[test]
    fn a_project_without_a_schema_has_none() {
        let dir = TempDir::new();
        let file = dir.write("dialogue/start.rep", "");
        let schemas = Schemas::default();
        schemas.set_roots(vec![dir.0.clone()]);

        assert!(matches!(schemas.for_file(&file), Lookup::Missing));
    }

    #[test]
    fn a_schema_is_loaded_for_the_files_under_it() {
        let dir = TempDir::new();
        dir.write("replique.toml", "speakers = [\"Robin\"]\n");
        let file = dir.write("dialogue/start.rep", "");
        let schemas = Schemas::default();
        schemas.set_roots(vec![dir.0.clone()]);

        let Lookup::Found(schema) = schemas.for_file(&file) else {
            panic!("expected a schema");
        };
        assert_eq!(schema.speakers, ["Robin"]);
    }

    #[test]
    fn a_schema_is_read_once_while_its_file_does_not_change() {
        let dir = TempDir::new();
        dir.write("replique.toml", "speakers = [\"Robin\"]\n");
        let file = dir.write("start.rep", "");
        let schemas = Schemas::default();

        let (Lookup::Found(first), Lookup::Found(second)) =
            (schemas.for_file(&file), schemas.for_file(&file))
        else {
            panic!("expected a schema twice");
        };
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn a_schema_that_changed_on_disk_is_read_again() {
        let dir = TempDir::new();
        let path = dir.write("replique.toml", "speakers = [\"Robin\"]\n");
        let file = dir.write("start.rep", "");
        let schemas = Schemas::default();
        assert!(matches!(schemas.for_file(&file), Lookup::Found(_)));

        fs::write(&path, "speakers = [\"Fanny\"]\n").unwrap();
        // Some file systems keep time to the second: the date is moved by
        // hand rather than waited for.
        let later = SystemTime::now() + std::time::Duration::from_secs(5);
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(later)
            .unwrap();

        let Lookup::Found(schema) = schemas.for_file(&file) else {
            panic!("expected a schema");
        };
        assert_eq!(schema.speakers, ["Fanny"]);
    }

    #[test]
    fn a_schema_that_does_not_read_is_told_once() {
        let dir = TempDir::new();
        let path = dir.write("replique.toml", "speakers = \"Robin\"\n");
        let file = dir.write("start.rep", "");
        let schemas = Schemas::default();

        let Lookup::Invalid {
            path: reported,
            error,
            fresh,
        } = schemas.for_file(&file)
        else {
            panic!("expected an invalid schema");
        };
        assert_eq!(reported, path);
        assert!(error.contains("speakers"), "{error}");
        assert!(fresh);

        assert!(matches!(
            schemas.for_file(&file),
            Lookup::Invalid { fresh: false, .. }
        ));
    }
}
