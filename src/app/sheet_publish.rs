//! PUBLISH from a sheet list (`.dsd`): the sheets of many drawings, in the
//! list's order, into one PDF with a bookmark per sheet, or into a PDF per
//! sheet.
//!
//! The drawings open one at a time through the OPEN a script uses — rows
//! queued at the front of the script queue — so each sheet plots exactly as
//! it does from its own drawing. A drawing that is already open is plotted
//! as it is. One opened for the job takes no edit lease and is closed
//! unsaved once its sheets are plotted. The PDF is written as the drawings
//! go, so memory holds one drawing at a time.

use crate::app::{Message, OpenCADStudio};
use crate::io::dsd::{Sheet, SheetList, Target};
use iced::Task;
use std::path::{Path, PathBuf};

/// The script row that plots the drawing just opened and opens the next.
pub(crate) const NEXT_STEP: &str = "PUBLISHNEXT";

/// A sheet list being published.
pub(in crate::app) struct SheetPublish {
    list: SheetList,
    /// The PDF's title: the sheet list's name.
    title: String,
    /// The next sheet to publish.
    next: usize,
    /// The multi-sheet PDF being written.
    pdf: Option<crate::io::pdf_publish::PublishPdf>,
    /// The drawing opened for sheets `start..end`, plotted at the next step.
    opening: Option<(PathBuf, usize, usize)>,
    /// The tab that was active when the job began, active again at its end.
    return_tab: u64,
    /// The destination name of each sheet's page, by its drawing and layout
    /// (lower case), and of a drawing's first sheet: what links open.
    destinations: std::collections::HashMap<(PathBuf, String), String>,
    first_destinations: std::collections::HashMap<PathBuf, String>,
    published: usize,
    failed: usize,
    started: std::time::Instant,
}

/// The destination name of the page of sheet `index` (0-based); in order,
/// as a name tree lists them.
fn sheet_destination(index: usize) -> String {
    format!("OcsSheet{:06}", index + 1)
}

/// `path` as the file system resolves it, so that two spellings of a drawing
/// compare equal; as it is when it does not exist.
fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

impl SheetPublish {
    /// The page `link` opens in the PDF: the sheet of its drawing (this
    /// sheet's `drawing` for a link without an address) and named location,
    /// or the drawing's first sheet when the link names no layout of it.
    fn link_destination(&self, link: &crate::io::plot::PlotLink, drawing: &Path) -> Option<String> {
        let target = canonical(&link.drawing(Some(drawing))?);
        for layout in link.layout_candidates() {
            if let Some(name) = self
                .destinations
                .get(&(target.clone(), layout.to_lowercase()))
            {
                return Some(name.clone());
            }
        }
        self.first_destinations.get(&target).cloned()
    }
}

impl OpenCADStudio {
    /// `PUBLISH <list.dsd>`: publish the sheet list at `path`.
    pub(in crate::app) fn start_sheet_publish(&mut self, path: &Path) -> Task<Message> {
        // A job a cancelled script left behind ends here, and takes its
        // unfinished file with it.
        self.sheet_publish = None;
        let list = match crate::io::dsd::read(path) {
            Ok(list) => list,
            Err(error) => {
                self.command_line
                    .push_error(crate::tf!("PUBLISH: {error}").as_ref());
                return Task::none();
            }
        };
        let pdf = match &list.target {
            Target::MultiSheetPdf(out) => match crate::io::pdf_publish::PublishPdf::create(out) {
                Ok(pdf) => Some(pdf),
                Err(error) => {
                    self.command_line
                        .push_error(crate::tf!("PUBLISH: {error}").as_ref());
                    return Task::none();
                }
            },
            Target::SingleSheetPdfs(folder) => {
                if !folder.is_dir() {
                    self.command_line.push_error(
                        crate::tf!("PUBLISH: the folder {} does not exist.", folder.display())
                            .as_ref(),
                    );
                    return Task::none();
                }
                None
            }
        };
        let target = match &list.target {
            Target::MultiSheetPdf(out) => out.display().to_string(),
            Target::SingleSheetPdfs(folder) => folder.display().to_string(),
        };
        self.command_line.push_output(
            crate::tf!(
                "Publishing {} sheet(s) from {} to {target}",
                list.sheets.len(),
                path.display()
            )
            .as_ref(),
        );
        let title = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut destinations = std::collections::HashMap::new();
        let mut first_destinations = std::collections::HashMap::new();
        for (index, sheet) in list.sheets.iter().enumerate() {
            let name = sheet_destination(index);
            let drawing = canonical(&sheet.drawing);
            destinations
                .entry((drawing.clone(), sheet.layout.to_lowercase()))
                .or_insert_with(|| name.clone());
            first_destinations.entry(drawing).or_insert(name);
        }
        self.sheet_publish = Some(Box::new(SheetPublish {
            list,
            title,
            next: 0,
            pdf,
            opening: None,
            return_tab: self.tabs[self.active_tab].id,
            destinations,
            first_destinations,
            published: 0,
            failed: 0,
            started: std::time::Instant::now(),
        }));
        self.script_queue.push_front(NEXT_STEP.to_string());
        if self.script_running {
            Task::none()
        } else {
            Task::done(Message::ScriptPump)
        }
    }

