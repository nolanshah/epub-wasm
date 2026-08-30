//! CFI (Canonical Fragment Identifier) parsing and generation
//!
//! CFI is a standard way to reference locations within an EPUB.
//! Format: epubcfi(/6/4!/4/2/1:0)
//!
//! Structure:
//! - /6/4 - spine position (package document path)
//! - ! - step into referenced document
//! - /4/2/1 - path within document
//! - :0 - character offset

use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;

use crate::error::{EpubError, Result};

/// A parsed CFI (Canonical Fragment Identifier)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cfi {
    /// Index into the spine (0-based)
    pub spine_index: usize,
    /// Path within the document
    pub path: Vec<CfiStep>,
    /// Character offset at the final position
    pub character_offset: Option<usize>,
    /// Temporal offset (for audio/video)
    pub temporal_offset: Option<f64>,
    /// Spatial offset (x, y percentages)
    pub spatial_offset: Option<(f64, f64)>,
}

/// A single step in a CFI path
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CfiStep {
    /// The index at this level (1-based in CFI, stored as-is)
    pub index: usize,
    /// Optional ID assertion
    pub id: Option<String>,
}

impl Cfi {
    /// Parse a CFI string
    pub fn parse(cfi: &str) -> Result<Self> {
        // Remove epubcfi() wrapper if present
        let cfi = cfi
            .strip_prefix("epubcfi(")
            .and_then(|s| s.strip_suffix(')'))
            .unwrap_or(cfi);

        // Split on the ! to separate spine and document paths
        let parts: Vec<&str> = cfi.split('!').collect();

        let (spine_path, doc_path) = match parts.len() {
            1 => (parts[0], None),
            2 => (parts[0], Some(parts[1])),
            _ => {
                return Err(EpubError::InvalidCfi(
                    "Multiple ! separators in CFI".to_string(),
                ))
            }
        };

        // Parse spine path to get spine index
        let spine_index = parse_spine_index(spine_path)?;

        // Parse document path
        let (path, character_offset, temporal_offset, spatial_offset) = match doc_path {
            Some(p) => parse_document_path(p)?,
            None => (Vec::new(), None, None, None),
        };

        Ok(Cfi {
            spine_index,
            path,
            character_offset,
            temporal_offset,
            spatial_offset,
        })
    }

    /// Create a CFI for a spine index
    pub fn from_spine_index(index: usize) -> Self {
        Cfi {
            spine_index: index,
            path: Vec::new(),
            character_offset: None,
            temporal_offset: None,
            spatial_offset: None,
        }
    }

    /// Create a CFI with a path
    pub fn from_path(spine_index: usize, path: Vec<CfiStep>) -> Self {
        Cfi {
            spine_index,
            path,
            character_offset: None,
            temporal_offset: None,
            spatial_offset: None,
        }
    }

    /// Compare two CFIs for ordering
    pub fn compare(&self, other: &Cfi) -> Ordering {
        // First compare spine index
        match self.spine_index.cmp(&other.spine_index) {
            Ordering::Equal => {}
            ord => return ord,
        }

        // Compare paths step by step
        for (a, b) in self.path.iter().zip(other.path.iter()) {
            match a.index.cmp(&b.index) {
                Ordering::Equal => continue,
                ord => return ord,
            }
        }

        // Shorter path comes first
        match self.path.len().cmp(&other.path.len()) {
            Ordering::Equal => {}
            ord => return ord,
        }

        // Compare character offsets
        match (self.character_offset, other.character_offset) {
            (Some(a), Some(b)) => a.cmp(&b),
            (None, Some(_)) => Ordering::Less,
            (Some(_), None) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
    }
}

impl fmt::Display for Cfi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "epubcfi(")?;

        // Spine path: /6/{spine_index * 2 + 2}
        // (spine items are even-numbered starting at 2)
        write!(f, "/6/{}", (self.spine_index + 1) * 2)?;

        if !self.path.is_empty() || self.character_offset.is_some() {
            write!(f, "!")?;

            for step in &self.path {
                write!(f, "/{}", step.index)?;
                if let Some(ref id) = step.id {
                    write!(f, "[{}]", id)?;
                }
            }

            if let Some(offset) = self.character_offset {
                write!(f, ":{}", offset)?;
            }

            if let Some(t) = self.temporal_offset {
                write!(f, "~{}", t)?;
            }

            if let Some((x, y)) = self.spatial_offset {
                write!(f, "@{}:{}", x, y)?;
            }
        }

        write!(f, ")")
    }
}

