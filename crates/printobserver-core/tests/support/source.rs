//! Reading this crate's own declarations, for the structural checks.
//!
//! Three of the properties this crate is held to are claims about
//! **declarations** rather than about values — which function issues an action,
//! which module can name the printer handle, what a port method's parameters
//! reach — so they are read off the sources themselves rather than off a table
//! somebody maintains beside them. Every fixture they refuse is a source
//! snippet read by this same reader, so what refuses a fixture is what reads
//! the committed crate.

use std::collections::BTreeSet;
use std::path::PathBuf;

use quote::ToTokens;

/// One module of a tree this reader is driven against: its name and its source.
pub type Module = (String, String);

/// The path of one crate of this workspace.
#[must_use]
pub fn crate_dir(crate_name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(crate_name)
}

/// Read one file, naming it when it cannot be read.
///
/// # Panics
///
/// Panics when the file cannot be read, which is a broken checkout.
#[must_use]
pub fn read(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

/// Parse one Rust source.
///
/// # Panics
///
/// Panics when the source does not parse, which is what a broken fixture is.
#[must_use]
pub fn parse(source: &str) -> syn::File {
    syn::parse_file(source).expect("the source parses as Rust")
}

/// Every module under one crate's `src`, by file stem, sorted by name.
///
/// # Panics
///
/// Panics when the directory cannot be read.
#[must_use]
pub fn crate_modules(crate_name: &str) -> Vec<Module> {
    let dir = crate_dir(crate_name).join("src");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("read {}: {error}", dir.display()))
        .map(|entry| entry.expect("a readable directory entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let name = path
                .file_stem()
                .expect("a file stem")
                .to_string_lossy()
                .into_owned();
            (name, read(&path))
        })
        .collect()
}

/// Every `.rs` file under one crate's `tests`, by path, sorted.
///
/// # Panics
///
/// Panics when the directory cannot be walked.
#[must_use]
pub fn crate_test_sources(crate_name: &str) -> Vec<Module> {
    let dir = crate_dir(crate_name).join("tests");
    let mut found = Vec::new();
    walk_rust_files(&dir, &mut found);
    found.sort_by(|left, right| left.0.cmp(&right.0));
    found
}

/// Collect every `.rs` file under one directory, recursively.
fn walk_rust_files(dir: &std::path::Path, found: &mut Vec<Module>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = entry.expect("a readable directory entry").path();
        if path.is_dir() {
            walk_rust_files(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push((path.to_string_lossy().into_owned(), read(&path)));
        }
    }
}

/// One source's tokens with every string literal removed.
///
/// A doc comment is a `#[doc = "..."]` attribute by the time a source is
/// tokens, so its prose would otherwise read as code to a reference walk.
#[must_use]
pub fn tokens_without_literals(source: &str) -> String {
    strip_literals(&parse(source).to_token_stream().to_string())
}

/// One token string with every string literal removed.
#[must_use]
pub fn strip_literals(rendered: &str) -> String {
    let mut kept = String::new();
    let mut inside = false;
    let mut escaped = false;
    for character in rendered.chars() {
        if inside {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                inside = false;
            }
            continue;
        }
        if character == '"' {
            inside = true;
            continue;
        }
        kept.push(character);
    }
    kept
}

/// One token string as words, with every delimiter its own word.
#[must_use]
pub fn words(tokens: &str) -> Vec<String> {
    let mut spaced = tokens.to_owned();
    for delimiter in ['(', ')', '[', ']', '{', '}', ',', ';', '<', '>', '&', '#', '!'] {
        spaced = spaced.replace(delimiter, &format!(" {delimiter} "));
    }
    spaced.split_whitespace().map(str::to_owned).collect()
}

/// Every syntactic reference one token string makes to one of these names.
///
/// A **reference** rather than a call: a path expression taken as a value, a
/// function pointer and an alias each have to name the method at the point they
/// are made, and each is exactly a name preceded by `.` or `::`. Enumerating
/// references rather than call sites is what reaches the alias and the function
/// pointer, neither of which a call-site walk could see.
#[must_use]
pub fn references(tokens: &str, names: &[String]) -> Vec<String> {
    let words = words(tokens);
    let mut found = Vec::new();
    for pair in words.windows(2) {
        if (pair[0] == "." || pair[0] == "::") && names.contains(&pair[1]) {
            found.push(pair[1].clone());
        }
    }
    found
}

/// How many times one identifier appears as a word in a token string.
#[must_use]
pub fn mentions(tokens: &str, identifier: &str) -> usize {
    words(tokens)
        .iter()
        .filter(|word| *word == identifier)
        .count()
}

