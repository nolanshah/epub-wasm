//! Main Book struct and API

use std::collections::HashMap;
use std::path::Path;

use crate::archive::Archive;
use crate::cfi::Cfi;
use crate::container;
use crate::error::{EpubError, Result};
use crate::navigation::{parse_nav, parse_ncx, resolve_hrefs, NavItem};
use crate::package::{ManifestItem, Metadata, Package, SpineItem};
use crate::path;
use crate::search::{matches_in_map, SearchMatch, SearchOptions};
use crate::section::Section;
use crate::text_map::TextMap;

/// Main EPUB book structure
pub struct Book {
    /// Book metadata
    pub metadata: Metadata,
    /// Spine items (reading order)
    pub spine: Vec<SpineItem>,
    /// Manifest items (all resources), keyed by id. Hrefs are as written in
    /// the OPF (relative to the OPF directory); use [`Book::manifest_path`]
    /// for archive paths.
    pub manifest: HashMap<String, ManifestItem>,
    /// Table of contents. Hrefs are normalized archive paths (+ `#fragment`).
    pub toc: Vec<NavItem>,
    /// `page-progression-direction` from the spine (`ltr`, `rtl`), if declared
    pub page_progression_direction: Option<String>,
    /// Sections (loaded on demand)
    sections: Vec<Section>,
    /// The archive containing all files
    archive: Archive,
    /// Base path for resolving relative URLs (directory containing OPF, with trailing slash)
    base_path: String,
    /// Archive path -> manifest id
    path_to_id: HashMap<String, String>,
}

