use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use scraper::{Html, Selector};

use crate::{Result, rustdoc::MethodCounts};

/// Counts stable public methods from a version-matched Rust `rust-docs` archive.
pub fn count_methods_in_html_root(root: &Path) -> Result<MethodCounts> {
    let std_root = root.join("std");
    let all_path = std_root.join("all.html");
    if !all_path.is_file() {
        return Err(format!(
            "rustdoc HTML artifact is missing its std/all.html index: {}",
            all_path.display()
        )
        .into());
    }

    let all_html = fs::read_to_string(&all_path)?;
    let all_document = Html::parse_document(&all_html);
    let link_selector = Selector::parse("a[href]").expect("valid link selector");

    let mut pages = HashSet::new();
    for link in all_document.select(&link_selector) {
        let Some(href) = link.value().attr("href") else {
            continue;
        };
        let relative = PathBuf::from(href);
        if is_public_item_page(&relative) && is_safe_relative_path(&relative) {
            pages.insert(relative);
        }
    }

    let method_link_selector =
        Selector::parse("ul.block.method a[href^=\"#method.\"]").expect("valid method selector");
    let method_section_selector =
        Selector::parse("section.method").expect("valid section selector");
    let since_selector = Selector::parse(".since").expect("valid since selector");

    let mut counts = MethodCounts {
        total_methods: 0,
        const_methods: 0,
    };

    for relative in pages {
        let page = std_root.join(&relative);
        let html = fs::read_to_string(&page)?;
        let document = Html::parse_document(&html);
        let mut sections = HashMap::new();

        for section in document.select(&method_section_selector) {
            let Some(id) = section.value().attr("id") else {
                continue;
            };
            let since = section
                .select(&since_selector)
                .flat_map(|element| element.text())
                .collect::<String>();
            sections.insert(id, since);
        }

        for link in document.select(&method_link_selector) {
            let Some(id) = link
                .value()
                .attr("href")
                .and_then(|href| href.strip_prefix('#'))
            else {
                continue;
            };
            let Some(since) = sections.get(id) else {
                continue;
            };

            // Rustdoc gives stable methods a `.since` badge. Unstable APIs are
            // still present in the generated docs, but have no such badge.
            if since.is_empty() {
                continue;
            }

            counts.total_methods += 1;
            if since.contains("const:") && !since.contains("const: unstable") {
                counts.const_methods += 1;
            }
        }
    }

    Ok(counts)
}

fn is_public_item_page(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };

    ["struct.", "enum.", "union.", "trait.", "primitive."]
        .iter()
        .any(|prefix| name.starts_with(prefix) && name.ends_with(".html"))
}

fn is_safe_relative_path(path: &Path) -> bool {
    path.is_relative()
        && path
            .components()
            .all(|component| !matches!(component, std::path::Component::ParentDir))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn counts_stable_and_const_stable_methods_from_html() {
        let root = std::env::temp_dir().join(format!(
            "rust-constness-html-fixture-{}",
            std::process::id()
        ));
        let std_root = root.join("std");
        fs::create_dir_all(&std_root).unwrap();
        fs::write(
            std_root.join("all.html"),
            r#"<a href="struct.Foo.html">Foo</a><a href="trait.Bar.html">Bar</a>"#,
        )
        .unwrap();
        fs::write(
            std_root.join("struct.Foo.html"),
            r##"
                <ul class="block method">
                  <li><a href="#method.stable">stable</a></li>
                  <li><a href="#method.const">const</a></li>
                  <li><a href="#method.unstable">unstable</a></li>
                </ul>
                <section id="method.stable" class="method"><span class="since">1.0.0</span></section>
                <section id="method.const" class="method"><span class="since">1.0.0 (const: 1.20.0)</span></section>
                <section id="method.unstable" class="method"></section>
            "##,
        )
        .unwrap();
        fs::write(
            std_root.join("trait.Bar.html"),
            r##"
                <ul class="block method"><li><a href="#method.trait_method">trait_method</a></li></ul>
                <section id="method.trait_method" class="method"><span class="since">1.5.0 (const: unstable)</span></section>
            "##,
        )
        .unwrap();

        assert_eq!(
            count_methods_in_html_root(&root).unwrap(),
            MethodCounts {
                total_methods: 3,
                const_methods: 1,
            }
        );

        fs::remove_dir_all(root).unwrap();
    }
}
