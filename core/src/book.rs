//! Main Book struct and API

use std::collections::HashMap;
use std::path::Path;

use crate::archive::Archive;
use crate::cfi::Cfi;
use crate::container;
use crate::error::{EpubError, Result};
use crate::navigation::{parse_nav, parse_ncx, NavItem};
use crate::package::{ManifestItem, Metadata, Package, SpineItem};
use crate::search::{search_content, SearchMatch, SearchOptions};
use crate::section::Section;

/// Main EPUB book structure
pub struct Book {
    /// Book metadata
    pub metadata: Metadata,
    /// Spine items (reading order)
    pub spine: Vec<SpineItem>,
    /// Manifest items (all resources)
    pub manifest: HashMap<String, ManifestItem>,
    /// Table of contents
    pub toc: Vec<NavItem>,
    /// Sections (loaded on demand)
    sections: Vec<Section>,
    /// The archive containing all files
    archive: Archive,
    /// Base path for resolving relative URLs (directory containing OPF)
    base_path: String,
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
        let opf_path = container::parse_container(&container_xml)?;

        // Get base path (directory containing OPF)
        let base_path = match opf_path.rfind('/') {
            Some(pos) => format!("{}/", &opf_path[..pos]),
            None => String::new(),
        };

        // Parse OPF
        let opf_xml = archive.get_file_string(&opf_path)?;
        let package = Package::parse(&opf_xml)?;

        // Parse TOC (try NAV first, then NCX)
        let toc = if let Some(ref nav_path) = package.nav_path {
            let full_path = format!("{}{}", base_path, nav_path);
            let nav_html = archive.get_file_string(&full_path)?;
            parse_nav(&nav_html).unwrap_or_default()
        } else if let Some(ref ncx_path) = package.ncx_path {
            let full_path = format!("{}{}", base_path, ncx_path);
            let ncx_xml = archive.get_file_string(&full_path)?;
            parse_ncx(&ncx_xml).unwrap_or_default()
        } else {
            Vec::new()
        };

        // Build sections from spine
        let sections: Vec<Section> = package
            .spine
            .iter()
            .enumerate()
            .filter_map(|(i, spine_item)| {
                let manifest_item = package.manifest.get(&spine_item.idref)?;
                let href = format!("{}{}", base_path, manifest_item.href);
                Some(Section::new(
                    i,
                    spine_item,
                    href,
                    manifest_item.media_type.clone(),
                ))
            })
            .collect();

        Ok(Book {
            metadata: package.metadata,
            spine: package.spine,
            manifest: package.manifest,
            toc,
            sections,
            archive,
            base_path,
        })
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

    /// Get a resource by href (relative to book base)
    pub fn get_resource(&self, href: &str) -> Option<&[u8]> {
        // First try with base path
        if let Some(data) = self.archive.get_file(&format!("{}{}", self.base_path, href)) {
            return Some(data);
        }
        // Try as absolute path
        self.archive.get_file(href)
    }

    /// Get a resource by manifest ID
    pub fn get_resource_by_id(&self, id: &str) -> Option<&[u8]> {
        let item = self.manifest.get(id)?;
        self.get_resource(&item.href)
    }

    /// Get the cover image data if available
    pub fn cover_image(&self) -> Option<&[u8]> {
        // Try cover_id from metadata
        if let Some(ref cover_id) = self.metadata.cover_id {
            if let Some(data) = self.get_resource_by_id(cover_id) {
                return Some(data);
            }
        }

        // Try to find item with cover-image property
        for item in self.manifest.values() {
            if item.properties.contains(&"cover-image".to_string()) {
                if let Some(data) = self.get_resource(&item.href) {
                    return Some(data);
                }
            }
        }

        None
    }

    /// Search the entire book
    pub fn search(&mut self, query: &str, options: &SearchOptions) -> Result<Vec<SearchMatch>> {
        let mut all_matches = Vec::new();

        for i in 0..self.sections.len() {
            // Load section content
            let content = self.section_content(i)?.to_string();

            // Search this section
            let section_matches = search_content(&content, query, i, options);
            all_matches.extend(section_matches);

            // Check max results
            if let Some(max) = options.max_results {
                if all_matches.len() >= max {
                    all_matches.truncate(max);
                    break;
                }
            }
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

    /// Find a TOC item by href
    pub fn find_toc_item(&self, href: &str) -> Option<&NavItem> {
        fn find_in_items<'a>(items: &'a [NavItem], href: &str) -> Option<&'a NavItem> {
            for item in items {
                // Check if href matches (ignoring fragment)
                let item_href = item.href.split('#').next().unwrap_or(&item.href);
                let target_href = href.split('#').next().unwrap_or(href);

                if item_href == target_href || item.href == href {
                    return Some(item);
                }

                if let Some(found) = find_in_items(&item.children, href) {
                    return Some(found);
                }
            }
            None
        }

        find_in_items(&self.toc, href)
    }

    /// Get section index by href
    pub fn section_index_by_href(&self, href: &str) -> Option<usize> {
        let normalized = href.split('#').next().unwrap_or(href);

        self.sections.iter().position(|s| {
            let section_href = s.href.split('#').next().unwrap_or(&s.href);
            section_href.ends_with(normalized) || normalized.ends_with(section_href)
        })
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
}

#[cfg(test)]
mod tests {
    // Integration tests would go here with actual EPUB files
}