impl Book {
    /// Open an EPUB from a file path
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        let archive = Archive::from_path(path)?;
        Self::from_archive(archive)
    }

    /// Open an EPUB from bytes
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        let archive = Archive::from_bytes(bytes)?;
        Self::from_archive(archive)
    }

    /// Open an EPUB from an archive
    fn from_archive(archive: Archive) -> Result<Self> {
        // Parse container.xml to find OPF path
        let container_xml = archive.get_file_string(container::container_path())?;
        let opf_path = path::normalize(&path::percent_decode(&container::parse_container(
            &container_xml,
        )?));

        // Base path (directory containing OPF)
        let base_path = path::dir_of(&opf_path).to_string();

        // Parse OPF
        let opf_xml = archive.get_file_string(&opf_path)?;
        let package = Package::parse(&opf_xml)?;

        // Archive path -> manifest id lookup
        let path_to_id: HashMap<String, String> = package
            .manifest
            .values()
            .map(|item| (path::resolve(&base_path, &item.href).0, item.id.clone()))
            .collect();

        // Parse TOC: NAV (EPUB3) first, fall back to NCX (EPUB2).
        let toc = Self::load_toc(&archive, &base_path, &package);

        // Build sections from spine
        let sections: Vec<Section> = package
            .spine
            .iter()
            .filter_map(|spine_item| {
                let manifest_item = package.manifest.get(&spine_item.idref)?;
                let href = path::resolve(&base_path, &manifest_item.href).0;
                Some((spine_item, href, manifest_item.media_type.clone()))
            })
            .enumerate()
            .map(|(i, (spine_item, href, media_type))| {
                Section::new(i, spine_item, href, media_type)
            })
            .collect();

        if sections.is_empty() {
            return Err(EpubError::InvalidStructure(
                "Spine references no manifest items".to_string(),
            ));
        }

        Ok(Book {
            metadata: package.metadata,
            spine: package.spine,
            manifest: package.manifest,
            toc,
            page_progression_direction: package.page_progression_direction,
            sections,
            archive,
            base_path,
            path_to_id,
        })
    }

    fn load_toc(archive: &Archive, base_path: &str, package: &Package) -> Vec<NavItem> {
        let try_load = |href: &str, parse: fn(&str) -> Result<Vec<NavItem>>| {
            let full_path = path::resolve(base_path, href).0;
            let xml = archive.get_file_string(&full_path).ok()?;
            let mut items = parse(&xml).ok()?;
            resolve_hrefs(&mut items, path::dir_of(&full_path));
            Some(items)
        };

        package
            .nav_path
            .as_deref()
            .and_then(|p| try_load(p, parse_nav))
            .filter(|items| !items.is_empty())
            .or_else(|| {
                package
                    .ncx_path
                    .as_deref()
                    .and_then(|p| try_load(p, parse_ncx))
            })
            .unwrap_or_default()
    }

    /// Get the number of sections (spine items)
    pub fn section_count(&self) -> usize {
        self.sections.len()
    }

    /// Get a section by index
    pub fn section(&self, index: usize) -> Option<&Section> {
        self.sections.get(index)
    }

    /// Get a mutable section by index
    pub fn section_mut(&mut self, index: usize) -> Option<&mut Section> {
        self.sections.get_mut(index)
    }

    /// Load content for a section
    pub fn load_section(&mut self, index: usize) -> Result<&Section> {
        let section = self
            .sections
            .get_mut(index)
            .ok_or_else(|| EpubError::ContentNotFound(format!("Section {} not found", index)))?;

        if !section.is_loaded() {
            let content = self.archive.get_file_string(&section.href)?;
            section.set_content(content);
        }

        Ok(self.sections.get(index).unwrap())
    }

    /// Get section content (loading if necessary)
    pub fn section_content(&mut self, index: usize) -> Result<&str> {
        self.load_section(index)?;
        Ok(self.sections[index].content().unwrap())
    }

    /// Get the plain text of a section, whitespace-collapsed (loading if necessary)
    pub fn section_text(&mut self, index: usize) -> Result<&str> {
        self.load_section(index)?;
        Ok(self.sections[index].text().unwrap())
    }

    /// Get the text/position map of a section (loading if necessary)
    pub fn section_map(&mut self, index: usize) -> Result<&TextMap> {
        self.load_section(index)?;
        Ok(self.sections[index].map().unwrap())
    }

    /// Resolve an href to an archive path. Tries it as archive-absolute first,
    /// then relative to the OPF directory. Percent-encoding and `../` are handled.
    fn resource_path(&self, href: &str) -> Option<String> {
        let (absolute, _) = path::resolve("", href);
        if self.archive.has_file(&absolute) {
            return Some(absolute);
        }
        let (relative, _) = path::resolve(&self.base_path, href);
        if self.archive.has_file(&relative) {
            return Some(relative);
        }
        None
    }

    /// Get a resource by href (archive path, or relative to the OPF directory)
    pub fn get_resource(&self, href: &str) -> Option<&[u8]> {
        let p = self.resource_path(href)?;
        self.archive.get_file(&p)
    }

    /// Get a resource by manifest ID
    pub fn get_resource_by_id(&self, id: &str) -> Option<&[u8]> {
        let item = self.manifest.get(id)?;
        self.get_resource(&item.href)
    }

    /// Archive path of a manifest item
    pub fn manifest_path(&self, id: &str) -> Option<String> {
        let item = self.manifest.get(id)?;
        Some(path::resolve(&self.base_path, &item.href).0)
    }

    /// Manifest item for an archive path
    pub fn manifest_item_for_path(&self, archive_path: &str) -> Option<&ManifestItem> {
        let id = self.path_to_id.get(archive_path)?;
        self.manifest.get(id)
    }

    /// MIME type for an archive path: from the manifest if declared there,
    /// otherwise guessed from the extension.
    pub fn media_type_for(&self, archive_path: &str) -> String {
        match self.manifest_item_for_path(archive_path) {
            Some(item) if !item.media_type.is_empty() => item.media_type.clone(),
            _ => path::mime_type_from_extension(archive_path).to_string(),
        }
    }

    /// Resolve an href that appears inside a section's content to
    /// `(archive_path, fragment)`; `None` if the href is external.
    pub fn resolve_from_section(
        &self,
        section_index: usize,
        href: &str,
    ) -> Option<(String, Option<String>)> {
        let section = self.section(section_index)?;
        if path::is_external(href) {
            return None;
        }
        let (mut p, frag) = section.resolve_href(href);
        if p.is_empty() {
            p = section.href.clone(); // "#fragment" refers to the same document
        }
        Some((p, frag))
    }

    /// Resolve an href inside a section to the resource bytes it points to.
    pub fn resource_from_section(&self, section_index: usize, href: &str) -> Option<(String, &[u8])> {
        let (p, _) = self.resolve_from_section(section_index, href)?;
        let data = self.archive.get_file(&p)?;
        Some((p, data))
    }

    /// Get the cover image data if available
    pub fn cover_image(&self) -> Option<&[u8]> {
        self.cover_path().and_then(|p| self.archive.get_file(&p))
    }

    /// Archive path of the cover image, if any
    pub fn cover_path(&self) -> Option<String> {
        // EPUB3: item with the cover-image property
        for item in self.manifest.values() {
            if item.properties.iter().any(|p| p == "cover-image") {
                return Some(path::resolve(&self.base_path, &item.href).0);
            }
        }

        // EPUB2: <meta name="cover" content="id"/>
        if let Some(ref cover_id) = self.metadata.cover_id {
            if let Some(p) = self.manifest_path(cover_id) {
                return Some(p);
            }
            // Some producers put the href in `content` instead of the id
            if let Some(p) = self.resource_path(cover_id) {
                return Some(p);
            }
        }

        // Heuristic: a manifest image whose id or href mentions "cover"
        self.manifest
            .values()
            .filter(|item| item.media_type.starts_with("image/"))
            .find(|item| {
                item.id.to_ascii_lowercase().contains("cover")
                    || item.href.to_ascii_lowercase().contains("cover")
            })
            .map(|item| path::resolve(&self.base_path, &item.href).0)
    }

    /// Search the entire book
    pub fn search(&mut self, query: &str, options: &SearchOptions) -> Result<Vec<SearchMatch>> {
        let mut all_matches = Vec::new();

        if query.is_empty() {
            return Ok(all_matches);
        }

        for i in 0..self.sections.len() {
            let remaining = options
                .max_results
                .map(|max| max.saturating_sub(all_matches.len()));
            if remaining == Some(0) {
                break;
            }

            let map = match self.section_map(i) {
                Ok(m) => m,
                // A single unreadable section shouldn't abort the whole search
                Err(_) => continue,
            };

            let section_opts = SearchOptions {
                max_results: remaining,
                ..options.clone()
            };

            all_matches.extend(matches_in_map(map, query, i, &section_opts));
        }

        Ok(all_matches)
    }

    /// Navigate to a CFI
    pub fn go_to_cfi(&mut self, cfi: &Cfi) -> Result<&Section> {
        self.load_section(cfi.spine_index)
    }

    /// Get the next section
    pub fn next_section(&self, current: usize) -> Option<usize> {
        let next = current + 1;
        if next < self.sections.len() {
            Some(next)
        } else {
            None
        }
    }

    /// Get the previous section
    pub fn prev_section(&self, current: usize) -> Option<usize> {
        if current > 0 {
            Some(current - 1)
        } else {
            None
        }
    }

    /// Find a TOC item by href (fragment-insensitive)
    pub fn find_toc_item(&self, href: &str) -> Option<&NavItem> {
        fn find_in_items<'a>(items: &'a [NavItem], target: &str) -> Option<&'a NavItem> {
            for item in items {
                let (item_path, _) = path::split_fragment(&item.href);
                if item_path == target || item.href == target {
                    return Some(item);
                }
                if let Some(found) = find_in_items(&item.children, target) {
                    return Some(found);
                }
            }
            None
        }

        let target = self.section_index_by_href(href).map(|i| self.sections[i].href.clone());
        let target = target.as_deref().unwrap_or(href);
        find_in_items(&self.toc, target)
    }

    /// Section index for an archive path (exact match, fragment ignored)
    pub fn section_index_by_path(&self, archive_path: &str) -> Option<usize> {
        let (p, _) = path::split_fragment(archive_path);
        self.sections.iter().position(|s| s.href == p)
    }

    /// Section index for an href. Accepts an archive path, a path relative to
    /// the OPF directory, or (as a last resort) a bare filename.
    pub fn section_index_by_href(&self, href: &str) -> Option<usize> {
        let (raw, _) = path::split_fragment(href);

        let (absolute, _) = path::resolve("", raw);
        if let Some(i) = self.section_index_by_path(&absolute) {
            return Some(i);
        }

        let (relative, _) = path::resolve(&self.base_path, raw);
        if let Some(i) = self.section_index_by_path(&relative) {
            return Some(i);
        }

        // Fallback: filename match (unique only)
        let filename = absolute.rsplit('/').next()?;
        if filename.is_empty() {
            return None;
        }
        let mut candidates = self
            .sections
            .iter()
            .filter(|s| s.href.rsplit('/').next() == Some(filename));
        let first = candidates.next()?;
        if candidates.next().is_some() {
            return None;
        }
        Some(first.index)
    }

    /// Resolve an href found inside `section_index` (a TOC entry, an `<a href>`)
    /// to `(section_index, fragment)`. Returns `None` for external links or
    /// targets that are not spine items.
    pub fn resolve_href(&self, section_index: usize, href: &str) -> Option<(usize, Option<String>)> {
        let (p, frag) = self.resolve_from_section(section_index, href)?;
        let idx = self.section_index_by_path(&p)?;
        Some((idx, frag))
    }

    /// Get an iterator over all sections
    pub fn sections(&self) -> impl Iterator<Item = &Section> {
        self.sections.iter()
    }

    /// Flatten the TOC into a list
    pub fn flat_toc(&self) -> Vec<&NavItem> {
        fn flatten<'a>(items: &'a [NavItem], result: &mut Vec<&'a NavItem>) {
            for item in items {
                result.push(item);
                flatten(&item.children, result);
            }
        }

        let mut result = Vec::new();
        flatten(&self.toc, &mut result);
        result
    }

    /// The archive path of the OPF directory (with trailing slash, or empty)
    pub fn base_path(&self) -> &str {
        &self.base_path
    }

    /// All file paths in the archive
    pub fn archive_paths(&self) -> impl Iterator<Item = &str> {
        self.archive.list_files()
    }
}