impl PartialOrd for Cfi {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.compare(other))
    }
}

/// A CFI range: `epubcfi(parent,start,end)`, where the full start point is
/// `parent + start` and the full end point is `parent + end`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CfiRange {
    pub start: Cfi,
    pub end: Cfi,
}

impl CfiRange {
    /// Parse a range CFI string.
    pub fn parse(s: &str) -> Result<Self> {
        let inner = s
            .strip_prefix("epubcfi(")
            .and_then(|x| x.strip_suffix(')'))
            .unwrap_or(s);

        let parts: Vec<&str> = inner.split(',').collect();
        if parts.len() != 3 {
            return Err(EpubError::InvalidCfi(format!(
                "Not a range CFI (expected 2 commas): {}",
                s
            )));
        }

        let start = Cfi::parse(&format!("{}{}", parts[0], parts[1]))?;
        let end = Cfi::parse(&format!("{}{}", parts[0], parts[2]))?;
        Ok(CfiRange { start, end })
    }

    /// Order by start point, then end point.
    pub fn compare(&self, other: &CfiRange) -> Ordering {
        match self.start.compare(&other.start) {
            Ordering::Equal => self.end.compare(&other.end),
            ord => ord,
        }
    }
}

impl fmt::Display for CfiRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "epubcfi(/6/{}!", (self.start.spine_index + 1) * 2)?;

        // Longest common step prefix, keeping at least one step (or the
        // offset) in each local part.
        let s = &self.start.path;
        let e = &self.end.path;
        let mut lcp = 0;
        while lcp < s.len() && lcp < e.len() && s[lcp] == e[lcp] {
            lcp += 1;
        }
        if lcp == s.len() && lcp == e.len() {
            lcp = lcp.saturating_sub(1);
        }

        let write_steps = |f: &mut fmt::Formatter<'_>, steps: &[CfiStep]| -> fmt::Result {
            for step in steps {
                write!(f, "/{}", step.index)?;
                if let Some(ref id) = step.id {
                    write!(f, "[{}]", id)?;
                }
            }
            Ok(())
        };

        write_steps(f, &s[..lcp])?;

        write!(f, ",")?;
        write_steps(f, &s[lcp..])?;
        if let Some(o) = self.start.character_offset {
            write!(f, ":{}", o)?;
        }

        write!(f, ",")?;
        write_steps(f, &e[lcp..])?;
        if let Some(o) = self.end.character_offset {
            write!(f, ":{}", o)?;
        }

        write!(f, ")")
    }
}

fn parse_spine_index(path: &str) -> Result<usize> {
    // Spine path format: /6/N where N is (spine_index + 1) * 2
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    if parts.len() < 2 {
        return Err(EpubError::InvalidCfi(format!(
            "Invalid spine path: {}",
            path
        )));
    }

    // The second number (after /6/) gives us the spine index
    let n: usize = parse_step_index(parts[1])?;

    // Convert from CFI numbering (N = (index + 1) * 2)
    if n < 2 || n % 2 != 0 {
        return Err(EpubError::InvalidCfi(format!(
            "Invalid spine index: {}",
            n
        )));
    }

    Ok(n / 2 - 1)
}

fn parse_step_index(step: &str) -> Result<usize> {
    // Parse "N" or "N[id]"
    let idx_str = step.split('[').next().unwrap_or(step);
    idx_str.parse().map_err(|_| {
        EpubError::InvalidCfi(format!("Invalid step index: {}", step))
    })
}

