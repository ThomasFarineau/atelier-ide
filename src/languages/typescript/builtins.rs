//! What TypeScript adds to JavaScript without an import: keywords, the primitive and library
//! types, and code templates.

use crate::languages::{CompletionItem, ItemKind};

pub const KEYWORDS: &[&str] = &[
    "abstract", "accessor", "as", "asserts", "declare", "enum", "global", "implements", "infer", "interface", "is",
    "keyof", "module", "namespace", "override", "private", "protected", "public", "readonly", "satisfies", "type",
    "unique",
];

/// The primitive types: (name, documentation).
pub const PRIMITIVES: &[(&str, &str)] = &[
    ("string", "Texte."),
    ("number", "Nombre, entier ou décimal."),
    ("boolean", "true ou false."),
    ("any", "N'importe quelle valeur, sans aucune vérification."),
    ("unknown", "Valeur de type inconnu : à vérifier avant tout usage."),
    ("never", "Type des valeurs qui n'existent jamais (fonction qui ne revient pas)."),
    ("void", "Absence de valeur, résultat d'une fonction sans retour."),
    ("null", "Absence volontaire de valeur."),
    ("undefined", "Valeur non définie."),
    ("object", "Tout ce qui n'est pas une valeur primitive."),
    ("symbol", "Valeur unique utilisable comme clé."),
    ("bigint", "Entier de taille arbitraire."),
];

/// Types of the standard library and utility types: (name, documentation).
pub const LIBRARY_TYPES: &[(&str, &str)] = &[
    ("Array", "Tableau : Array<T> s'écrit aussi T[]."),
    ("ReadonlyArray", "Tableau en lecture seule."),
    ("Promise", "Résultat futur d'une opération asynchrone."),
    ("Record", "Record<K, V> : objet dont les clés sont de type K et les valeurs de type V."),
    ("Partial", "Partial<T> : T dont toutes les propriétés sont facultatives."),
    ("Required", "Required<T> : T dont toutes les propriétés sont obligatoires."),
    ("Readonly", "Readonly<T> : T dont toutes les propriétés sont en lecture seule."),
    ("Pick", "Pick<T, K> : T réduit aux propriétés K."),
    ("Omit", "Omit<T, K> : T sans les propriétés K."),
    ("Exclude", "Exclude<T, U> : les membres de l'union T qui ne sont pas dans U."),
    ("Extract", "Extract<T, U> : les membres de l'union T qui sont dans U."),
    ("NonNullable", "NonNullable<T> : T sans null ni undefined."),
    ("ReturnType", "ReturnType<F> : type de retour de la fonction F."),
    ("Parameters", "Parameters<F> : types des paramètres de la fonction F, en tuple."),
    ("InstanceType", "InstanceType<C> : type des instances de la classe C."),
    ("Awaited", "Awaited<T> : type obtenu en attendant une promesse."),
    ("Map", "Collection de paires clé-valeur."),
    ("Set", "Collection de valeurs uniques."),
    ("WeakMap", "Map dont les clés objets ne sont pas retenues."),
    ("WeakSet", "Set d'objets non retenus."),
    ("Date", "Dates et heures."),
    ("RegExp", "Expression régulière."),
    ("Error", "Erreur."),
    ("Function", "Toute fonction."),
    ("Object", "Tout objet."),
    ("Uppercase", "Uppercase<S> : la chaîne S en majuscules, au niveau des types."),
    ("Lowercase", "Lowercase<S> : la chaîne S en minuscules, au niveau des types."),
    ("Capitalize", "Capitalize<S> : la chaîne S avec une majuscule initiale."),
];

/// Code templates for TypeScript: (trigger, label, text, caret offset in the text, description).
pub const SNIPPETS: &[(&str, &str, &str, usize, &str)] = &[
    ("interface", "interface", "interface Name {\n    \n}", 10, "Déclaration d'interface"),
    ("type", "type", "type Name = ;", 12, "Alias de type"),
    ("enum", "enum", "enum Name {\n    \n}", 5, "Énumération"),
    ("tclass", "tclass", "class Name {\n    constructor(private readonly value: string) {}\n}", 6, "Classe avec propriété de constructeur"),
    ("tfunc", "tfunc", "function name(param: string): void {\n    \n}", 9, "Fonction typée"),
    ("tarrow", "tarrow", "const name = (param: string): void => {\n    \n};", 6, "Fonction fléchée typée"),
    ("atry", "atry", "try {\n    \n} catch (error: unknown) {\n    \n}", 11, "try / catch avec erreur unknown"),
    ("generic", "generic", "function name<T>(value: T): T {\n    return value;\n}", 9, "Fonction générique"),
];

/// Keywords and templates of TypeScript, for the places where a value or a statement goes.
pub fn items() -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = KEYWORDS
        .iter()
        .map(|k| CompletionItem::new(*k, ItemKind::Keyword).rank(70))
        .collect();
    for (_, label, text, caret, doc) in SNIPPETS {
        items.push(
            CompletionItem::new(*label, ItemKind::Snippet)
                .detail(*doc)
                .insert(*text, Some(*caret))
                .rank(80),
        );
    }
    items
}

/// The types that exist without a declaration, for the places where a type goes.
pub fn type_items() -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = PRIMITIVES
        .iter()
        .map(|(name, doc)| CompletionItem::new(*name, ItemKind::Keyword).doc(*doc).rank(40))
        .collect();
    for (name, doc) in LIBRARY_TYPES {
        items.push(CompletionItem::new(*name, ItemKind::Interface).doc(*doc).rank(45));
    }
    for keyword in ["typeof", "keyof", "readonly", "unique", "infer"] {
        items.push(CompletionItem::new(keyword, ItemKind::Keyword).rank(70));
    }
    items
}

/// The documentation of a primitive or library type.
pub fn type_doc(name: &str) -> Option<&'static str> {
    PRIMITIVES
        .iter()
        .chain(LIBRARY_TYPES.iter())
        .find(|(n, _)| *n == name)
        .map(|(_, doc)| *doc)
}

/// Whether the name is a type of the language or its library (no import needed).
pub fn is_library_type(name: &str) -> bool {
    type_doc(name).is_some()
}
