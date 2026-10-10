use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use scraper::{ElementRef, Html, Selector};

use crate::{Result, rustdoc::MethodCounts, versions::release_is_before};

const FIRST_STABLE_CONST_FN_RELEASE: &str = "1.31.0";

/// Counts stable public methods from a version-matched Rust `rust-docs` archive.
pub fn count_methods_in_html_root(root: &Path, release: &str) -> Result<MethodCounts> {
    let std_root = root.join("std");
    let pages = public_item_pages(&std_root)?;
    let stable_const_functions = !release_is_before(release, FIRST_STABLE_CONST_FN_RELEASE)?;

    let mut counts = MethodCounts {
        total_methods: 0,
        const_methods: 0,
    };

    for relative in pages {
        let page = std_root.join(&relative);
        let html = fs::read_to_string(&page)?;
        let document = Html::parse_document(&html);
        for method in declared_methods(&document) {
            if method_is_unstable(&method) {
                continue;
            }

            counts.total_methods += 1;
            if stable_const_functions && method_is_const_stable(&method) {
                counts.const_methods += 1;
            }
        }
    }

    Ok(counts)
}

fn public_item_pages(std_root: &Path) -> Result<HashSet<PathBuf>> {
    let all_path = std_root.join("all.html");
    if all_path.is_file() {
        let all_html = fs::read_to_string(all_path)?;
        let document = Html::parse_document(&all_html);
        let link_selector = Selector::parse("a[href]").expect("valid link selector");
        let pages = document
            .select(&link_selector)
            .filter_map(|link| link.value().attr("href"))
            .map(PathBuf::from)
            .filter(|path| is_public_item_page(path) && is_safe_relative_path(path))
            .collect();
        return Ok(pages);
    }

    if !std_root.join("index.html").is_file() {
        return Err(format!(
            "rustdoc HTML artifact has neither std/all.html nor std/index.html under {}",
            std_root.display()
        )
        .into());
    }

    let mut pages = HashSet::new();
    collect_item_pages(std_root, std_root, &mut pages)?;
    Ok(pages)
}

fn collect_item_pages(root: &Path, directory: &Path, pages: &mut HashSet<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_item_pages(root, &entry.path(), pages)?;
        } else if file_type.is_file() {
            let relative = entry.path().strip_prefix(root)?.to_owned();
            if is_public_item_page(&relative) {
                pages.insert(relative);
            }
        }
    }
    Ok(())
}

fn declared_methods(document: &Html) -> Vec<ElementRef<'_>> {
    let mut in_declared_method_group = false;
    let mut methods = Vec::new();

    for element in document.root_element().descendent_elements() {
        let name = element.value().name();
        let id = element.value().attr("id").unwrap_or_default();

        if name == "h2" {
            if is_declared_method_group(&element, id) {
                in_declared_method_group = true;
            } else if is_method_group_boundary(id) {
                in_declared_method_group = false;
            }
            continue;
        }

        if in_declared_method_group && is_method_element(&element, id) {
            methods.push(element);
        }
    }

    methods
}

fn is_declared_method_group(heading: &ElementRef<'_>, id: &str) -> bool {
    if matches!(id, "methods" | "required-methods" | "provided-methods") {
        return true;
    }
    if id != "implementations" {
        return false;
    }

    heading
        .text()
        .collect::<String>()
        .trim_start()
        .starts_with("Implementations")
}

fn is_method_group_boundary(id: &str) -> bool {
    id.starts_with("deref-methods")
        || matches!(
            id,
            "implementations"
                | "trait-implementations"
                | "synthetic-implementations"
                | "auto-trait-implementations"
                | "blanket-implementations"
                | "implementors"
                | "foreign-impls"
                | "implementations-on-foreign-types"
        )
}

fn is_method_element(element: &ElementRef<'_>, id: &str) -> bool {
    (id.starts_with("method.") || id.starts_with("tymethod."))
        && element
            .value()
            .classes()
            .any(|class_name| class_name == "method")
}

fn method_is_unstable(method: &ElementRef<'_>) -> bool {
    element_has_unstable_marker(method)
        || adjacent_item_metadata(method)
            .is_some_and(|metadata| element_has_unstable_marker(&metadata))
}

fn element_has_unstable_marker(element: &ElementRef<'_>) -> bool {
    is_unstable_marker(element)
        || element
            .descendent_elements()
            .any(|descendant| is_unstable_marker(&descendant))
}

fn is_unstable_marker(element: &ElementRef<'_>) -> bool {
    let classes = element.value().classes().collect::<Vec<_>>();
    classes
        .iter()
        .any(|class_name| class_name.eq_ignore_ascii_case("unstable"))
        && classes
            .iter()
            .any(|class_name| matches!(*class_name, "stab" | "stability"))
}

fn adjacent_item_metadata<'a>(method: &ElementRef<'a>) -> Option<ElementRef<'a>> {
    next_sibling_element(method)
        // Rustdoc 1.54 through 1.77 places trait method metadata after the
        // enclosing <summary> instead of directly after the method section.
        .or_else(|| {
            ElementRef::wrap(method.parent()?)
                .filter(|parent| parent.value().name() == "summary")
                .and_then(|summary| next_sibling_element(&summary))
        })
        .filter(|element| {
            element
                .value()
                .classes()
                .any(|class_name| matches!(class_name, "item-info" | "stability"))
        })
}

fn next_sibling_element<'a>(element: &ElementRef<'a>) -> Option<ElementRef<'a>> {
    element.next_siblings().filter_map(ElementRef::wrap).next()
}

