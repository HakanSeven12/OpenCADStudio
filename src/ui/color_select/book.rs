//! Colour books: AutoCAD `.acb` files.
//!
//! An `.acb` is XML: a `bookName` and `colorPage`s of `colorEntry`s, each a
//! `colorName` and an `RGB8` value. Books from some vendors encrypt their
//! values; those cannot be read.

/// One named colour.
#[derive(Debug, Clone, PartialEq)]
pub struct BookColor {
    pub name: String,
    pub rgb: (u8, u8, u8),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ColorBook {
    pub name: String,
    /// Every colour, page after page.
    pub colors: Vec<BookColor>,
    /// Where each page starts in `colors`.
    page_starts: Vec<usize>,
}

/// Pages for a book without `colorPage`s, as AutoCAD shows them.
const PAGE_SIZE: usize = 7;

impl ColorBook {
    pub fn page_count(&self) -> usize {
        self.page_starts.len()
    }

    /// The colours of one page, and the index of its first colour.
    pub fn page(&self, page: usize) -> (usize, &[BookColor]) {
        let start = self.page_starts.get(page).copied().unwrap_or(0);
        let end = self
            .page_starts
            .get(page + 1)
            .copied()
            .unwrap_or(self.colors.len());
        (start, &self.colors[start..end])
    }

    /// The page holding colour `index`.
    pub fn page_of(&self, index: usize) -> usize {
        self.page_starts
            .partition_point(|&start| start <= index)
            .saturating_sub(1)
    }

    /// The colour with this name, as stored after the `$` of `BOOK$NAME`.
    pub fn find(&self, name: &str) -> Option<usize> {
        self.colors.iter().position(|c| c.name == name)
    }

    /// The `BOOK$NAME` identity objects and layers store.
    pub fn identity(&self, index: usize) -> String {
        format!("{}${}", self.name, self.colors[index].name)
    }
}

/// Add `book`, replacing one of the same name; returns where it now is.
pub fn add_or_replace(books: &mut Vec<ColorBook>, book: ColorBook) -> usize {
    match books.iter().position(|b| b.name == book.name) {
        Some(index) => {
            books[index] = book;
            index
        }
        None => {
            books.push(book);
            books.len() - 1
        }
    }
}

/// The colour book that ships with the program; its licence is in the
/// file's header comment.
pub fn builtin() -> impl Iterator<Item = ColorBook> {
    parse(include_str!("../../../assets/color_books/ral-classic.acb")).into_iter()
}

/// Where colour books are kept: `color_books` in the config folder. The
/// web has no folders.
#[cfg(not(target_arch = "wasm32"))]
pub fn folder() -> Option<std::path::PathBuf> {
    crate::config::config_dir().map(|dir| dir.join("color_books"))
}

/// The built-in book plus every `.acb` file in the colour book folder, in
/// file-name order. A book replaces an earlier one of the same name.
pub fn read_all() -> Vec<ColorBook> {
    #[cfg_attr(target_arch = "wasm32", allow(unused_mut))]
    let mut books: Vec<_> = builtin().collect();
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(dir) = folder() {
        let mut files: Vec<_> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                path.extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("acb"))
            })
            .collect();
        files.sort();
        for book in files
            .into_iter()
            .filter_map(|path| std::fs::read_to_string(path).ok())
            .filter_map(|text| parse(&text).ok())
        {
            add_or_replace(&mut books, book);
        }
    }
    books
}

/// Ask for an `.acb` file and read it. On desktop the file is copied into
/// the colour book folder, so it stays when the original moves; the web
/// keeps the book for the session only.
pub async fn load_from_dialog() -> Result<ColorBook, String> {
    let file = crate::sys::file_dialog()
        .add_filter(crate::t!("Color book").as_ref(), &["acb", "ACB"])
        .pick_file()
        .await
        .ok_or_else(|| "Cancelled".to_string())?;
    let bytes = file.read().await;
    let book = parse(&String::from_utf8_lossy(&bytes))?;
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(folder) = folder() {
        std::fs::create_dir_all(&folder)
            .and_then(|()| std::fs::write(folder.join(file.file_name()), &bytes))
            .map_err(|e| e.to_string())?;
    }
    Ok(book)
}

/// How deeply a colour book's elements may nest; real books nest four
/// levels.
const MAX_XML_DEPTH: usize = 256;

