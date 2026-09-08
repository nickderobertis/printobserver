//! The transitive closure of the types a method's parameters reach.
//!
//! The printer port's contract is that no method admits a byte sequence at all,
//! neither as a parameter's own type nor anywhere in the closure reachable from
//! it, and that the only string in that closure is the one `FileName` wraps.
//! That is a claim about a closure rather than about a signature, so this walks
//! the closure: it follows each declared field and each variant payload in turn
//! through the declarations it can resolve.

use std::collections::BTreeSet;

use crate::surface::{Field, Variant, enum_variants, struct_fields};

/// The types the walk resolves names in.
pub struct Universe {
    /// The parsed sources names are resolved against.
    sources: Vec<syn::File>,
}

/// The declaration one name resolves to.
enum Declaration {
    /// A struct, and its fields.
    Fields(Vec<Field>),
    /// An enum, and its variants' fields.
    Variants(Vec<Variant>),
}

/// Types the walk treats as leaves rather than resolving.
const PRIMITIVES: [&str; 17] = [
    "bool", "char", "f32", "f64", "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32",
    "u64", "u128", "usize", "PathBuf",
];

/// Container spellings the walk sees through rather than resolving.
const CONTAINERS: [&str; 8] =
    ["Option", "Vec", "Box", "Arc", "Rc", "BTreeMap", "HashMap", "Result"];

/// What the walk found in a closure.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Findings {
    /// Where a string is reachable, as the path the walk reached it by.
    pub strings: Vec<String>,
    /// Where a byte sequence is reachable, as the path the walk reached it by.
    pub bytes: Vec<String>,
    /// Every named type the closure reaches.
    pub reached: BTreeSet<String>,
}

impl Universe {
    /// Resolve names against these sources.
    #[must_use]
    pub const fn new(sources: Vec<syn::File>) -> Self {
        Self { sources }
    }

    /// The declaration one name resolves to, if these sources declare it.
    fn resolve(&self, name: &str) -> Option<Declaration> {
        for source in &self.sources {
            let fields = struct_fields(source, name);
            if !fields.is_empty() {
                return Some(Declaration::Fields(fields));
            }
            let variants = enum_variants(source, name);
            if !variants.is_empty() {
                return Some(Declaration::Variants(variants));
            }
        }
        None
    }

    /// Walk one type spelling, recording what it reaches.
    fn walk(&self, path: &str, spelling: &str, findings: &mut Findings, seen: &mut BTreeSet<String>) {
        if spelling.contains("Vec<u8>") || spelling.contains("[u8]") || spelling.contains("RawBytes")
        {
            findings.bytes.push(path.to_owned());
        }
        for name in identifiers(spelling) {
            if name == "String" || name == "str" {
                findings.strings.push(path.to_owned());
                continue;
            }
            if PRIMITIVES.contains(&name.as_str()) || CONTAINERS.contains(&name.as_str()) {
                continue;
            }
            findings.reached.insert(name.clone());
            if !seen.insert(name.clone()) {
                continue;
            }
            match self.resolve(&name) {
                Some(Declaration::Fields(fields)) => {
                    for field in fields {
                        self.walk(&format!("{path}/{name}.{}", field.name), &field.ty, findings, seen);
                    }
                }
                Some(Declaration::Variants(variants)) => {
                    for variant in variants {
                        for field in variant.fields {
                            self.walk(
                                &format!("{path}/{name}::{}.{}", variant.name, field.name),
                                &field.ty,
                                findings,
                                seen,
                            );
                        }
                    }
                }
                None => {}
            }
        }
    }

    /// Walk every one of these type spellings, under the paths given for them.
    #[must_use]
    pub fn closure(&self, roots: &[(String, String)]) -> Findings {
        let mut findings = Findings::default();
        let mut seen = BTreeSet::new();
        for (path, spelling) in roots {
            self.walk(path, spelling, &mut findings, &mut seen);
        }
        findings
    }
}

/// The identifiers one type spelling names, in order.
fn identifiers(spelling: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut current = String::new();
    for character in spelling.chars() {
        if character.is_alphanumeric() || character == '_' {
            current.push(character);
        } else if !current.is_empty() {
            names.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        names.push(current);
    }
    names
}