/// A type's whitespace-free spelling, which is what a comparison uses.
fn spelling<T: ToTokens>(node: &T) -> String {
    node.to_token_stream().to_string().replace([' ', '\n'], "")
}

/// A type's tokens with their own spacing, which is what a word walk reads.
///
/// The whitespace-free spelling runs `dyn PrinterPort` together into one word,
/// so a walk for the identifier would not find it there.
fn type_tokens<T: ToTokens>(node: &T) -> String {
    node.to_token_stream().to_string()
}

/// The methods one trait declares, in declaration order.
#[must_use]
pub fn trait_method_names(file: &syn::File, trait_name: &str) -> Vec<String> {
    let mut names = Vec::new();
    for item in &file.items {
        let syn::Item::Trait(declaration) = item else {
            continue;
        };
        if declaration.ident != trait_name {
            continue;
        }
        for entry in &declaration.items {
            if let syn::TraitItem::Fn(function) = entry {
                names.push(function.sig.ident.to_string());
            }
        }
    }
    names
}

/// The parameters of one trait's methods: the method, the name and the type.
#[must_use]
pub fn trait_method_params(file: &syn::File, trait_name: &str) -> Vec<(String, String, String)> {
    let mut found = Vec::new();
    for item in &file.items {
        let syn::Item::Trait(declaration) = item else {
            continue;
        };
        if declaration.ident != trait_name {
            continue;
        }
        for entry in &declaration.items {
            let syn::TraitItem::Fn(function) = entry else {
                continue;
            };
            for input in &function.sig.inputs {
                if let syn::FnArg::Typed(typed) = input {
                    found.push((
                        function.sig.ident.to_string(),
                        spelling(&typed.pat),
                        spelling(&typed.ty),
                    ));
                }
            }
        }
    }
    found
}

/// The variants one enum declares, in declaration order.
#[must_use]
pub fn enum_variant_names(file: &syn::File, enum_name: &str) -> Vec<String> {
    let mut names = Vec::new();
    for item in &file.items {
        let syn::Item::Enum(declaration) = item else {
            continue;
        };
        if declaration.ident != enum_name {
            continue;
        }
        for variant in &declaration.variants {
            names.push(variant.ident.to_string());
        }
    }
    names
}

/// How many `impl <trait> for` blocks one source declares.
#[must_use]
pub fn impls_of_trait(file: &syn::File, trait_name: &str) -> usize {
    file.items
        .iter()
        .filter(|item| match item {
            syn::Item::Impl(block) => block.trait_.as_ref().is_some_and(|(path, _)| {
                path.segments
                    .last()
                    .is_some_and(|segment| segment.ident == trait_name)
            }),
            _ => false,
        })
        .count()
}

/// Every function one source declares, wherever it is declared.
///
/// Free functions, associated functions, methods of an `impl` block and
/// methods with a default body in a trait are all functions, and each is
/// enumerated here exactly as the others are.
#[must_use]
pub fn functions(file: &syn::File) -> Vec<(String, String)> {
    let mut found = Vec::new();
    collect_functions(&file.items, &mut found);
    found
}

/// Collect every function declared among these items.
fn collect_functions(items: &[syn::Item], found: &mut Vec<(String, String)>) {
    for item in items {
        match item {
            syn::Item::Fn(function) => {
                found.push((
                    function.sig.ident.to_string(),
                    function.to_token_stream().to_string(),
                ));
            }
            syn::Item::Impl(block) => {
                for entry in &block.items {
                    if let syn::ImplItem::Fn(function) = entry {
                        found.push((
                            function.sig.ident.to_string(),
                            function.to_token_stream().to_string(),
                        ));
                    }
                }
            }
            syn::Item::Trait(declaration) => {
                for entry in &declaration.items {
                    if let syn::TraitItem::Fn(function) = entry
                        && function.default.is_some()
                    {
                        found.push((
                            function.sig.ident.to_string(),
                            function.to_token_stream().to_string(),
                        ));
                    }
                }
            }
            syn::Item::Mod(module) => {
                if let Some((_, items)) = &module.content {
                    collect_functions(items, found);
                }
            }
            _ => {}
        }
    }
}

/// Every struct field whose type mentions one identifier: struct, field, public.
#[must_use]
pub fn fields_mentioning(file: &syn::File, identifier: &str) -> Vec<(String, String, bool)> {
    let mut found = Vec::new();
    for item in &file.items {
        let syn::Item::Struct(declaration) = item else {
            continue;
        };
        for field in &declaration.fields {
            if mentions(&type_tokens(&field.ty), identifier) == 0 {
                continue;
            }
            found.push((
                declaration.ident.to_string(),
                field
                    .ident
                    .as_ref()
                    .map_or_else(|| "0".to_owned(), ToString::to_string),
                matches!(field.vis, syn::Visibility::Public(_)),
            ));
        }
    }
    found
}

