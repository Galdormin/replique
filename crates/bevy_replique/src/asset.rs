use bevy::{
    asset::{AssetLoader, AssetPath, AsyncReadExt, LoadContext, io::Reader},
    prelude::*,
};
use thiserror::Error;

use replique::{
    RepliqueFile,
    dialogue::Dialogue,
    parser::diagnostic::{Color, Diagnostics},
};

#[derive(Asset, TypePath)]
pub struct RepliqueDialogue(Dialogue);

impl RepliqueDialogue {
    pub fn dialogue(&self) -> &Dialogue {
        &self.0
    }
}

#[derive(Error, Debug)]
pub enum RepliqueDialogueLoaderError {
    #[error("failed to read the replique file: {0}")]
    Io(#[from] std::io::Error),
    #[error(
        "failed to parse `{}`:\n{}", path.to_string(),
        diagnostics.render(src, Color::Never)
    )]
    Parse {
        /// Path of the asset, as shown in the `-->` header of the diagnostics.
        path: AssetPath<'static>,
        /// Source the spans of the diagnostics point into.
        src: String,
        /// Everything the parser reported, errors and warnings alike.
        diagnostics: Diagnostics,
    },
}

/// Loads a `.rep` file into a [`RepliqueDialogue`].
#[derive(Default, TypePath)]
pub struct RepliqueDialogueLoader;

impl AssetLoader for RepliqueDialogueLoader {
    type Asset = RepliqueDialogue;
    type Settings = ();
    type Error = RepliqueDialogueLoaderError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _: &(),
        load_context: &mut LoadContext<'_>,
    ) -> Result<RepliqueDialogue, RepliqueDialogueLoaderError> {
        let mut src = String::new();
        reader.read_to_string(&mut src).await?;

        let replique = RepliqueFile::from_source(src);
        let Some(dialogue) = replique.dialogue else {
            return Err(RepliqueDialogueLoaderError::Parse {
                path: load_context.path().clone(),
                src: replique.source,
                diagnostics: replique.diagnostics.with_path(load_context.path().path()),
            });
        };
        Ok(RepliqueDialogue(dialogue))
    }

    fn extensions(&self) -> &[&str] {
        &["replique", "rep"]
    }
}