fn method_is_const_stable(method: &ElementRef<'_>) -> bool {
    let metadata = method_metadata_text(method).to_ascii_lowercase();
    if metadata.contains("const: unstable") || metadata.contains("const unstable") {
        return false;
    }

    metadata.contains("const:")
        || metadata.contains("const since")
        || method_signature(method).is_some_and(|signature| signature_is_const(&signature))
}

fn method_metadata_text(method: &ElementRef<'_>) -> String {
    let since_selector = Selector::parse(".since").expect("valid since selector");
    let mut metadata = String::new();
    for badge in method.select(&since_selector) {
        metadata.extend(badge.text());
        if let Some(title) = badge.value().attr("title") {
            metadata.push(' ');
            metadata.push_str(title);
        }
    }
    if let Some(item_metadata) = adjacent_item_metadata(method) {
        metadata.extend(item_metadata.text());
    }
    metadata
}

fn method_signature(method: &ElementRef<'_>) -> Option<String> {
    let id = method.value().attr("id")?;
    let link_selector = Selector::parse("a[href]").expect("valid method link selector");
    let signature_selector =
        Selector::parse("code, .code-header").expect("valid signature selector");
    let expected_href = format!("#{id}");

    for signature in method.select(&signature_selector) {
        if signature
            .select(&link_selector)
            .any(|link| link.value().attr("href") == Some(expected_href.as_str()))
        {
            return Some(signature.text().collect::<Vec<_>>().join(" "));
        }
    }
    None
}

fn signature_is_const(signature: &str) -> bool {
    let mut saw_const = false;
    for token in signature.split_whitespace() {
        let token = token.trim_matches(|character: char| !character.is_ascii_alphabetic());
        match token {
            "const" => saw_const = true,
            "fn" => return saw_const,
            _ => {}
        }
    }
    false
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
                <h2 id="implementations">Implementations</h2>
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
            count_methods_in_html_root(&root, "1.97.0").unwrap(),
            MethodCounts {
                total_methods: 3,
                const_methods: 1,
            }
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn supports_each_historical_rustdoc_markup_generation() {
        let generations = [
            (
                "1.0.0",
                include_str!("../tests/fixtures/rustdoc/1.0.html"),
                2,
                0,
            ),
            (
                "1.25.0",
                include_str!("../tests/fixtures/rustdoc/1.25.html"),
                1,
                0,
            ),
            (
                "1.40.0",
                include_str!("../tests/fixtures/rustdoc/1.40.html"),
                2,
                1,
            ),
            (
                "1.50.0",
                include_str!("../tests/fixtures/rustdoc/1.50.html"),
                2,
                1,
            ),
            (
                "1.55.0",
                include_str!("../tests/fixtures/rustdoc/1.55.html"),
                2,
                1,
            ),
            (
                "1.60.0",
                include_str!("../tests/fixtures/rustdoc/1.60.html"),
                2,
                1,
            ),
            (
                "1.70.0",
                include_str!("../tests/fixtures/rustdoc/1.70.html"),
                2,
                1,
            ),
            (
                "1.75.0",
                include_str!("../tests/fixtures/rustdoc/1.75.html"),
                2,
                1,
            ),
        ];

        for (release, html, total_methods, const_methods) in generations {
            let root = fixture_root(release, html, true);
            let counts = count_methods_in_html_root(&root, release).unwrap();
            assert_eq!(counts.total_methods, total_methods, "Rust {release}");
            assert_eq!(counts.const_methods, const_methods, "Rust {release}");
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn counts_required_and_provided_trait_methods_without_implementor_copies() {
        let root = fixture_root(
            "trait-methods",
            include_str!("../tests/fixtures/rustdoc/trait-methods.html"),
            true,
        );

        assert_eq!(
            count_methods_in_html_root(&root, "1.97.0").unwrap(),
            MethodCounts {
                total_methods: 2,
                const_methods: 1,
            }
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn excludes_unstable_trait_methods_whose_badge_follows_the_summary() {
        let root = fixture_root(
            "1.77-trait-methods",
            include_str!("../tests/fixtures/rustdoc/1.77-trait-methods.html"),
            true,
        );

        assert_eq!(
            count_methods_in_html_root(&root, "1.77.2").unwrap(),
            MethodCounts {
                total_methods: 2,
                const_methods: 0,
            }
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn discovers_item_pages_before_all_html_existed() {
        let root = fixture_root(
            "pre-all-index",
            include_str!("../tests/fixtures/rustdoc/1.25.html"),
            false,
        );

        assert_eq!(
            count_methods_in_html_root(&root, "1.25.0")
                .unwrap()
                .total_methods,
            1
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recognizes_const_signatures_before_const_badges_existed() {
        let document = Html::parse_document(include_str!("../tests/fixtures/rustdoc/1.40.html"));
        let const_method = declared_methods(&document)
            .into_iter()
            .find(|method| method.value().attr("id") == Some("method.const_stable"))
            .unwrap();

        let signature = method_signature(&const_method).unwrap();
        assert!(signature_is_const(&signature));
        assert!(method_is_const_stable(&const_method));
    }

    fn fixture_root(name: &str, item_html: &str, include_all_index: bool) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("rust-constness-html-{name}-{}", std::process::id()));
        let std_root = root.join("std");
        fs::create_dir_all(&std_root).unwrap();
        fs::write(std_root.join("index.html"), "std").unwrap();
        if include_all_index {
            fs::write(
                std_root.join("all.html"),
                r#"<a href="struct.Foo.html">Foo</a>"#,
            )
            .unwrap();
        }
        fs::write(std_root.join("struct.Foo.html"), item_html).unwrap();
        root
    }
}