/// Read an `.acb` file's text.
pub fn parse(text: &str) -> Result<ColorBook, String> {
    let text = text.trim_start_matches('\u{feff}');
    if !crate::xml_depth::within(text, MAX_XML_DEPTH) {
        return Err(format!(
            "The color book nests deeper than {MAX_XML_DEPTH} levels."
        ));
    }
    let doc = roxmltree::Document::parse(text).map_err(|e| e.to_string())?;
    let root = doc.root_element();
    if !root.has_tag_name("colorBook") {
        return Err(crate::t!("Not a color book").into_owned());
    }
    let mut book = ColorBook {
        name: text_of(root, "bookName").unwrap_or_default().to_string(),
        colors: Vec::new(),
        page_starts: Vec::new(),
    };
    let mut encrypted = false;
    let mut read_entries = |parent: roxmltree::Node<'_, '_>, book: &mut ColorBook| {
        for entry in parent.children().filter(|n| n.has_tag_name("colorEntry")) {
            let rgb = child(entry, "RGB8").and_then(|rgb| {
                let channel = |tag| text_of(rgb, tag)?.parse::<u8>().ok();
                Some((channel("red")?, channel("green")?, channel("blue")?))
            });
            match (text_of(entry, "colorName"), rgb) {
                (Some(name), Some(rgb)) => book.colors.push(BookColor {
                    name: name.to_string(),
                    rgb,
                }),
                _ => encrypted = true,
            }
        }
    };
    let mut pages = root
        .children()
        .filter(|n| n.has_tag_name("colorPage"))
        .peekable();
    if pages.peek().is_some() {
        for page in pages {
            let start = book.colors.len();
            read_entries(page, &mut book);
            if book.colors.len() > start {
                book.page_starts.push(start);
            }
        }
    } else {
        read_entries(root, &mut book);
        book.page_starts = (0..book.colors.len()).step_by(PAGE_SIZE).collect();
    }
    if book.colors.is_empty() {
        return Err(if encrypted {
            crate::t!("This color book is encrypted and cannot be read")
        } else {
            crate::t!("The color book has no colors")
        }
        .into_owned());
    }
    Ok(book)
}

fn child<'a, 'i>(node: roxmltree::Node<'a, 'i>, tag: &str) -> Option<roxmltree::Node<'a, 'i>> {
    node.children().find(|n| n.has_tag_name(tag))
}

fn text_of<'a>(node: roxmltree::Node<'a, '_>, tag: &str) -> Option<&'a str> {
    child(node, tag).and_then(|n| n.text()).map(str::trim)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, (r, g, b): (u8, u8, u8)) -> String {
        format!(
            "<colorEntry><colorName>{name}</colorName>\
             <RGB8><red>{r}</red><green>{g}</green><blue>{b}</blue></RGB8></colorEntry>"
        )
    }

    #[test]
    fn reads_pages_and_colours() {
        let text = format!(
            "\u{feff}<?xml version=\"1.0\"?><colorBook><bookName>RAL CLASSIC</bookName>\
             <colorPage><pageColor><RGB8><red>1</red><green>2</green><blue>3</blue></RGB8></pageColor>{}{}</colorPage>\
             <colorPage>{}</colorPage></colorBook>",
            entry("RAL 1000", (190, 189, 127)),
            entry("RAL 1001", (194, 176, 120)),
            entry("RAL 5015", (34, 113, 179)),
        );
        let book = parse(&text).unwrap();
        assert_eq!(book.name, "RAL CLASSIC");
        assert_eq!(book.page_count(), 2);
        assert_eq!(book.page(1), (2, &book.colors[2..]));
        assert_eq!(book.page_of(1), 0);
        assert_eq!(book.page_of(2), 1);
        let blue = book.find("RAL 5015").unwrap();
        assert_eq!(book.colors[blue].rgb, (34, 113, 179));
        assert_eq!(book.identity(blue), "RAL CLASSIC$RAL 5015");
    }

    #[test]
    fn a_book_without_pages_gets_pages_of_seven() {
        let entries: String = (0..9).map(|i| entry(&format!("C{i}"), (i, i, i))).collect();
        let book = parse(&format!(
            "<colorBook><bookName>B</bookName>{entries}</colorBook>"
        ))
        .unwrap();
        assert_eq!(book.page_count(), 2);
        assert_eq!(book.page(1).1.len(), 2);
    }

    #[test]
    fn rejects_unreadable_books() {
        assert!(parse("not xml").is_err());
        assert!(parse("<palette/>").is_err());
        let encrypted = "<colorBook><bookName>P</bookName><colorPage><colorEntry>\
                         <colorName>X</colorName><RGB8Encrypt>AbC</RGB8Encrypt></colorEntry>\
                         </colorPage></colorBook>";
        assert!(parse(encrypted).is_err());
    }
}