fn parse_document_path(
    path: &str,
) -> Result<(Vec<CfiStep>, Option<usize>, Option<f64>, Option<(f64, f64)>)> {
    let mut steps = Vec::new();
    let mut char_offset = None;
    let mut temporal_offset = None;
    let mut spatial_offset = None;

    // Split by / and process each step
    for part in path.split('/').filter(|s| !s.is_empty()) {
        // Check for character offset
        if let Some(colon_pos) = part.find(':') {
            let (step_part, offset_part) = part.split_at(colon_pos);

            if !step_part.is_empty() {
                steps.push(parse_cfi_step(step_part)?);
            }

            // Parse offset (may have temporal/spatial suffixes)
            let offset_str = &offset_part[1..];
            let (offset, remaining) = parse_offset_value(offset_str)?;
            char_offset = Some(offset);

            // Check for temporal offset (~)
            if let Some(t) = remaining.strip_prefix('~') {
                let (t_val, remaining2) = parse_float_value(t)?;
                temporal_offset = Some(t_val);

                // Check for spatial offset (@)
                if let Some(s) = remaining2.strip_prefix('@') {
                    spatial_offset = Some(parse_spatial_offset(s)?);
                }
            } else if let Some(s) = remaining.strip_prefix('@') {
                spatial_offset = Some(parse_spatial_offset(s)?);
            }

            continue;
        }

        // Check for temporal offset without character offset
        if let Some(tilde_pos) = part.find('~') {
            let (step_part, offset_part) = part.split_at(tilde_pos);

            if !step_part.is_empty() {
                steps.push(parse_cfi_step(step_part)?);
            }

            let (t_val, remaining) = parse_float_value(&offset_part[1..])?;
            temporal_offset = Some(t_val);

            if let Some(s) = remaining.strip_prefix('@') {
                spatial_offset = Some(parse_spatial_offset(s)?);
            }

            continue;
        }

        // Check for spatial offset only
        if let Some(at_pos) = part.find('@') {
            let (step_part, offset_part) = part.split_at(at_pos);

            if !step_part.is_empty() {
                steps.push(parse_cfi_step(step_part)?);
            }

            spatial_offset = Some(parse_spatial_offset(&offset_part[1..])?);
            continue;
        }

        steps.push(parse_cfi_step(part)?);
    }

    Ok((steps, char_offset, temporal_offset, spatial_offset))
}

fn parse_cfi_step(s: &str) -> Result<CfiStep> {
    let mut id = None;

    let index_str = if let Some(bracket_pos) = s.find('[') {
        if let Some(end_bracket) = s.find(']') {
            id = Some(s[bracket_pos + 1..end_bracket].to_string());
        }
        &s[..bracket_pos]
    } else {
        s
    };

    let index = index_str.parse().map_err(|_| {
        EpubError::InvalidCfi(format!("Invalid step index: {}", s))
    })?;

    Ok(CfiStep { index, id })
}

fn parse_offset_value(s: &str) -> Result<(usize, &str)> {
    let end = s
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(s.len());
    let value = s[..end].parse().map_err(|_| {
        EpubError::InvalidCfi(format!("Invalid offset: {}", s))
    })?;
    Ok((value, &s[end..]))
}

fn parse_float_value(s: &str) -> Result<(f64, &str)> {
    let end = s
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(s.len());
    let value = s[..end].parse().map_err(|_| {
        EpubError::InvalidCfi(format!("Invalid float: {}", s))
    })?;
    Ok((value, &s[end..]))
}