/// Every function whose **return type** mentions one identifier.
#[must_use]
pub fn returns_mentioning(file: &syn::File, identifier: &str) -> Vec<String> {
    let mut found = Vec::new();
    collect_signatures(&file.items, &mut found);
    found
        .into_iter()
        .filter_map(|(name, returns, _)| (mentions(&returns, identifier) > 0).then_some(name))
        .collect()
}

/// Every function that takes a parameter whose type mentions one identifier.
#[must_use]
pub fn params_mentioning(file: &syn::File, identifier: &str) -> Vec<String> {
    let mut found = Vec::new();
    collect_signatures(&file.items, &mut found);
    found
        .into_iter()
        .filter_map(|(name, _, params)| {
            params
                .iter()
                .any(|param| mentions(param, identifier) > 0)
                .then_some(name)
        })
        .collect()
}

/// Every function signature among these items: the name, its return, its params.
fn collect_signatures(items: &[syn::Item], found: &mut Vec<(String, String, Vec<String>)>) {
    let mut push = |signature: &syn::Signature| {
        let returns = match &signature.output {
            syn::ReturnType::Default => "()".to_owned(),
            syn::ReturnType::Type(_, ty) => type_tokens(ty),
        };
        let params = signature
            .inputs
            .iter()
            .filter_map(|input| match input {
                syn::FnArg::Typed(typed) => Some(type_tokens(&typed.ty)),
                syn::FnArg::Receiver(_) => None,
            })
            .collect();
        found.push((signature.ident.to_string(), returns, params));
    };
    for item in items {
        match item {
            syn::Item::Fn(function) => push(&function.sig),
            syn::Item::Impl(block) => {
                for entry in &block.items {
                    if let syn::ImplItem::Fn(function) = entry {
                        push(&function.sig);
                    }
                }
            }
            syn::Item::Trait(declaration) => {
                for entry in &declaration.items {
                    if let syn::TraitItem::Fn(function) = entry {
                        push(&function.sig);
                    }
                }
            }
            _ => {}
        }
    }
}

/// The struct fields and enum-variant fields one universe of sources declares.
pub struct Universe {
    /// The sources names are resolved against.
    sources: Vec<syn::File>,
}

/// What a walk over a closure of types found.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Findings {
    /// Every path at which a string is reachable.
    pub strings: Vec<String>,
    /// Every path at which a byte sequence is reachable.
    pub bytes: Vec<String>,
    /// Every named type the closure reaches.
    pub reached: BTreeSet<String>,
}

/// Types the walk treats as leaves rather than resolving.
const PRIMITIVES: [&str; 18] = [
    "bool", "char", "f32", "f64", "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32",
    "u64", "u128", "usize", "PathBuf", "Timestamp",
];

/// Container spellings the walk sees through rather than resolving.
const CONTAINERS: [&str; 8] = [
    "Option", "Vec", "Box", "Arc", "Rc", "BTreeMap", "HashMap", "Result",
];

impl Universe {
    /// Resolve names against these sources.
    #[must_use]
    pub const fn new(sources: Vec<syn::File>) -> Self {
        Self { sources }
    }

    /// The fields one struct name resolves to, if these sources declare it.
    fn struct_fields(&self, name: &str) -> Option<Vec<(String, String)>> {
        for source in &self.sources {
            for item in &source.items {
                let syn::Item::Struct(declaration) = item else {
                    continue;
                };
                if declaration.ident == name {
                    return Some(
                        declaration
                            .fields
                            .iter()
                            .enumerate()
                            .map(|(index, field)| {
                                (
                                    field
                                        .ident
                                        .as_ref()
                                        .map_or_else(|| index.to_string(), ToString::to_string),
                                    spelling(&field.ty),
                                )
                            })
                            .collect(),
                    );
                }
            }
        }
        None
    }

    /// The variant payloads one enum name resolves to, if it is declared.
    fn enum_fields(&self, name: &str) -> Option<Vec<(String, String)>> {
        for source in &self.sources {
            for item in &source.items {
                let syn::Item::Enum(declaration) = item else {
                    continue;
                };
                if declaration.ident == name {
                    let mut fields = Vec::new();
                    for variant in &declaration.variants {
                        for (index, field) in variant.fields.iter().enumerate() {
                            fields.push((
                                format!(
                                    "{}::{}",
                                    variant.ident,
                                    field
                                        .ident
                                        .as_ref()
                                        .map_or_else(|| index.to_string(), ToString::to_string)
                                ),
                                spelling(&field.ty),
                            ));
                        }
                    }
                    return Some(fields);
                }
            }
        }
        None
    }