    /// Whether `path` is the drawing a sheet list is opening only to plot it.
    pub(in crate::app) fn opened_for_publish(&self, path: &Path) -> bool {
        let Some((opening, _, _)) = self
            .sheet_publish
            .as_ref()
            .and_then(|job| job.opening.as_ref())
        else {
            return false;
        };
        opening == path
            || std::fs::canonicalize(opening)
                .ok()
                .zip(std::fs::canonicalize(path).ok())
                .is_some_and(|(a, b)| a == b)
    }

    /// The step between drawings: plot the drawing opened for the job and
    /// close it, then plot what is open already, open the next drawing, or
    /// finish the PDF.
    pub(in crate::app) fn sheet_publish_step(&mut self) -> Task<Message> {
        let Some(mut job) = self.sheet_publish.take() else {
            return Task::none();
        };
        let mut tasks = Vec::new();
        if let Some((path, start, end)) = job.opening.take() {
            match self.tab_showing(&path) {
                Some(idx) => {
                    self.active_tab = idx;
                    self.publish_drawing_sheets(&mut job, start, end);
                    let id = self.tabs[idx].id;
                    // Plotting stamps the plot date into fields; nothing of
                    // it is saved.
                    self.tabs[idx].dirty = false;
                    tasks.push(self.update(Message::TabClose(id)));
                }
                None => self.fail_sheets(
                    &mut job,
                    start..end,
                    &crate::t!("the drawing could not be opened."),
                ),
            }
        }
        let count = job.list.sheets.len();
        while job.next < count {
            let start = job.next;
            let drawing = job.list.sheets[start].drawing.clone();
            let end = (start..count)
                .find(|&k| job.list.sheets[k].drawing != drawing)
                .unwrap_or(count);
            job.next = end;
            if !drawing.is_file() {
                let reason = crate::tf!("drawing {} not found.", drawing.display());
                self.fail_sheets(&mut job, start..end, &reason);
                continue;
            }
            if let Some(idx) = self.tab_showing(&drawing) {
                let active = self.active_tab;
                self.active_tab = idx;
                self.publish_drawing_sheets(&mut job, start, end);
                self.active_tab = active;
                continue;
            }
            job.opening = Some((drawing.clone(), start, end));
            self.script_queue.push_front(NEXT_STEP.to_string());
            self.script_queue
                .push_front(format!("OPEN \"{}\"", drawing.display()));
            self.sheet_publish = Some(job);
            return Task::batch(tasks);
        }
        let return_tab = job.return_tab;
        self.finish_sheet_publish(job);
        if let Some(idx) = self.tabs.iter().position(|tab| tab.id == return_tab) {
            if idx != self.active_tab {
                tasks.push(self.update(Message::TabSwitch(idx)));
            }
        }
        Task::batch(tasks)
    }

