use std::collections::HashMap;

use serde_json::Value;

use crate::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MethodCounts {
    pub total_methods: usize,
    pub const_methods: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MethodOwner {
    InherentImpl,
    PublicTrait(String),
}

/// Counts stable public methods and associated functions in a rustdoc JSON document.
///
/// Methods declared by traits are counted once. Methods supplied by trait impls,
/// blanket impls, synthetic impls, and deref expansion are not counted separately.
pub fn count_methods(document: &Value) -> Result<MethodCounts> {
    let index = document
        .get("index")
        .and_then(Value::as_object)
        .ok_or("rustdoc JSON is missing an object-valued index")?;

    let mut owners = HashMap::new();
    let mut trait_stability = HashMap::new();

    for (id, item) in index {
        let Some(inner) = item.get("inner").and_then(Value::as_object) else {
            continue;
        };

        if let Some(implementation) = inner.get("impl").and_then(Value::as_object) {
            let is_inherent = implementation.get("trait").is_none_or(Value::is_null)
                && implementation
                    .get("blanket_impl")
                    .is_none_or(Value::is_null)
                && !implementation
                    .get("synthetic")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);

            if !is_inherent {
                continue;
            }

            if let Some(items) = implementation.get("items").and_then(Value::as_array) {
                for item_id in items.iter().filter_map(id_from_value) {
                    owners.insert(item_id, MethodOwner::InherentImpl);
                }
            }
        }

        if let Some(trait_definition) = inner.get("trait").and_then(Value::as_object) {
            let is_public = item
                .get("visibility")
                .and_then(Value::as_str)
                .is_some_and(|visibility| visibility == "public");

            if !is_public {
                continue;
            }

            trait_stability.insert(id.clone(), is_stable(item.get("stability")));

            if let Some(items) = trait_definition.get("items").and_then(Value::as_array) {
                for item_id in items.iter().filter_map(id_from_value) {
                    owners.insert(item_id, MethodOwner::PublicTrait(id.clone()));
                }
            }
        }
    }

    let mut total_methods = 0;
    let mut const_methods = 0;

    for (id, owner) in owners {
        let Some(item) = index.get(&id) else {
            continue;
        };
        let Some(inner) = item.get("inner").and_then(Value::as_object) else {
            continue;
        };
        let Some(function) = inner.get("function").or_else(|| inner.get("method")) else {
            continue;
        };

        let trait_id = match &owner {
            MethodOwner::InherentImpl => None,
            MethodOwner::PublicTrait(trait_id) => Some(trait_id),
        };

        let is_public = match &owner {
            MethodOwner::InherentImpl => item
                .get("visibility")
                .and_then(Value::as_str)
                .is_some_and(|visibility| visibility == "public"),
            MethodOwner::PublicTrait(_) => true,
        };

        if !is_public || !is_stable(item.get("stability")) {
            continue;
        }

        // A trait definition can be public but unstable. The method itself may
        // omit a stability field and inherit the trait's stability.
        if trait_id.is_some_and(|trait_id| !trait_stability.get(trait_id).copied().unwrap_or(true))
        {
            continue;
        }

        total_methods += 1;
        if is_const(function) && is_stable(item.get("const_stability")) {
            const_methods += 1;
        }
    }

    Ok(MethodCounts {
        total_methods,
        const_methods,
    })
}

fn id_from_value(value: &Value) -> Option<String> {
    match value {
        Value::String(id) => Some(id.clone()),
        Value::Number(id) => Some(id.to_string()),
        _ => None,
    }
}

fn is_const(function: &Value) -> bool {
    match function.get("header") {
        Some(Value::Object(header)) => header
            .get("is_const")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        Some(Value::String(header)) => header.split_whitespace().any(|part| part == "const"),
        _ => false,
    }
}

fn is_stable(value: Option<&Value>) -> bool {
    let Some(value) = value else {
        return true;
    };

    match value {
        Value::Null => true,
        Value::String(level) => level != "unstable",
        Value::Object(object) => {
            if object.contains_key("unstable") {
                return false;
            }
            if object.contains_key("stable") {
                return true;
            }
            object
                .get("level")
                .or_else(|| object.get("stability"))
                .is_none_or(|level| is_stable(Some(level)))
        }
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn counts_inherent_and_trait_methods_without_trait_impl_duplicates() {
        let document = json!({
            "index": {
                "trait": {
                    "visibility": "public",
                    "stability": {"level": "stable"},
                    "inner": {"trait": {"items": ["trait_method"]}}
                },
                "trait_method": {
                    "visibility": "default",
                    "stability": null,
                    "const_stability": {"level": "stable"},
                    "inner": {"function": {"header": {"is_const": true}}}
                },
                "impl": {
                    "visibility": "default",
                    "inner": {"impl": {"trait": null, "blanket_impl": null, "items": ["inherent_method", "unstable_const"]}}
                },
                "inherent_method": {
                    "visibility": "public",
                    "stability": {"level": "stable"},
                    "const_stability": null,
                    "inner": {"function": {"header": ""}}
                },
                "unstable_const": {
                    "visibility": "public",
                    "stability": {"level": "stable"},
                    "const_stability": {"level": "unstable"},
                    "inner": {"function": {"header": {"is_const": true}}}
                },
                "trait_impl": {
                    "visibility": "default",
                    "inner": {"impl": {"trait": {"path": "PublicTrait"}, "blanket_impl": null, "items": ["trait_impl_method"]}}
                },
                "trait_impl_method": {
                    "visibility": "public",
                    "stability": {"level": "stable"},
                    "inner": {"function": {"header": ""}}
                }
            }
        });

        assert_eq!(
            count_methods(&document).unwrap(),
            MethodCounts {
                total_methods: 3,
                const_methods: 1,
            }
        );
    }

    #[test]
    fn accepts_legacy_const_header_strings() {
        let document = json!({
            "index": {
                "implementation": {
                    "inner": {"impl": {"trait": null, "blanket_impl": null, "items": ["method"]}}
                },
                "method": {
                    "visibility": "public",
                    "inner": {"method": {"header": "const unsafe"}}
                }
            }
        });

        assert_eq!(count_methods(&document).unwrap().const_methods, 1);
    }
}
