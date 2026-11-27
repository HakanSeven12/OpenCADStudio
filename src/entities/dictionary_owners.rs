//! Which dictionaries a document holds owners for, published while a batch
//! walk runs, so finding a block definition's extension dictionary does not
//! scan every object each time.

use document::objects::ObjectType;
use document::{Document, Handle};

type PublishedOwners = Vec<(usize, usize, std::sync::Arc<rustc_hash::FxHashSet<Handle>>)>;

/// Documents whose dictionary owners are published: (address, guards, owners).
static PUBLISHED: std::sync::RwLock<PublishedOwners> = std::sync::RwLock::new(Vec::new());

/// The owners of every dictionary in a document, published for as long as
/// this guard borrows it, so nothing can change them meanwhile.
///
/// Resolving a block's extension dictionary scans every object when the block
/// has none (most blocks), and every INSERT placement asks for its block's
/// base point: a drawing with half a million entities spent minutes there
/// (#2023). Batch walks publish the owners once; [`definition_dictionary`]
/// then answers "none" for an unowned block straight away, on any thread.
pub(crate) struct DictionaryOwners<'a> {
    address: usize,
    _document: std::marker::PhantomData<&'a Document>,
}

impl<'a> DictionaryOwners<'a> {
    pub(crate) fn publish(document: &'a Document) -> Self {
        let address = document as *const Document as usize;
        let mut published = PUBLISHED.write().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = published.iter_mut().find(|entry| entry.0 == address) {
            entry.1 += 1;
        } else {
            let owners = document
                .objects
                .values()
                .filter_map(|object| match object {
                    ObjectType::Dictionary(dictionary) => Some(dictionary.owner),
                    _ => None,
                })
                .collect();
            published.push((address, 1, std::sync::Arc::new(owners)));
        }
        Self {
            address,
            _document: std::marker::PhantomData,
        }
    }
}

impl Drop for DictionaryOwners<'_> {
    fn drop(&mut self) {
        let mut published = PUBLISHED.write().unwrap_or_else(|e| e.into_inner());
        if let Some(at) = published.iter().position(|entry| entry.0 == self.address) {
            published[at].1 -= 1;
            if published[at].1 == 0 {
                published.swap_remove(at);
            }
        }
    }
}

/// A block definition's extension dictionary. Skips the whole-document scan
/// [`Document::extension_dictionary_handle`] falls back to when the
/// document's dictionary owners are published and none is `definition`.
pub(crate) fn definition_dictionary(document: &Document, definition: Handle) -> Option<Handle> {
    let address = document as *const Document as usize;
    let unowned = PUBLISHED
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|entry| entry.0 == address)
        .is_some_and(|entry| !entry.2.contains(&definition));
    if unowned {
        return None;
    }
    document.extension_dictionary_handle(definition)
}