fn parse_spatial_offset(s: &str) -> Result<(f64, f64)> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 2 {
        return Err(EpubError::InvalidCfi(format!(
            "Invalid spatial offset: {}",
            s
        )));
    }

    let x: f64 = parts[0].parse().map_err(|_| {
        EpubError::InvalidCfi(format!("Invalid x coordinate: {}", parts[0]))
    })?;
    let y: f64 = parts[1].parse().map_err(|_| {
        EpubError::InvalidCfi(format!("Invalid y coordinate: {}", parts[1]))
    })?;

    Ok((x, y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_cfi() {
        let cfi = Cfi::parse("epubcfi(/6/4!/4/2:10)").unwrap();
        assert_eq!(cfi.spine_index, 1);
        assert_eq!(cfi.path.len(), 2);
        assert_eq!(cfi.path[0].index, 4);
        assert_eq!(cfi.path[1].index, 2);
        assert_eq!(cfi.character_offset, Some(10));
    }

    #[test]
    fn test_parse_cfi_with_id() {
        let cfi = Cfi::parse("epubcfi(/6/2!/4[body]/2[chapter1])").unwrap();
        assert_eq!(cfi.spine_index, 0);
        assert_eq!(cfi.path[0].id, Some("body".to_string()));
        assert_eq!(cfi.path[1].id, Some("chapter1".to_string()));
    }

    #[test]
    fn test_cfi_display() {
        let cfi = Cfi {
            spine_index: 1,
            path: vec![
                CfiStep { index: 4, id: None },
                CfiStep { index: 2, id: None },
            ],
            character_offset: Some(10),
            temporal_offset: None,
            spatial_offset: None,
        };

        assert_eq!(cfi.to_string(), "epubcfi(/6/4!/4/2:10)");
    }

    #[test]
    fn test_cfi_ordering() {
        let cfi1 = Cfi::parse("epubcfi(/6/2!/4/2:0)").unwrap();
        let cfi2 = Cfi::parse("epubcfi(/6/2!/4/2:10)").unwrap();
        let cfi3 = Cfi::parse("epubcfi(/6/4!/4/2:0)").unwrap();

        assert!(cfi1 < cfi2);
        assert!(cfi2 < cfi3);
    }

    #[test]
    fn test_from_spine_index() {
        let cfi = Cfi::from_spine_index(5);
        assert_eq!(cfi.to_string(), "epubcfi(/6/12)");
    }

    #[test]
    fn range_parse_and_display_round_trip() {
        let s = "epubcfi(/6/4!/4/2,/1:5,/2/1:3)";
        let r = CfiRange::parse(s).unwrap();
        assert_eq!(r.start.spine_index, 1);
        assert_eq!(r.start.path.iter().map(|p| p.index).collect::<Vec<_>>(), vec![4, 2, 1]);
        assert_eq!(r.start.character_offset, Some(5));
        assert_eq!(r.end.path.iter().map(|p| p.index).collect::<Vec<_>>(), vec![4, 2, 2, 1]);
        assert_eq!(r.end.character_offset, Some(3));
        assert_eq!(r.to_string(), s);
    }

    #[test]
    fn range_within_one_chunk() {
        let s = "epubcfi(/6/2!/4/2,/1:6,/1:11)";
        let r = CfiRange::parse(s).unwrap();
        assert_eq!(r.start.character_offset, Some(6));
        assert_eq!(r.end.character_offset, Some(11));
        assert_eq!(
            r.start.path.iter().map(|p| p.index).collect::<Vec<_>>(),
            vec![4, 2, 1]
        );
        assert_eq!(r.to_string(), s);
    }

    #[test]
    fn range_ordering_and_errors() {
        let a = CfiRange::parse("epubcfi(/6/2!/4/2/1,:0,:5)").unwrap();
        let b = CfiRange::parse("epubcfi(/6/2!/4/2/1,:3,:8)").unwrap();
        assert_eq!(a.compare(&b), std::cmp::Ordering::Less);

        assert!(CfiRange::parse("epubcfi(/6/2!/4/2/1:3)").is_err());
        // Point CFIs still reject range syntax
        assert!(Cfi::parse("epubcfi(/6/4!/4/2,/1:5,/2/1:3)").is_err());
    }
}
