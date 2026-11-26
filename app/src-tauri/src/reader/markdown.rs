use pulldown_cmark::{html, Event, LinkType, Options, Parser, Tag, TagEnd};

fn readable_link(kind: LinkType, url: &str) -> bool {
    if kind == LinkType::Email {
        return true;
    }
    let Some((scheme, rest)) = url.split_once(':') else {
        return false;
    };
    scheme.eq_ignore_ascii_case("mailto")
        || ((scheme.eq_ignore_ascii_case("https") || scheme.eq_ignore_ascii_case("http"))
            && rest.starts_with("//"))
}

pub(super) fn render(text: &str) -> String {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut links = Vec::new();
    let events = Parser::new_ext(text, options).filter_map(|event| {
        Some(match event {
            Event::Html(raw) | Event::InlineHtml(raw) => Event::Text(raw),
            Event::Start(Tag::HtmlBlock) => Event::Start(Tag::Paragraph),
            Event::End(TagEnd::HtmlBlock) => Event::End(TagEnd::Paragraph),
            Event::SoftBreak => Event::HardBreak,
            // Display image descriptions as links so reading a conversation never fetches its attachments.
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            })
            | Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                let active = links.is_empty() && readable_link(link_type, &dest_url);
                links.push(active);
                if !active {
                    return None;
                }
                Event::Start(Tag::Link {
                    link_type,
                    dest_url,
                    title,
                    id,
                })
            }
            Event::End(TagEnd::Link | TagEnd::Image) => {
                if links.pop() != Some(true) {
                    return None;
                }
                Event::End(TagEnd::Link)
            }
            other => other,
        })
    });
    let mut result = String::with_capacity(text.len());
    html::push_html(&mut result, events);
    result
}

#[cfg(test)]
mod tests {
    use super::render;

    #[test]
    fn renders_chat_markdown_and_keeps_original_line_breaks() {
        let output = render("## 标题\n\n正文 **强调** 与 `内联`\n下一行\n\n> 引文\n\n1. 一\n   - 二\n\n|项目|状态|\n|---|---|\n|钳钳|完成|\n\n```rust\n  x < y\n```\n\n- [x] 好了");
        for part in [
            "<h2>标题</h2>",
            "<strong>强调</strong>",
            "<code>内联</code>",
            "<br />\n下一行",
            "<blockquote>",
            "<ol>",
            "<ul>",
            "<table>",
            "<td>完成</td>",
            "  x &lt; y\n</code>",
            "checkbox",
        ] {
            assert!(output.contains(part), "{part} missing from {output}");
        }
    }

    #[test]
    fn quotes_raw_html_and_only_emits_readable_links() {
        let output = render("<div>原文</div>\n\n文字 <b>粗体标记</b> [帮助](https://example.org/help) [文件](file:///tmp/note) ![配图](https://example.org/image.png)");
        assert!(output.contains("&lt;div&gt;原文&lt;/div&gt;"));
        assert!(output.contains("&lt;b&gt;粗体标记&lt;/b&gt;"));
        assert!(output.contains("<a href=\"https://example.org/help\">帮助</a>"));
        assert!(output.contains("<a href=\"https://example.org/image.png\">配图</a>"));
        assert!(output.contains("文件"));
        assert!(!output.contains("file:") && !output.contains("<img"));
    }
}
