//! The layer files that LAYERSTATE IMPORT and LAYTRANS read (a `.las`
//! layer state, a mapping list, a standards drawing), read off the UI
//! thread. Each command reads its words at once (a bad word is an error
//! before any read), a worker reads the files, and the command goes on
//! when the read ends ([`OpenCADStudio::finish_layer_file_read`]): in the
//! drawing it was started for, or in the Layer Translator window while it
//! is open.

use super::*;
use crate::modules::draw::layers::laytrans::{self, Mapping, TargetLayer};
use std::path::PathBuf;

/// What goes on when the files are read.
#[derive(Clone, Debug)]
pub enum LayerFileJob {
    /// LAYERSTATE IMPORT: store the state.
    LayerStateImport,
    /// LAYTRANS <standards>: translate the layers named alike.
    TranslateSame,
    /// -LAYTRANS <standards> [FROM=TO …]: translate these pairs, or the
    /// layers named alike when there are none.
    TranslatePairs(Vec<Mapping>),
    /// -LAYTRANS LOAD <file> [<standards>]: translate the list.
    TranslateList,
    /// -LAYTRANS LIST <standards>: show the target layers.
    ListTargets,
    /// The Layer Translator window's standards file.
    WindowStandards,
    /// The Layer Translator window's mapping list.
    WindowMappings,
}

/// The files to read.
#[derive(Clone, Debug, Default)]
pub struct LayerFiles {
    /// A `.las` layer state.
    pub layer_state: Option<PathBuf>,
    /// A mapping list.
    pub mappings: Option<PathBuf>,
    /// A standards drawing.
    pub standards: Option<PathBuf>,
}

/// What the reads gave; each is `None` when that file was not asked for.
#[derive(Clone, Debug, Default)]
pub struct LayerFilesRead {
    pub layer_state: Option<Result<document::layer_state::LayerState, String>>,
    pub mappings: Option<Result<Vec<Mapping>, String>>,
    pub targets: Option<Result<Vec<TargetLayer>, String>>,
}

/// The end of a read: the drawing it was started for, the job, the files
/// and what they gave.
#[derive(Clone, Debug)]
pub struct LayerFileRead {
    pub tab_id: u64,
    pub generation: u64,
    pub job: LayerFileJob,
    pub files: LayerFiles,
    pub read: LayerFilesRead,
}

/// The readers that the commands used on the UI thread before.
pub(in crate::app) fn read_layer_files(files: &LayerFiles) -> LayerFilesRead {
    LayerFilesRead {
        layer_state: files.layer_state.as_ref().map(|path| {
            std::fs::read_to_string(path)
                .map_err(|error| error.to_string())
                .and_then(|text| super::layers::layer_state_from_las(&text))
        }),
        mappings: files.mappings.as_deref().map(laytrans::load_mappings),
        targets: files.standards.as_deref().map(laytrans::load_targets),
    }
}

fn shown(path: &Option<PathBuf>) -> String {
    path.as_deref()
        .map(|path| path.display().to_string())
        .unwrap_or_default()
}

impl OpenCADStudio {
    /// Reads `files` off the UI thread for drawing `i`; `job` goes on when
    /// the read ends.
    pub(in crate::app) fn read_layer_files(
        &mut self,
        i: usize,
        job: LayerFileJob,
        files: LayerFiles,
    ) -> Task<Message> {
        self.read_layer_files_with(i, job, files, read_layer_files)
    }

    /// [`Self::read_layer_files`] with the reader given, so a test can give
    /// one that panics.
    pub(in crate::app) fn read_layer_files_with(
        &mut self,
        i: usize,
        job: LayerFileJob,
        files: LayerFiles,
        reader: fn(&LayerFiles) -> LayerFilesRead,
    ) -> Task<Message> {
        let context = match job {
            LayerFileJob::LayerStateImport => "LAYERSTATE",
            _ => "LAYTRANS",
        };
        for path in [&files.layer_state, &files.mappings, &files.standards]
            .into_iter()
            .flatten()
        {
            self.command_line
                .push_info(&format!("Reading {}…", path.display()));
        }
        let (tab_id, generation) = (self.tabs[i].id, self.tabs[i].document_generation);
        crate::app::update::background_task(
            context,
            move || {
                let read = reader(&files);
                (files, read)
            },
            move |(files, read)| {
                Message::LayerFileRead(Box::new(LayerFileRead {
                    tab_id,
                    generation,
                    job,
                    files,
                    read,
                }))
            },
        )
    }

    /// Goes on with a command when its files are read. A drawing command's
    /// result for a drawing that was closed, replaced or cleared is
    /// dropped (the command line says why); a window's result is dropped
    /// when the window is closed.
    pub(in crate::app) fn finish_layer_file_read(&mut self, done: LayerFileRead) -> Task<Message> {
        let LayerFileRead {
            tab_id,
            generation,
            job,
            files,
            read,
        } = done;
        match job {
            LayerFileJob::WindowStandards => {
                if self.dialog_open(crate::app::ModalKind::LayerTranslator) {
                    if let (Some(path), Some(targets)) = (files.standards, read.targets) {
                        self.take_layer_translator_targets(&path, targets);
                    }
                }
                return Task::none();
            }
            LayerFileJob::WindowMappings => {
                if self.dialog_open(crate::app::ModalKind::LayerTranslator) {
                    if let Some(mappings) = read.mappings {
                        self.take_layer_translator_mappings(mappings);
                    }
                }
                return Task::none();
            }
            LayerFileJob::ListTargets => {
                if let Some(targets) = read.targets {
                    self.list_layer_translation_targets(&shown(&files.standards), targets);
                }
                return Task::none();
            }
            _ => {}
        }
        let Some(i) = self.import_target(tab_id, generation) else {
            return Task::none();
        };
        match job {
            LayerFileJob::LayerStateImport => {
                if let Some(state) = read.layer_state {
                    self.import_layer_state(i, &shown(&files.layer_state), state);
                }
                Task::none()
            }
            LayerFileJob::TranslateSame => match read.targets {
                Some(targets) => self.layer_translate_same_to(i, targets),
                None => Task::none(),
            },
            LayerFileJob::TranslatePairs(pairs) => match read.targets {
                Some(targets) => self.layer_translate_pairs(i, targets, pairs),
                None => Task::none(),
            },
            LayerFileJob::TranslateList => match read.mappings {
                Some(mappings) => {
                    self.layer_translate_list(i, &shown(&files.mappings), mappings, read.targets)
                }
                None => Task::none(),
            },
            LayerFileJob::ListTargets
            | LayerFileJob::WindowStandards
            | LayerFileJob::WindowMappings => Task::none(),
        }
    }
}

impl OpenCADStudio {
    /// The Layer Translator window's standards file when it is read.
    pub(in crate::app) fn take_layer_translator_targets(
        &mut self,
        path: &std::path::Path,
        targets: Result<Vec<TargetLayer>, String>,
    ) {
        match targets {
            Ok(targets) => {
                let state = self.layer_translator.get_or_insert_with(Default::default);
                state.source_file = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                state.targets = targets;
                // A target set that no longer contains a mapped name
                // would translate onto nothing.
                let names: Vec<String> = state.targets.iter().map(|t| t.name.clone()).collect();
                state
                    .mappings
                    .retain(|m| names.iter().any(|n| n.eq_ignore_ascii_case(&m.to)));
                state.selected_to = None;
            }
            Err(why) => self
                .command_line
                .push_error(crate::tf!("LAYTRANS: {why}.").as_ref()),
        }
    }
}