    /// Plot sheets `start..end` of the job from the active drawing and add
    /// them to the output.
    fn publish_drawing_sheets(&mut self, job: &mut SheetPublish, start: usize, end: usize) {
        self.stamp_plot_fields();
        // Print All's "same settings for every layout" is not the sheet
        // list's: each sheet plots with its own page setup.
        let settings_override = std::mem::replace(&mut self.print_all_settings_override, false);
        let count = job.list.sheets.len();
        let mut pages = Vec::new();
        let mut titles = Vec::new();
        let mut names = Vec::new();
        let one_file = matches!(job.list.target, Target::MultiSheetPdf(_));
        for k in start..end {
            let sheet = &job.list.sheets[k];
            match self.sheet_page(sheet) {
                Ok(mut page) => {
                    // In one PDF, a link to a sheet of the list opens its page.
                    let content = &mut page.content;
                    content.destination = None;
                    for link in &mut content.links {
                        link.destination = if one_file {
                            job.link_destination(link, &sheet.drawing)
                        } else {
                            None
                        };
                    }
                    if one_file {
                        content.destination = Some(sheet_destination(k));
                    }
                    names.push(content.destination.clone());
                    self.command_line.push_output(
                        crate::tf!(
                            "  [{}/{count}] {}: {:.1}×{:.1} mm",
                            k + 1,
                            sheet.name,
                            page.paper_w,
                            page.paper_h
                        )
                        .as_ref(),
                    );
                    pages.push(page);
                    titles.push(sheet.name.clone());
                }
                Err(error) => {
                    job.failed += 1;
                    self.command_line.push_error(
                        crate::tf!("  [{}/{count}] {}: {error}", k + 1, sheet.name).as_ref(),
                    );
                }
            }
        }
        self.print_all_settings_override = settings_override;
        if pages.is_empty() {
            return;
        }
        let style = self.dialog_plot_style(&self.plot_dialog);
        match &job.list.target {
            Target::MultiSheetPdf(_) => {
                let Some(pdf) = job.pdf.as_mut() else {
                    return;
                };
                match crate::io::pdf_export::render_pdf_document(
                    &pages,
                    style.as_ref(),
                    crate::io::pdf_export::FontEmbedding::Whole,
                )
                    .and_then(|doc| pdf.append(doc, &titles, &names))
                {
                    Ok(appended) => job.published += appended,
                    Err(error) => {
                        job.failed += pages.len();
                        self.command_line
                            .push_error(crate::tf!("PUBLISH: {error}").as_ref());
                    }
                }
            }
            Target::SingleSheetPdfs(folder) => {
                for (page, title) in pages.iter().zip(&titles) {
                    let path = folder.join(crate::io::dsd::sheet_file_name(title));
                    match crate::io::pdf_export::export_plot_pdf_pages(
                        std::slice::from_ref(page),
                        &path,
                        style.as_ref(),
                    ) {
                        Ok(()) => job.published += 1,
                        Err(error) => {
                            job.failed += 1;
                            self.command_line.push_error(
                                crate::tf!("PUBLISH: {}: {error}", path.display()).as_ref(),
                            );
                        }
                    }
                }
            }
        }
    }

    /// The page of one sheet of the active drawing.
    fn sheet_page(&mut self, sheet: &Sheet) -> Result<crate::io::pdf_export::PdfPageInput, String> {
        let scene = &self.tabs[self.active_tab].scene;
        let layout = scene
            .layout_names()
            .into_iter()
            .find(|name| name.eq_ignore_ascii_case(&sheet.layout))
            .ok_or_else(|| {
                crate::tf!("the drawing has no layout named '{}'.", sheet.layout).into_owned()
            })?;
        let setup = match sheet.setup.as_deref() {
            None => None,
            Some(setup) => {
                // A page setup imported from another drawing is written
                // `name|drawing`.
                let (name, source) = match setup.split_once('|') {
                    Some((name, source)) => (name.trim(), Some(source.trim())),
                    None => (setup, None),
                };
                if let Some(source) = source.filter(|source| !source.is_empty()) {
                    let same = std::fs::canonicalize(source).ok()
                        == std::fs::canonicalize(&sheet.drawing).ok();
                    if !same {
                        return Err(crate::tf!(
                            "page setup '{name}' comes from another drawing ({source}); \
                             only the drawing's own page setups are supported."
                        )
                        .into_owned());
                    }
                }
                Some(scene.page_setup_get(name).ok_or_else(|| {
                    crate::tf!("the drawing has no page setup named '{name}'.").into_owned()
                })?)
            }
        };
        self.plot_layout_pages(&[(layout, setup)])?
            .pop()
            .ok_or_else(|| crate::t!("Nothing to publish: no page could be built.").into_owned())
    }

