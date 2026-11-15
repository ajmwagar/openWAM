//! WAM 2.4 imports `<path>` elements only (see `docs/wzr-format.md`, fileBB rule).
//! `paths_only` rebuilds an SVG the way WAM sees it: the root size attributes and
//! every rendered `<path>`, with ancestor transforms folded into one `matrix()`.
//! Editor namespaces, groups, rect/circle shapes and styling are dropped.

use crate::bbox::{BBoxError, node_ctm};

/// Result of [`paths_only`].
#[derive(Debug, Clone, PartialEq)]
pub struct PathsOnly {
    pub svg: String,
    pub paths: usize,
    /// Rendered `rect`/`circle`/`ellipse`/`line`/`polyline`/`polygon` elements WAM ignores.
    pub ignored_shapes: usize,
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn paths_only(svg: &str) -> Result<PathsOnly, BBoxError> {
    let doc = roxmltree::Document::parse(svg)?;
    let root = doc.root_element();
    if root.tag_name().name() != "svg" {
        return Err(BBoxError::NotSvg);
    }
    let mut out = String::from(r#"<svg xmlns="http://www.w3.org/2000/svg""#);
    for a in ["width", "height", "viewBox"] {
        if let Some(v) = root.attribute(a) {
            out += &format!(r#" {a}="{}""#, escape(v));
        }
    }
    out += ">";
    let (mut paths, mut ignored_shapes) = (0, 0);
    for node in root.descendants().filter(|n| n.is_element()) {
        let name = node.tag_name().name();
        let shape = matches!(
            name,
            "rect" | "circle" | "ellipse" | "line" | "polyline" | "polygon"
        );
        if name != "path" && !shape {
            continue;
        }
        let (ctm, hidden) = node_ctm(&node)?;
        if hidden {
            continue;
        }
        if shape {
            ignored_shapes += 1;
            continue;
        }
        let Some(d) = node.attribute("d") else {
            continue;
        };
        out += &format!(r#"<path d="{}""#, escape(d));
        if let Some(id) = node.attribute("id") {
            out += &format!(r#" id="{}""#, escape(id));
        }
        let c = ctm.as_coeffs();
        if c != [1.0, 0.0, 0.0, 1.0, 0.0, 0.0] {
            out += &format!(
                r#" transform="matrix({} {} {} {} {} {})""#,
                c[0], c[1], c[2], c[3], c[4], c[5]
            );
        }
        out += "/>";
        paths += 1;
    }
    out += "</svg>";
    if paths == 0 {
        return Err(BBoxError::Empty);
    }
    Ok(PathsOnly {
        svg: out,
        paths,
        ignored_shapes,
    })
}

#[cfg(test)]
mod tests {
    use super::paths_only;

    #[test]
    fn keeps_only_paths_and_folds_transforms() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:x="urn:x" width="10mm" height="10mm" viewBox="0 0 10 10"><defs><path id="h" d="M0 0L1 1"/></defs><rect width="5" height="5"/><g transform="translate(2 3)"><path id="p" d="M0 0H4V4Z"/></g><circle r="2"/></svg>"#;
        let r = paths_only(svg).unwrap();
        assert_eq!((r.paths, r.ignored_shapes), (1, 2));
        assert!(r.svg.contains(r#"transform="matrix(1 0 0 1 2 3)""#));
        assert!(!r.svg.contains("rect") && !r.svg.contains("urn:x") && !r.svg.contains("\"h\""));
        assert!(paths_only(r#"<svg xmlns="http://www.w3.org/2000/svg"><rect/></svg>"#).is_err());
    }
}
