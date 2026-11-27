//! Paginate the same sanitized HTML as the Nomad reader. Cutting the Markdown
//! source would change the meaning of fences, references and nested lists.
use dom_query::{Document, NodeId, NodeRef};
use std::{collections::HashSet, ops::Range};

const PART_SIZE: usize = 280;

pub(super) struct Pages {
    document: Document,
    leaves: Vec<(NodeId, Range<usize>)>,
    length: usize,
}

impl Pages {
    pub fn new(markdown: &str) -> Self {
        let document = Document::fragment(super::super::markdown::render(markdown));
        let mut leaves = Vec::new();
        let mut length = 0;
        for leaf in document
            .html_root()
            .descendants_it()
            .filter(|n| n.first_child().is_none())
        {
            let width = if leaf.is_text() {
                let text = leaf.text();
                // The HTML writer inserts a newline after block elements and <br>.
                // It occupies no space in normal flow. Code preserves its whitespace.
                if text.as_ref() == "\n"
                    && leaf.parent().and_then(|p| p.node_name()).as_deref() != Some("code")
                {
                    0
                } else {
                    text.chars().count()
                }
            } else {
                1 // horizontal rules, checkboxes, line breaks and empty cells
            };
            leaves.push((leaf.id, length..length + width));
            length += width;
        }
        Self {
            document,
            leaves,
            length,
        }
    }

    pub fn last_part(&self) -> usize {
        self.length.saturating_sub(1) / PART_SIZE
    }

    pub fn render(self, part: usize) -> String {
        let root = self.document.html_root();
        if self.last_part() == 0 {
            return root.inner_html().to_string();
        }
        let start = part * PART_SIZE;
        let end = start + PART_SIZE;
        let mut keep = HashSet::new();
        for (id, span) in self.leaves {
            if span.is_empty() || span.start >= end || span.end <= start {
                continue;
            }
            let node = NodeRef::new(id, root.tree);
            if node.is_text() && (span.start < start || span.end > end) {
                let text: String = node
                    .text()
                    .chars()
                    .skip(start.saturating_sub(span.start))
                    .take(span.end.min(end) - span.start.max(start))
                    .collect();
                node.set_text(text);
            }
            keep.insert(id);
        }
        let nodes = root.descendants();
        for node in nodes.iter().rev() {
            if keep.contains(&node.id) {
                if let Some(parent) = node.parent() {
                    keep.insert(parent.id);
                }
            }
        }
        for node in nodes {
            if keep.contains(&node.id) {
                if node.node_name().as_deref() == Some("ol") {
                    let skipped = node
                        .children_it(false)
                        .filter(|n| n.node_name().as_deref() == Some("li"))
                        .take_while(|n| !keep.contains(&n.id))
                        .count();
                    if skipped > 0 {
                        let start = node
                            .attr("start")
                            .and_then(|v| v.parse::<u64>().ok())
                            .unwrap_or(1);
                        node.set_attr("start", &(start + skipped as u64).to_string());
                    }
                }
            } else if !matches!(node.node_name().as_deref(), Some("td" | "th")) {
                // Leave empty cells in retained rows to preserve column alignment.
                node.remove_from_parent();
            }
        }
        root.inner_html().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{Pages, PART_SIZE};
    use dom_query::Document;

    #[test]
    fn a_fenced_block_stays_code_across_every_page() {
        let code = "  **原样** <x>\n".repeat(90);
        let source = format!("## 标题\n\n```rust\n{code}```\n");
        let mut actual = String::new();
        for i in 0..=Pages::new(&source).last_part() {
            let page = Document::fragment(Pages::new(&source).render(i));
            assert!(page.select("pre > code.language-rust").exists());
            assert_eq!(page.select("h2").exists(), i == 0);
            assert!(!page.select("strong, x").exists());
            let chunk = page.select("pre code").text();
            assert!(chunk.chars().count() <= PART_SIZE);
            actual.push_str(&chunk);
        }
        assert_eq!(actual, code);
    }

    #[test]
    fn rendered_pages_keep_list_numbers_reference_links_and_table_columns() {
        let source = format!("7. {}\n8. **[指南][ref]**\n\n|甲|乙|\n|---|---|\n|{}|尾列|\n\n[ref]: https://example.org/guide", "字".repeat(PART_SIZE), "字".repeat(PART_SIZE));
        let second = Document::fragment(Pages::new(&source).render(1));
        assert!(second
            .select("ol[start='8'] li strong a[href='https://example.org/guide']")
            .exists());
        let last = Document::fragment(Pages::new(&source).render(2));
        assert_eq!(
            last.select("tbody tr td:nth-child(2)").text().as_ref(),
            "尾列"
        );
    }
}
