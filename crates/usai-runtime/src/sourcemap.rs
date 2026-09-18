//! Source-map-v3 lookup for the application bundle, so a guest stack frame
//! `usai:app:20807:34` reads `src/notes/pg.ts:41:12` where it matters (dev
//! error responses, application-error logs). Not part of the artifact's
//! identity: the same application with or without a map is the same
//! application (like the precompiled image).

use std::sync::Arc;

use serde::Deserialize;

#[derive(Deserialize)]
struct RawMap {
    #[serde(default)]
    sources: Vec<String>,
    #[serde(default)]
    mappings: String,
    #[serde(default, rename = "sourceRoot")]
    source_root: Option<String>,
}

/// One generated position → original position.
#[derive(Clone, Copy, Debug)]
struct Segment {
    generated_column: u32,
    source: u32,
    line: u32,
    column: u32,
}

#[derive(Debug)]
pub struct SourceMap {
    sources: Vec<String>,
    /// Per generated line (0-based), segments sorted by generated column.
    lines: Vec<Vec<Segment>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Position {
    pub source: String,
    /// 1-based.
    pub line: u32,
    /// 1-based.
    pub column: u32,
}

fn vlq(bytes: &[u8], pos: &mut usize) -> Option<i64> {
    const CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result: i64 = 0;
    let mut shift = 0u32;
    loop {
        let c = *bytes.get(*pos)?;
        *pos += 1;
        let digit = CHARS.iter().position(|&x| x == c)? as i64;
        result |= (digit & 31) << shift;
        shift += 5;
        if digit & 32 == 0 {
            break;
        }
        if shift > 35 {
            return None;
        }
    }
    let negative = result & 1 == 1;
    let value = result >> 1;
    Some(if negative { -value } else { value })
}

impl SourceMap {
    pub fn parse(json: &str) -> Option<Arc<SourceMap>> {
        let raw: RawMap = serde_json::from_str(json).ok()?;
        let root = raw.source_root.unwrap_or_default();
        let sources = raw
            .sources
            .into_iter()
            .map(|s| {
                let s = if root.is_empty() {
                    s
                } else {
                    format!("{root}{s}")
                };
                // esbuild writes paths relative to the outfile's directory
                // (`../../src/app.ts`); the leading climbs are noise.
                s.trim_start_matches("./")
                    .trim_start_matches("../")
                    .trim_start_matches("../")
                    .trim_start_matches("../")
                    .to_owned()
            })
            .collect();
        let mut lines = Vec::new();
        let (mut source, mut line, mut column) = (0i64, 0i64, 0i64);
        for generated_line in raw.mappings.split(';') {
            let mut segments = Vec::new();
            let mut generated_column = 0i64;
            for segment in generated_line.split(',') {
                if segment.is_empty() {
                    continue;
                }
                let bytes = segment.as_bytes();
                let mut pos = 0;
                generated_column += vlq(bytes, &mut pos)?;
                if pos < bytes.len() {
                    source += vlq(bytes, &mut pos)?;
                    line += vlq(bytes, &mut pos)?;
                    column += vlq(bytes, &mut pos)?;
                    if pos < bytes.len() {
                        let _name = vlq(bytes, &mut pos)?;
                    }
                    segments.push(Segment {
                        generated_column: generated_column.max(0) as u32,
                        source: source.max(0) as u32,
                        line: line.max(0) as u32,
                        column: column.max(0) as u32,
                    });
                }
            }
            lines.push(segments);
        }
        Some(Arc::new(SourceMap { sources, lines }))
    }

    /// Original position for a 1-based generated line/column.
    pub fn lookup(&self, line: u32, column: u32) -> Option<Position> {
        let segments = self.lines.get(line.checked_sub(1)? as usize)?;
        let target = column.saturating_sub(1);
        let index = segments.partition_point(|s| s.generated_column <= target);
        let segment = segments.get(index.checked_sub(1)?)?;
        Some(Position {
            source: self.sources.get(segment.source as usize)?.clone(),
            line: segment.line + 1,
            column: segment.column + 1,
        })
    }

    /// Rewrites `usai:app:LINE:COL` occurrences in a stack trace.
    pub fn map_stack(&self, stack: &str) -> String {
        let mut out = String::with_capacity(stack.len());
        let mut rest = stack;
        while let Some(idx) = rest.find("usai:app:") {
            out.push_str(&rest[..idx]);
            let after = &rest[idx + "usai:app:".len()..];
            let digits = |s: &str| s.chars().take_while(|c| c.is_ascii_digit()).count();
            let n1 = digits(after);
            let mapped = if n1 > 0 && after[n1..].starts_with(':') {
                let n2 = digits(&after[n1 + 1..]);
                if n2 > 0 {
                    let line: u32 = after[..n1].parse().unwrap_or(0);
                    let col: u32 = after[n1 + 1..n1 + 1 + n2].parse().unwrap_or(0);
                    self.lookup(line, col)
                        .map(|p| (format!("{}:{}:{}", p.source, p.line, p.column), n1 + 1 + n2))
                } else {
                    None
                }
            } else {
                None
            };
            match mapped {
                Some((text, consumed)) => {
                    out.push_str(&text);
                    rest = &after[consumed..];
                }
                None => {
                    out.push_str("usai:app:");
                    rest = after;
                }
            }
        }
        out.push_str(rest);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_esbuild_style_mappings() {
        // Two generated lines; line 2 col 5 maps to src/a.ts line 3 col 2.
        let map = SourceMap::parse(
            r#"{"version":3,"sources":["../src/a.ts"],"mappings":"AAAA;IAEC,CAAC"}"#,
        )
        .unwrap();
        assert_eq!(
            map.lookup(2, 5),
            Some(Position {
                source: "src/a.ts".into(),
                line: 3,
                column: 2
            })
        );
        assert_eq!(
            map.map_stack("at f (usai:app:2:6)\n    at usai:bridge:1:1"),
            "at f (src/a.ts:3:3)\n    at usai:bridge:1:1"
        );
    }
}