    /// Report `sheets` of the job as not published.
    fn fail_sheets(
        &mut self,
        job: &mut SheetPublish,
        sheets: std::ops::Range<usize>,
        reason: &str,
    ) {
        let count = job.list.sheets.len();
        for k in sheets {
            job.failed += 1;
            self.command_line.push_error(
                crate::tf!(
                    "  [{}/{count}] {}: {error}",
                    k + 1,
                    job.list.sheets[k].name,
                    error = reason
                )
                .as_ref(),
            );
        }
    }

    /// Write the end of the PDF and report what was published.
    fn finish_sheet_publish(&mut self, mut job: Box<SheetPublish>) {
        let seconds = job.started.elapsed().as_secs_f64();
        let failed = job.failed;
        match (job.pdf.take(), &job.list.target) {
            (Some(pdf), Target::MultiSheetPdf(out)) => {
                let pages = pdf.page_count();
                match pdf.finish(&job.title, job.list.bookmarks) {
                    Ok(bytes) => self.command_line.push_output(
                        crate::tf!(
                            "Published: {}  {pages} page(s), {bytes} bytes, {seconds:.1} s",
                            out.display()
                        )
                        .as_ref(),
                    ),
                    Err(error) => self
                        .command_line
                        .push_error(crate::tf!("PUBLISH: {error}").as_ref()),
                }
            }
            (_, target) => {
                let folder = match target {
                    Target::SingleSheetPdfs(folder) => folder.display().to_string(),
                    Target::MultiSheetPdf(out) => out.display().to_string(),
                };
                self.command_line.push_output(
                    crate::tf!(
                        "Published: {} PDF file(s) to {folder}, {seconds:.1} s",
                        job.published
                    )
                    .as_ref(),
                );
            }
        }
        if failed > 0 {
            self.command_line
                .push_error(crate::tf!("PUBLISH: {failed} sheet(s) were not published.").as_ref());
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::app::OpenCADStudio;
    use std::path::{Path, PathBuf};

    /// A drawing with a line in model space, saved as DXF at `path`.
    fn drawing(path: &Path) {
        let mut doc = document::Document::new();
        let line = document::entities::Line::from_points(
            document::types::Vector3::new(0.0, 0.0, 0.0),
            document::types::Vector3::new(100.0, 50.0, 0.0),
        );
        doc.add_entity(document::EntityType::Line(line)).unwrap();
        std::fs::write(
            path,
            crate::io::save_to_bytes(&doc, "dxf", doc.version).unwrap(),
        )
        .unwrap();
    }

    fn sheet(name: &str, drawing: &Path, layout: &str) -> String {
        format!(
            "[DWF6Sheet:{name}]\nDWG={}\nLayout={layout}\nSetup=\nHas Plot Port=0\n",
            drawing.display()
        )
    }

    /// Bookmark titles of the PDF at `path`, in order, with the page number
    /// each opens.
    fn bookmarks(path: &Path) -> Vec<(String, u32)> {
        let doc = lopdf::Document::load(path).unwrap();
        let numbers: std::collections::HashMap<lopdf::ObjectId, u32> = doc
            .get_pages()
            .into_iter()
            .map(|(number, id)| (id, number))
            .collect();
        let catalog = doc.catalog().unwrap();
        let Ok(outlines) = catalog
            .get(b"Outlines")
            .and_then(lopdf::Object::as_reference)
        else {
            return Vec::new();
        };
        let mut item = doc
            .get_dictionary(outlines)
            .unwrap()
            .get(b"First")
            .and_then(lopdf::Object::as_reference)
            .ok();
        let mut titles = Vec::new();
        while let Some(id) = item {
            let dict = doc.get_dictionary(id).unwrap();
            let bytes = dict.get(b"Title").unwrap().as_str().unwrap();
            let title = match bytes.strip_prefix(&[0xFE, 0xFF]) {
                Some(utf16) => String::from_utf16(
                    &utf16
                        .chunks(2)
                        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
                        .collect::<Vec<_>>(),
                )
                .unwrap(),
                None => String::from_utf8(bytes.to_vec()).unwrap(),
            };
            let page = dict.get(b"Dest").unwrap().as_array().unwrap()[0]
                .as_reference()
                .unwrap();
            titles.push((title, numbers[&page]));
            item = dict.get(b"Next").and_then(lopdf::Object::as_reference).ok();
        }
        titles
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ocs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A batch run publishes a sheet list from no open drawing: the sheets
    /// of both drawings in list order, one bookmark each; sheets that cannot
    /// be plotted are reported and the rest still publish. The drawings are
    /// closed again and leave no lock file behind.
    #[test]
    fn a_sheet_list_publishes_the_sheets_of_many_drawings_into_one_pdf() {
        let dir = temp_dir("publish-list");
        let (a, b) = (dir.join("a.dxf"), dir.join("sub b.dxf"));
        drawing(&a);
        drawing(&b);
        let out = dir.join("set.pdf");
        let list = dir.join("set.dsd");
        std::fs::write(
            &list,
            [
                "[DWF6Version]\nVer=1\n".to_string(),
                sheet("0001 - Erstes Blatt", &a, "Layout1"),
                sheet("0002 - Übersicht", &b, "Layout1"),
                sheet("0003 - Model", &b, "Model"),
                sheet("0004 - Missing", &dir.join("missing.dxf"), "Layout1"),
                sheet("0005 - No layout", &a, "Nope"),
                sheet("0006 - Again", &a, "Model"),
                format!("[Target]\nType=6\nDWF={}\n", out.display()),
                "[PdfOptions]\nCreateBookmarks=TRUE\n".to_string(),
            ]
            .concat(),
        )
        .unwrap();
        let mut app = OpenCADStudio::new_for_test();
        let status = app.run_batch(&format!("-PUBLISH \"{}\"\n", list.display()), &[]);
        assert_eq!(status, 1, "two sheets fail");
        let output: Vec<String> = app
            .command_line
            .history
            .iter()
            .map(|entry| entry.text.clone())
            .collect();
        assert_eq!(
            bookmarks(&out),
            vec![
                ("0001 - Erstes Blatt".to_string(), 1),
                ("0002 - Übersicht".to_string(), 2),
                ("0003 - Model".to_string(), 3),
                ("0006 - Again".to_string(), 4),
            ],
            "{output:#?}"
        );
        assert!(
            app.tabs.iter().all(|tab| tab.is_start),
            "the drawings are closed"
        );
        let left: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains("lock") || name.starts_with('.'))
            .collect();
        assert!(left.is_empty(), "{left:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Typed at the command line: a drawing that is already open is
    /// published as it is and stays open; single-sheet output writes a PDF
    /// per sheet named after it.
    #[test]
    fn an_open_drawing_stays_open_and_single_sheet_lists_write_a_pdf_per_sheet() {
        let dir = temp_dir("publish-single");
        let a = dir.join("a.dxf");
        drawing(&a);
        let list = dir.join("single.dsd");
        std::fs::write(
            &list,
            [
                sheet("A: one", &a, "Layout1"),
                sheet("A two", &a, "Model"),
                "[Target]\nType=5\nOUT=.\n".to_string(),
            ]
            .concat(),
        )
        .unwrap();
        let mut app = OpenCADStudio::new_for_test();
        app.script_queue
            .push_back(format!("OPEN \"{}\"", a.display()));
        let task = app.update(crate::app::Message::ScriptPump);
        app.drive_headless_task(task).unwrap();
        assert!(!app.script_running);
        let task = app.dispatch_command(&format!("PUBLISH \"{}\"", list.display()));
        app.drive_headless_task(task).unwrap();
        assert!(app.sheet_publish.is_none(), "the job ran to its end");
        assert!(dir.join("A_ one.pdf").is_file());
        assert!(dir.join("A two.pdf").is_file());
        assert_eq!(app.tabs.iter().filter(|tab| !tab.is_start).count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The page annotations of a PDF page, as (action type, target).
    fn annotations(
        doc: &lopdf::Document,
        page: lopdf::ObjectId,
    ) -> Vec<(String, String, [f32; 4])> {
        let page = doc.get_dictionary(page).unwrap();
        let Ok(list) = page.get(b"Annots").and_then(lopdf::Object::as_array) else {
            return Vec::new();
        };
        list.iter()
            .map(|annotation| {
                let annotation = doc
                    .get_dictionary(annotation.as_reference().unwrap())
                    .unwrap();
                let action = annotation.get(b"A").unwrap().as_dict().unwrap();
                let kind = String::from_utf8(action.get(b"S").unwrap().as_name().unwrap().to_vec())
                    .unwrap();
                let target = action
                    .get(b"D")
                    .or_else(|_| action.get(b"URI"))
                    .unwrap()
                    .as_str()
                    .unwrap();
                let rect: Vec<f32> = annotation
                    .get(b"Rect")
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_float().unwrap())
                    .collect();
                (
                    kind,
                    String::from_utf8(target.to_vec()).unwrap(),
                    [rect[0], rect[1], rect[2], rect[3]],
                )
            })
            .collect()
    }

    /// A table-of-contents row the way openCirt links it: a text of spaces
    /// fitted over the row, linked to the other drawing (relative path, its
    /// layout). In the published PDF the row opens that sheet's page; a web
    /// link stays an address.
    #[test]
    fn a_link_to_a_sheet_of_the_list_opens_its_page() {
        use crate::scene::hyperlink::Link;
        use document::types::Vector3;
        let dir = temp_dir("publish-links");
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let (toc, b) = (dir.join("toc.dxf"), dir.join("sub").join("b.dxf"));
        drawing(&b);
        let mut doc = document::Document::new();
        let frame = document::entities::Line::from_points(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(200.0, 100.0, 0.0),
        );
        let frame = doc.add_entity(document::EntityType::Line(frame)).unwrap();
        let mut row = document::entities::Text::with_value(" ", Vector3::new(20.0, 50.0, 0.0));
        row.height = 10.0;
        row.horizontal_alignment = document::entities::TextHorizontalAlignment::Fit;
        row.alignment_point = Some(Vector3::new(180.0, 50.0, 0.0));
        let row = doc.add_entity(document::EntityType::Text(row)).unwrap();
        crate::scene::hyperlink::set(
            &mut doc,
            row,
            Some(&Link {
                url: "sub/b.dxf".into(),
                text: "Blatt B".into(),
                location: "Model".into(),
                flags: 0,
            }),
        );
        crate::scene::hyperlink::set(
            &mut doc,
            frame,
            Some(&Link {
                url: "https://example.org/".into(),
                ..Default::default()
            }),
        );
        std::fs::write(
            &toc,
            crate::io::save_to_bytes(&doc, "dxf", doc.version).unwrap(),
        )
        .unwrap();
        let out = dir.join("set.pdf");
        let list = dir.join("set.dsd");
        std::fs::write(
            &list,
            [
                sheet("0001 - Inhalt", &toc, "Model"),
                sheet("0002 - Blatt B", &b, "Model"),
                format!("[Target]\nType=6\nDWF={}\n", out.display()),
            ]
            .concat(),
        )
        .unwrap();
        let mut app = OpenCADStudio::new_for_test();
        assert_eq!(
            app.run_batch(&format!("PUBLISH \"{}\"\n", list.display()), &[]),
            0
        );
        let pdf = lopdf::Document::load(&out).unwrap();
        let pages: Vec<lopdf::ObjectId> = pdf.get_pages().into_values().collect();
        let links = annotations(&pdf, pages[0]);
        assert_eq!(links.len(), 2, "{links:?}");
        let row_link = links.iter().find(|link| link.0 == "GoTo").unwrap();
        assert_eq!(row_link.1, "OcsSheet000002");
        // The row's link is far wider than high.
        let [x0, y0, x1, y1] = row_link.2;
        assert!(x1 - x0 > 5.0 * (y1 - y0) && y1 > y0, "{row_link:?}");
        assert!(links
            .iter()
            .any(|link| link.0 == "URI" && link.1 == "https://example.org/"));
        let names = pdf
            .catalog()
            .unwrap()
            .get(b"Names")
            .unwrap()
            .as_dict()
            .unwrap();
        let tree = names
            .get(b"Dests")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"Names")
            .unwrap()
            .as_array()
            .unwrap();
        let target = tree
            .chunks(2)
            .find(|pair| pair[0].as_str().unwrap() == b"OcsSheet000002")
            .unwrap();
        assert_eq!(
            target[1].as_array().unwrap()[0].as_reference().unwrap(),
            pages[1]
        );
        assert!(annotations(&pdf, pages[1]).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