    /// Walk one type spelling, recording what it reaches.
    fn walk(
        &self,
        path: &str,
        spelling: &str,
        findings: &mut Findings,
        seen: &mut BTreeSet<String>,
    ) {
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
            let resolved = self
                .struct_fields(&name)
                .or_else(|| self.enum_fields(&name));
            if let Some(fields) = resolved {
                for (field, ty) in fields {
                    self.walk(&format!("{path}/{name}.{field}"), &ty, findings, seen);
                }
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
            names.push(core::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        names.push(current);
    }
    names
}

/// Every place a tree references an action method outside its one chokepoint.
///
/// Empty means the tree holds the property: every syntactic reference to an
/// action method of the printer port — a call, a path taken as a value, a
/// function pointer, an alias — lies inside the one named function, and exactly
/// one function claims that role.
#[must_use]
pub fn chokepoint_findings(
    modules: &[Module],
    chokepoint: &str,
    action_methods: &[String],
) -> Vec<String> {
    let mut findings = Vec::new();
    let mut claimed: Vec<(String, String)> = Vec::new();
    for (name, source) in modules {
        for (function, tokens) in functions(&parse(source)) {
            if function == chokepoint {
                claimed.push((name.clone(), strip_literals(&tokens)));
            }
        }
    }
    if claimed.len() != 1 {
        findings.push(format!(
            "{} functions claim the chokepoint role `{chokepoint}`, and exactly one may",
            claimed.len()
        ));
    }
    let inside: usize = claimed
        .iter()
        .map(|(_, tokens)| references(tokens, action_methods).len())
        .sum();
    let mut total = 0;
    for (name, source) in modules {
        let found = references(&tokens_without_literals(source), action_methods);
        let here = found.len();
        total += here;
        let owned = claimed
            .iter()
            .filter(|(module, _)| module == name)
            .map(|(_, tokens)| references(tokens, action_methods).len())
            .sum::<usize>();
        if here > owned {
            findings.push(format!(
                "`{name}` references {:?} outside the chokepoint",
                found
            ));
        }
    }
    if total != inside {
        findings.push(format!(
            "{total} action-method references in the tree and {inside} inside the chokepoint"
        ));
    }
    findings
}

/// Every place a tree lets the printer handle out of the chokepoint's module.
///
/// Empty means the tree holds the property: the handle is declared once, as a
/// private field of that one module, the identifier naming its trait appears
/// nowhere else in the crate, and nothing anywhere hands it back out.
#[must_use]
pub fn handle_findings(modules: &[Module], chokepoint_module: &str, handle: &str) -> Vec<String> {
    let mut findings = Vec::new();
    let mut fields = Vec::new();
    for (name, source) in modules {
        let file = parse(source);
        if name != chokepoint_module && mentions(&tokens_without_literals(source), handle) > 0 {
            findings.push(format!("`{name}` names `{handle}`, and only the chokepoint's module may"));
        }
        for (holder, field, public) in fields_mentioning(&file, handle) {
            fields.push((name.clone(), holder, field, public));
        }
        for function in returns_mentioning(&file, handle) {
            findings.push(format!("`{name}::{function}` hands `{handle}` back out"));
        }
    }
    if fields.len() != 1 {
        findings.push(format!(
            "{} fields hold `{handle}`, and exactly one may",
            fields.len()
        ));
    }
    for (module, holder, field, public) in fields {
        if module != chokepoint_module {
            findings.push(format!(
                "`{module}::{holder}.{field}` holds `{handle}` outside the chokepoint's module"
            ));
        }
        if public {
            findings.push(format!("`{holder}.{field}` is visible beyond its module"));
        }
    }
    findings
}

/// The dependencies one crate's manifest declares, under one table.
///
/// # Panics
///
/// Panics when the manifest cannot be read.
#[must_use]
pub fn manifest_dependencies(crate_name: &str, table: &str) -> Vec<String> {
    let manifest = read(&crate_dir(crate_name).join("Cargo.toml"));
    let mut names = Vec::new();
    let mut inside = false;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            inside = trimmed == format!("[{table}]");
            continue;
        }
        if !inside || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((name, _)) = trimmed.split_once('=') {
            names.push(name.trim().to_owned());
        }
    }
    names.sort();
    names
}
