//! What JavaScript and Node provide without an import: globals, the members of the built-in
//! types, keywords and code snippets. The type names are the keys of [`members`].

use crate::languages::{CompletionItem, ItemKind};

/// A member of a built-in type: (name, signature, documentation, type of the value or result).
/// The signature starts with `(` for a method; a property has the type only.
pub type Member = (&'static str, &'static str, &'static str, &'static str);

pub const STRING: &[Member] = &[
    ("length", "number", "Nombre d'unités UTF-16 de la chaîne.", "number"),
    ("at", "(index: number): string", "Caractère à la position donnée ; un index négatif compte depuis la fin.", "string"),
    ("charAt", "(index: number): string", "Caractère à la position donnée.", "string"),
    ("charCodeAt", "(index: number): number", "Unité de code UTF-16 à la position donnée.", "number"),
    ("codePointAt", "(index: number): number", "Point de code Unicode à la position donnée.", "number"),
    ("concat", "(...strings: string[]): string", "Concatène des chaînes.", "string"),
    ("endsWith", "(search: string, length?: number): boolean", "Indique si la chaîne se termine par la valeur donnée.", "boolean"),
    ("includes", "(search: string, from?: number): boolean", "Indique si la chaîne contient la valeur donnée.", "boolean"),
    ("indexOf", "(search: string, from?: number): number", "Première position de la valeur, ou -1.", "number"),
    ("lastIndexOf", "(search: string, from?: number): number", "Dernière position de la valeur, ou -1.", "number"),
    ("localeCompare", "(other: string): number", "Compare deux chaînes selon la langue.", "number"),
    ("match", "(regexp: RegExp | string): RegExpMatchArray | null", "Correspondances d'une expression régulière.", "Array"),
    ("matchAll", "(regexp: RegExp): Iterator", "Itère sur toutes les correspondances.", "Array"),
    ("normalize", "(form?: string): string", "Forme normalisée Unicode.", "string"),
    ("padEnd", "(length: number, fill?: string): string", "Complète la fin jusqu'à la longueur donnée.", "string"),
    ("padStart", "(length: number, fill?: string): string", "Complète le début jusqu'à la longueur donnée.", "string"),
    ("repeat", "(count: number): string", "Répète la chaîne.", "string"),
    ("replace", "(search: string | RegExp, by: string | Function): string", "Remplace la première correspondance.", "string"),
    ("replaceAll", "(search: string | RegExp, by: string | Function): string", "Remplace toutes les correspondances.", "string"),
    ("search", "(regexp: RegExp): number", "Position de la première correspondance, ou -1.", "number"),
    ("slice", "(start?: number, end?: number): string", "Extrait une partie de la chaîne.", "string"),
    ("split", "(separator: string | RegExp, limit?: number): string[]", "Découpe la chaîne en tableau.", "Array"),
    ("startsWith", "(search: string, from?: number): boolean", "Indique si la chaîne commence par la valeur donnée.", "boolean"),
    ("substring", "(start: number, end?: number): string", "Extrait les caractères entre deux positions.", "string"),
    ("toLocaleLowerCase", "(locale?: string): string", "Minuscules selon la langue.", "string"),
    ("toLocaleUpperCase", "(locale?: string): string", "Majuscules selon la langue.", "string"),
    ("toLowerCase", "(): string", "Convertit en minuscules.", "string"),
    ("toString", "(): string", "La chaîne elle-même.", "string"),
    ("toUpperCase", "(): string", "Convertit en majuscules.", "string"),
    ("trim", "(): string", "Retire les espaces au début et à la fin.", "string"),
    ("trimEnd", "(): string", "Retire les espaces à la fin.", "string"),
    ("trimStart", "(): string", "Retire les espaces au début.", "string"),
    ("valueOf", "(): string", "Valeur primitive.", "string"),
];

pub const ARRAY: &[Member] = &[
    ("length", "number", "Nombre d'éléments du tableau.", "number"),
    ("at", "(index: number): T", "Élément à la position donnée ; un index négatif compte depuis la fin.", "any"),
    ("concat", "(...items: T[]): T[]", "Concatène des tableaux.", "Array"),
    ("copyWithin", "(target: number, start: number, end?: number): T[]", "Copie une partie du tableau sur elle-même.", "Array"),
    ("entries", "(): Iterator<[number, T]>", "Itérateur de paires [index, valeur].", "Array"),
    ("every", "(test: (value: T, index: number) => boolean): boolean", "Vrai si tous les éléments passent le test.", "boolean"),
    ("fill", "(value: T, start?: number, end?: number): T[]", "Remplit le tableau avec une valeur.", "Array"),
    ("filter", "(test: (value: T, index: number) => boolean): T[]", "Nouveau tableau des éléments qui passent le test.", "Array"),
    ("find", "(test: (value: T, index: number) => boolean): T | undefined", "Premier élément qui passe le test.", "any"),
    ("findIndex", "(test: (value: T, index: number) => boolean): number", "Position du premier élément qui passe le test, ou -1.", "number"),
    ("findLast", "(test: (value: T, index: number) => boolean): T | undefined", "Dernier élément qui passe le test.", "any"),
    ("findLastIndex", "(test: (value: T, index: number) => boolean): number", "Position du dernier élément qui passe le test, ou -1.", "number"),
    ("flat", "(depth?: number): any[]", "Aplatit les tableaux imbriqués.", "Array"),
    ("flatMap", "(map: (value: T, index: number) => U | U[]): U[]", "Applique une fonction puis aplatit d'un niveau.", "Array"),
    ("forEach", "(action: (value: T, index: number) => void): void", "Exécute une fonction pour chaque élément.", "void"),
    ("includes", "(value: T, from?: number): boolean", "Indique si le tableau contient la valeur.", "boolean"),
    ("indexOf", "(value: T, from?: number): number", "Première position de la valeur, ou -1.", "number"),
    ("join", "(separator?: string): string", "Assemble les éléments en une chaîne.", "string"),
    ("keys", "(): Iterator<number>", "Itérateur des index.", "Array"),
    ("lastIndexOf", "(value: T, from?: number): number", "Dernière position de la valeur, ou -1.", "number"),
    ("map", "(map: (value: T, index: number) => U): U[]", "Nouveau tableau des résultats de la fonction.", "Array"),
    ("pop", "(): T | undefined", "Retire et renvoie le dernier élément.", "any"),
    ("push", "(...items: T[]): number", "Ajoute des éléments à la fin ; renvoie la nouvelle longueur.", "number"),
    ("reduce", "(reducer: (acc: U, value: T) => U, initial?: U): U", "Réduit le tableau à une seule valeur.", "any"),
    ("reduceRight", "(reducer: (acc: U, value: T) => U, initial?: U): U", "Réduit le tableau de droite à gauche.", "any"),
    ("reverse", "(): T[]", "Inverse le tableau sur place.", "Array"),
    ("shift", "(): T | undefined", "Retire et renvoie le premier élément.", "any"),
    ("slice", "(start?: number, end?: number): T[]", "Copie une partie du tableau.", "Array"),
    ("some", "(test: (value: T, index: number) => boolean): boolean", "Vrai si au moins un élément passe le test.", "boolean"),
    ("sort", "(compare?: (a: T, b: T) => number): T[]", "Trie le tableau sur place.", "Array"),
    ("splice", "(start: number, deleteCount?: number, ...items: T[]): T[]", "Supprime et insère des éléments sur place.", "Array"),
    ("toReversed", "(): T[]", "Copie inversée du tableau.", "Array"),
    ("toSorted", "(compare?: (a: T, b: T) => number): T[]", "Copie triée du tableau.", "Array"),
    ("toSpliced", "(start: number, deleteCount?: number, ...items: T[]): T[]", "Copie modifiée comme par splice.", "Array"),
    ("toString", "(): string", "Représentation en chaîne.", "string"),
    ("unshift", "(...items: T[]): number", "Ajoute des éléments au début ; renvoie la nouvelle longueur.", "number"),
    ("values", "(): Iterator<T>", "Itérateur des valeurs.", "Array"),
    ("with", "(index: number, value: T): T[]", "Copie avec un élément remplacé.", "Array"),
];

pub const NUMBER: &[Member] = &[
    ("toExponential", "(digits?: number): string", "Notation exponentielle.", "string"),
    ("toFixed", "(digits?: number): string", "Notation à virgule fixe.", "string"),
    ("toLocaleString", "(locale?: string, options?: object): string", "Nombre formaté selon la langue.", "string"),
    ("toPrecision", "(precision?: number): string", "Nombre avec une précision donnée.", "string"),
    ("toString", "(radix?: number): string", "Représentation dans une base donnée.", "string"),
    ("valueOf", "(): number", "Valeur primitive.", "number"),
];

pub const BOOLEAN: &[Member] = &[
    ("toString", "(): string", "\"true\" ou \"false\".", "string"),
    ("valueOf", "(): boolean", "Valeur primitive.", "boolean"),
];

pub const FUNCTION: &[Member] = &[
    ("apply", "(thisArg: any, args?: any[]): any", "Appelle la fonction avec un `this` et un tableau d'arguments.", "any"),
    ("bind", "(thisArg: any, ...args: any[]): Function", "Fonction liée à un `this` et à des arguments.", "Function"),
    ("call", "(thisArg: any, ...args: any[]): any", "Appelle la fonction avec un `this` et des arguments.", "any"),
    ("length", "number", "Nombre de paramètres déclarés.", "number"),
    ("name", "string", "Nom de la fonction.", "string"),
    ("toString", "(): string", "Code source de la fonction.", "string"),
];

pub const OBJECT: &[Member] = &[
    ("constructor", "Function", "Constructeur de l'objet.", "Function"),
    ("hasOwnProperty", "(key: string): boolean", "Indique si la propriété appartient à l'objet lui-même.", "boolean"),
    ("isPrototypeOf", "(object: object): boolean", "Indique si l'objet est dans la chaîne de prototypes.", "boolean"),
    ("propertyIsEnumerable", "(key: string): boolean", "Indique si la propriété est énumérable.", "boolean"),
    ("toLocaleString", "(): string", "Représentation selon la langue.", "string"),
    ("toString", "(): string", "Représentation en chaîne.", "string"),
    ("valueOf", "(): any", "Valeur primitive.", "any"),
];

pub const OBJECT_STATIC: &[Member] = &[
    ("assign", "(target: object, ...sources: object[]): object", "Copie les propriétés des sources vers la cible.", "Object"),
    ("create", "(prototype: object | null, properties?: object): object", "Crée un objet avec le prototype donné.", "Object"),
    ("defineProperties", "(object: object, properties: object): object", "Définit plusieurs propriétés.", "Object"),
    ("defineProperty", "(object: object, key: string | symbol, descriptor: object): object", "Définit une propriété.", "Object"),
    ("entries", "(object: object): [string, any][]", "Paires [clé, valeur] des propriétés propres énumérables.", "Array"),
    ("freeze", "(object: object): object", "Gèle l'objet : plus aucune modification.", "Object"),
    ("fromEntries", "(entries: Iterable<[string, any]>): object", "Crée un objet depuis des paires [clé, valeur].", "Object"),
    ("getOwnPropertyDescriptor", "(object: object, key: string): object", "Descripteur d'une propriété propre.", "Object"),
    ("getOwnPropertyDescriptors", "(object: object): object", "Descripteurs de toutes les propriétés propres.", "Object"),
    ("getOwnPropertyNames", "(object: object): string[]", "Noms des propriétés propres.", "Array"),
    ("getOwnPropertySymbols", "(object: object): symbol[]", "Symboles des propriétés propres.", "Array"),
    ("getPrototypeOf", "(object: object): object | null", "Prototype de l'objet.", "Object"),
    ("groupBy", "(items: Iterable<T>, key: (item: T) => string): object", "Regroupe des éléments par clé.", "Object"),
    ("hasOwn", "(object: object, key: string): boolean", "Indique si la propriété est propre à l'objet.", "boolean"),
    ("is", "(a: any, b: any): boolean", "Égalité stricte, avec NaN égal à NaN.", "boolean"),
    ("isExtensible", "(object: object): boolean", "Indique si on peut ajouter des propriétés.", "boolean"),
    ("isFrozen", "(object: object): boolean", "Indique si l'objet est gelé.", "boolean"),
    ("isSealed", "(object: object): boolean", "Indique si l'objet est scellé.", "boolean"),
    ("keys", "(object: object): string[]", "Clés des propriétés propres énumérables.", "Array"),
    ("preventExtensions", "(object: object): object", "Empêche l'ajout de propriétés.", "Object"),
    ("seal", "(object: object): object", "Scelle l'objet : pas d'ajout ni de suppression.", "Object"),
    ("setPrototypeOf", "(object: object, prototype: object | null): object", "Change le prototype de l'objet.", "Object"),
    ("values", "(object: object): any[]", "Valeurs des propriétés propres énumérables.", "Array"),
];

pub const ARRAY_STATIC: &[Member] = &[
    ("from", "(items: Iterable<T> | ArrayLike<T>, map?: (value: T, index: number) => U): U[]", "Crée un tableau depuis un itérable.", "Array"),
    ("fromAsync", "(items: AsyncIterable<T>): Promise<T[]>", "Crée un tableau depuis un itérable asynchrone.", "Promise"),
    ("isArray", "(value: any): boolean", "Indique si la valeur est un tableau.", "boolean"),
    ("of", "(...items: T[]): T[]", "Crée un tableau avec les éléments donnés.", "Array"),
];

pub const STRING_STATIC: &[Member] = &[
    ("fromCharCode", "(...codes: number[]): string", "Chaîne depuis des unités de code UTF-16.", "string"),
    ("fromCodePoint", "(...codes: number[]): string", "Chaîne depuis des points de code Unicode.", "string"),
    ("raw", "(template: TemplateStringsArray, ...values: any[]): string", "Chaîne brute d'un gabarit.", "string"),
];

pub const NUMBER_STATIC: &[Member] = &[
    ("EPSILON", "number", "Plus petit écart entre deux nombres.", "number"),
    ("MAX_SAFE_INTEGER", "number", "Plus grand entier représenté exactement (2^53 - 1).", "number"),
    ("MAX_VALUE", "number", "Plus grand nombre représentable.", "number"),
    ("MIN_SAFE_INTEGER", "number", "Plus petit entier représenté exactement.", "number"),
    ("MIN_VALUE", "number", "Plus petit nombre positif représentable.", "number"),
    ("NaN", "number", "Not-a-Number.", "number"),
    ("NEGATIVE_INFINITY", "number", "Moins l'infini.", "number"),
    ("POSITIVE_INFINITY", "number", "Plus l'infini.", "number"),
    ("isFinite", "(value: any): boolean", "Indique si la valeur est un nombre fini.", "boolean"),
    ("isInteger", "(value: any): boolean", "Indique si la valeur est un entier.", "boolean"),
    ("isNaN", "(value: any): boolean", "Indique si la valeur est NaN.", "boolean"),
    ("isSafeInteger", "(value: any): boolean", "Indique si la valeur est un entier sûr.", "boolean"),
    ("parseFloat", "(text: string): number", "Convertit une chaîne en nombre décimal.", "number"),
    ("parseInt", "(text: string, radix?: number): number", "Convertit une chaîne en entier.", "number"),
];

pub const MATH: &[Member] = &[
    ("E", "number", "Nombre d'Euler.", "number"),
    ("LN10", "number", "Logarithme naturel de 10.", "number"),
    ("LN2", "number", "Logarithme naturel de 2.", "number"),
    ("LOG10E", "number", "Logarithme décimal de e.", "number"),
    ("LOG2E", "number", "Logarithme en base 2 de e.", "number"),
    ("PI", "number", "Pi.", "number"),
    ("SQRT1_2", "number", "Racine carrée de 1/2.", "number"),
    ("SQRT2", "number", "Racine carrée de 2.", "number"),
    ("abs", "(x: number): number", "Valeur absolue.", "number"),
    ("acos", "(x: number): number", "Arc cosinus.", "number"),
    ("asin", "(x: number): number", "Arc sinus.", "number"),
    ("atan", "(x: number): number", "Arc tangente.", "number"),
    ("atan2", "(y: number, x: number): number", "Angle du point (x, y).", "number"),
    ("cbrt", "(x: number): number", "Racine cubique.", "number"),
    ("ceil", "(x: number): number", "Entier supérieur ou égal.", "number"),
    ("clz32", "(x: number): number", "Nombre de zéros de tête sur 32 bits.", "number"),
    ("cos", "(x: number): number", "Cosinus.", "number"),
    ("exp", "(x: number): number", "e à la puissance x.", "number"),
    ("floor", "(x: number): number", "Entier inférieur ou égal.", "number"),
    ("fround", "(x: number): number", "Flottant 32 bits le plus proche.", "number"),
    ("hypot", "(...values: number[]): number", "Racine de la somme des carrés.", "number"),
    ("imul", "(a: number, b: number): number", "Multiplication d'entiers 32 bits.", "number"),
    ("log", "(x: number): number", "Logarithme naturel.", "number"),
    ("log10", "(x: number): number", "Logarithme décimal.", "number"),
    ("log1p", "(x: number): number", "Logarithme naturel de 1 + x.", "number"),
    ("log2", "(x: number): number", "Logarithme en base 2.", "number"),
    ("max", "(...values: number[]): number", "Plus grande valeur.", "number"),
    ("min", "(...values: number[]): number", "Plus petite valeur.", "number"),
    ("pow", "(base: number, exponent: number): number", "Puissance.", "number"),
    ("random", "(): number", "Nombre pseudo-aléatoire dans [0, 1[.", "number"),
    ("round", "(x: number): number", "Entier le plus proche.", "number"),
    ("sign", "(x: number): number", "Signe : -1, 0 ou 1.", "number"),
    ("sin", "(x: number): number", "Sinus.", "number"),
    ("sqrt", "(x: number): number", "Racine carrée.", "number"),
    ("tan", "(x: number): number", "Tangente.", "number"),
    ("trunc", "(x: number): number", "Partie entière.", "number"),
];

pub const JSON: &[Member] = &[
    ("parse", "(text: string, reviver?: Function): any", "Convertit du JSON en valeur JavaScript.", "any"),
    ("stringify", "(value: any, replacer?: Function | any[], space?: number | string): string", "Convertit une valeur en JSON.", "string"),
];

pub const CONSOLE: &[Member] = &[
    ("assert", "(condition: boolean, ...data: any[]): void", "Affiche un message si la condition est fausse.", "void"),
    ("clear", "(): void", "Efface la console.", "void"),
    ("count", "(label?: string): void", "Compte les appels avec ce libellé.", "void"),
    ("debug", "(...data: any[]): void", "Message de débogage.", "void"),
    ("dir", "(object: any, options?: object): void", "Affiche les propriétés d'un objet.", "void"),
    ("error", "(...data: any[]): void", "Message d'erreur.", "void"),
    ("group", "(...label: any[]): void", "Ouvre un groupe de messages.", "void"),
    ("groupEnd", "(): void", "Ferme le groupe courant.", "void"),
    ("info", "(...data: any[]): void", "Message d'information.", "void"),
    ("log", "(...data: any[]): void", "Affiche un message.", "void"),
    ("table", "(data: any, columns?: string[]): void", "Affiche des données sous forme de tableau.", "void"),
    ("time", "(label?: string): void", "Démarre un chronomètre.", "void"),
    ("timeEnd", "(label?: string): void", "Arrête le chronomètre et affiche la durée.", "void"),
    ("trace", "(...data: any[]): void", "Affiche la pile d'appels.", "void"),
    ("warn", "(...data: any[]): void", "Avertissement.", "void"),
];

pub const PROMISE: &[Member] = &[
    ("catch", "(onRejected: (reason: any) => any): Promise", "Traite le rejet de la promesse.", "Promise"),
    ("finally", "(onFinally: () => void): Promise", "Exécuté quand la promesse est réglée.", "Promise"),
    ("then", "(onFulfilled: (value: T) => any, onRejected?: (reason: any) => any): Promise", "Traite le résultat de la promesse.", "Promise"),
];

pub const PROMISE_STATIC: &[Member] = &[
    ("all", "(promises: Iterable<Promise>): Promise<any[]>", "Attend toutes les promesses.", "Promise"),
    ("allSettled", "(promises: Iterable<Promise>): Promise<object[]>", "Attend que toutes les promesses soient réglées.", "Promise"),
    ("any", "(promises: Iterable<Promise>): Promise", "Première promesse tenue.", "Promise"),
    ("race", "(promises: Iterable<Promise>): Promise", "Première promesse réglée.", "Promise"),
    ("reject", "(reason: any): Promise", "Promesse rejetée.", "Promise"),
    ("resolve", "(value?: any): Promise", "Promesse tenue.", "Promise"),
    ("withResolvers", "(): { promise, resolve, reject }", "Promesse avec ses fonctions resolve et reject.", "Object"),
];

pub const MAP: &[Member] = &[
    ("size", "number", "Nombre d'entrées.", "number"),
    ("clear", "(): void", "Supprime toutes les entrées.", "void"),
    ("delete", "(key: K): boolean", "Supprime une entrée.", "boolean"),
    ("entries", "(): Iterator<[K, V]>", "Itérateur des paires [clé, valeur].", "Array"),
    ("forEach", "(action: (value: V, key: K) => void): void", "Exécute une fonction pour chaque entrée.", "void"),
    ("get", "(key: K): V | undefined", "Valeur associée à la clé.", "any"),
    ("has", "(key: K): boolean", "Indique si la clé existe.", "boolean"),
    ("keys", "(): Iterator<K>", "Itérateur des clés.", "Array"),
    ("set", "(key: K, value: V): Map", "Associe une valeur à une clé.", "Map"),
    ("values", "(): Iterator<V>", "Itérateur des valeurs.", "Array"),
];

pub const SET: &[Member] = &[
    ("size", "number", "Nombre d'éléments.", "number"),
    ("add", "(value: T): Set", "Ajoute un élément.", "Set"),
    ("clear", "(): void", "Supprime tous les éléments.", "void"),
    ("delete", "(value: T): boolean", "Supprime un élément.", "boolean"),
    ("entries", "(): Iterator<[T, T]>", "Itérateur des paires [valeur, valeur].", "Array"),
    ("forEach", "(action: (value: T) => void): void", "Exécute une fonction pour chaque élément.", "void"),
    ("has", "(value: T): boolean", "Indique si l'élément existe.", "boolean"),
    ("keys", "(): Iterator<T>", "Itérateur des éléments.", "Array"),
    ("values", "(): Iterator<T>", "Itérateur des éléments.", "Array"),
];

pub const DATE: &[Member] = &[
    ("getDate", "(): number", "Jour du mois (1-31).", "number"),
    ("getDay", "(): number", "Jour de la semaine (0 = dimanche).", "number"),
    ("getFullYear", "(): number", "Année.", "number"),
    ("getHours", "(): number", "Heures (0-23).", "number"),
    ("getMilliseconds", "(): number", "Millisecondes.", "number"),
    ("getMinutes", "(): number", "Minutes.", "number"),
    ("getMonth", "(): number", "Mois (0 = janvier).", "number"),
    ("getSeconds", "(): number", "Secondes.", "number"),
    ("getTime", "(): number", "Millisecondes depuis le 1er janvier 1970 UTC.", "number"),
    ("getTimezoneOffset", "(): number", "Décalage avec UTC, en minutes.", "number"),
    ("setDate", "(day: number): number", "Définit le jour du mois.", "number"),
    ("setFullYear", "(year: number, month?: number, day?: number): number", "Définit l'année.", "number"),
    ("setHours", "(hours: number, min?: number, sec?: number, ms?: number): number", "Définit l'heure.", "number"),
    ("setMinutes", "(min: number, sec?: number, ms?: number): number", "Définit les minutes.", "number"),
    ("setMonth", "(month: number, day?: number): number", "Définit le mois.", "number"),
    ("setSeconds", "(sec: number, ms?: number): number", "Définit les secondes.", "number"),
    ("setTime", "(time: number): number", "Définit la date en millisecondes.", "number"),
    ("toDateString", "(): string", "Date en texte lisible.", "string"),
    ("toISOString", "(): string", "Date au format ISO 8601.", "string"),
    ("toJSON", "(): string", "Date pour la sérialisation JSON.", "string"),
    ("toLocaleDateString", "(locale?: string, options?: object): string", "Date selon la langue.", "string"),
    ("toLocaleString", "(locale?: string, options?: object): string", "Date et heure selon la langue.", "string"),
    ("toLocaleTimeString", "(locale?: string, options?: object): string", "Heure selon la langue.", "string"),
    ("toString", "(): string", "Date et heure en texte.", "string"),
    ("toUTCString", "(): string", "Date et heure UTC en texte.", "string"),
    ("valueOf", "(): number", "Millisecondes depuis 1970.", "number"),
];

pub const DATE_STATIC: &[Member] = &[
    ("UTC", "(year: number, month?: number, ...): number", "Millisecondes d'une date UTC.", "number"),
    ("now", "(): number", "Millisecondes écoulées depuis le 1er janvier 1970 UTC.", "number"),
    ("parse", "(text: string): number", "Millisecondes d'une date en texte.", "number"),
];

pub const REGEXP: &[Member] = &[
    ("exec", "(text: string): RegExpExecArray | null", "Cherche une correspondance.", "Array"),
    ("flags", "string", "Drapeaux de l'expression.", "string"),
    ("global", "boolean", "Drapeau g.", "boolean"),
    ("lastIndex", "number", "Position de la prochaine recherche.", "number"),
    ("source", "string", "Texte du motif.", "string"),
    ("test", "(text: string): boolean", "Indique si le texte correspond.", "boolean"),
    ("toString", "(): string", "Représentation /motif/drapeaux.", "string"),
];

pub const ERROR: &[Member] = &[
    ("cause", "unknown", "Cause de l'erreur.", "any"),
    ("message", "string", "Description de l'erreur.", "string"),
    ("name", "string", "Nom du type d'erreur.", "string"),
    ("stack", "string", "Pile d'appels au moment de l'erreur.", "string"),
];

pub const PROCESS: &[Member] = &[
    ("arch", "string", "Architecture du processeur.", "string"),
    ("argv", "string[]", "Arguments de la ligne de commande.", "Array"),
    ("cwd", "(): string", "Dossier de travail courant.", "string"),
    ("env", "{ [key: string]: string | undefined }", "Variables d'environnement.", "Object"),
    ("exit", "(code?: number): never", "Termine le processus.", "void"),
    ("hrtime", "(): [number, number]", "Temps haute résolution.", "Array"),
    ("memoryUsage", "(): object", "Utilisation de la mémoire.", "Object"),
    ("nextTick", "(callback: Function, ...args: any[]): void", "Planifie un callback avant la prochaine boucle d'événements.", "void"),
    ("on", "(event: string, listener: Function): process", "Écoute un événement du processus.", "any"),
    ("pid", "number", "Identifiant du processus.", "number"),
    ("platform", "string", "Système d'exploitation (win32, linux, darwin…).", "string"),
    ("stderr", "WriteStream", "Flux d'erreur standard.", "Object"),
    ("stdin", "ReadStream", "Flux d'entrée standard.", "Object"),
    ("stdout", "WriteStream", "Flux de sortie standard.", "Object"),
    ("uptime", "(): number", "Secondes depuis le démarrage du processus.", "number"),
    ("version", "string", "Version de Node.js.", "string"),
    ("versions", "object", "Versions de Node.js et de ses dépendances.", "Object"),
];

pub const SYMBOL_STATIC: &[Member] = &[
    ("asyncIterator", "symbol", "Symbole de l'itération asynchrone.", "symbol"),
    ("for", "(key: string): symbol", "Symbole du registre global.", "symbol"),
    ("iterator", "symbol", "Symbole de l'itération.", "symbol"),
    ("keyFor", "(symbol: symbol): string | undefined", "Clé d'un symbole du registre.", "string"),
    ("toPrimitive", "symbol", "Symbole de la conversion en primitive.", "symbol"),
];

pub const URL_INSTANCE: &[Member] = &[
    ("hash", "string", "Fragment (#…).", "string"),
    ("host", "string", "Hôte et port.", "string"),
    ("hostname", "string", "Nom d'hôte.", "string"),
    ("href", "string", "URL complète.", "string"),
    ("origin", "string", "Origine (schéma, hôte, port).", "string"),
    ("pathname", "string", "Chemin.", "string"),
    ("port", "string", "Port.", "string"),
    ("protocol", "string", "Schéma (https:).", "string"),
    ("search", "string", "Paramètres (?…).", "string"),
    ("searchParams", "URLSearchParams", "Paramètres de recherche.", "Object"),
    ("toString", "(): string", "URL complète.", "string"),
];

/// Members of the instances of a built-in type, by name.
pub fn members(ty: &str) -> &'static [Member] {
    match ty {
        "String" => STRING,
        "Array" => ARRAY,
        "Number" => NUMBER,
        "Boolean" => BOOLEAN,
        "Function" => FUNCTION,
        "Object" => OBJECT,
        "Promise" => PROMISE,
        "Map" | "WeakMap" => MAP,
        "Set" | "WeakSet" => SET,
        "Date" => DATE,
        "RegExp" => REGEXP,
        "Error" | "TypeError" | "RangeError" | "SyntaxError" | "ReferenceError" => ERROR,
        "URL" => URL_INSTANCE,
        _ => &[],
    }
}

/// Members of a global object or constructor (`Math.`, `Object.`, `console.`).
pub fn statics(name: &str) -> &'static [Member] {
    match name {
        "Math" => MATH,
        "JSON" => JSON,
        "console" => CONSOLE,
        "process" => PROCESS,
        "Object" => OBJECT_STATIC,
        "Array" => ARRAY_STATIC,
        "String" => STRING_STATIC,
        "Number" => NUMBER_STATIC,
        "Promise" => PROMISE_STATIC,
        "Date" => DATE_STATIC,
        "Symbol" => SYMBOL_STATIC,
        _ => &[],
    }
}

/// A global name: (name, kind, signature or type, documentation, built-in type of its value).
pub type Global = (&'static str, ItemKind, &'static str, &'static str, &'static str);

pub const GLOBALS: &[Global] = &[
    ("undefined", ItemKind::Constant, "undefined", "Valeur d'une variable non initialisée.", ""),
    ("NaN", ItemKind::Constant, "number", "Not-a-Number.", "Number"),
    ("Infinity", ItemKind::Constant, "number", "Plus l'infini.", "Number"),
    ("globalThis", ItemKind::Variable, "typeof globalThis", "Objet global.", ""),
    ("console", ItemKind::Variable, "Console", "Console de débogage.", "console"),
    ("Math", ItemKind::Variable, "Math", "Fonctions et constantes mathématiques.", "Math"),
    ("JSON", ItemKind::Variable, "JSON", "Lecture et écriture de JSON.", "JSON"),
    ("process", ItemKind::Variable, "NodeJS.Process", "Processus Node.js courant.", "process"),
    ("Object", ItemKind::Class, "ObjectConstructor", "Constructeur et utilitaires des objets.", "Object"),
    ("Array", ItemKind::Class, "ArrayConstructor", "Constructeur et utilitaires des tableaux.", "Array"),
    ("String", ItemKind::Class, "StringConstructor", "Constructeur et utilitaires des chaînes.", "String"),
    ("Number", ItemKind::Class, "NumberConstructor", "Constructeur et utilitaires des nombres.", "Number"),
    ("Boolean", ItemKind::Class, "BooleanConstructor", "Constructeur des booléens.", "Boolean"),
    ("Symbol", ItemKind::Class, "SymbolConstructor", "Valeurs uniques utilisables comme clés.", "Symbol"),
    ("BigInt", ItemKind::Class, "BigIntConstructor", "Entiers de taille arbitraire.", ""),
    ("Promise", ItemKind::Class, "PromiseConstructor", "Résultat futur d'une opération asynchrone.", "Promise"),
    ("Map", ItemKind::Class, "MapConstructor", "Collection de paires clé-valeur.", "Map"),
    ("Set", ItemKind::Class, "SetConstructor", "Collection de valeurs uniques.", "Set"),
    ("WeakMap", ItemKind::Class, "WeakMapConstructor", "Map à clés objets non retenues par le ramasse-miettes.", "WeakMap"),
    ("WeakSet", ItemKind::Class, "WeakSetConstructor", "Set d'objets non retenus par le ramasse-miettes.", "WeakSet"),
    ("Date", ItemKind::Class, "DateConstructor", "Dates et heures.", "Date"),
    ("RegExp", ItemKind::Class, "RegExpConstructor", "Expressions régulières.", "RegExp"),
    ("Error", ItemKind::Class, "ErrorConstructor", "Erreur générique.", "Error"),
    ("TypeError", ItemKind::Class, "TypeErrorConstructor", "Valeur d'un type inattendu.", "TypeError"),
    ("RangeError", ItemKind::Class, "RangeErrorConstructor", "Valeur hors de l'intervalle autorisé.", "RangeError"),
    ("SyntaxError", ItemKind::Class, "SyntaxErrorConstructor", "Code ou donnée mal formé.", "SyntaxError"),
    ("ReferenceError", ItemKind::Class, "ReferenceErrorConstructor", "Référence à une variable inexistante.", "ReferenceError"),
    ("Proxy", ItemKind::Class, "ProxyConstructor", "Intercepte les opérations sur un objet.", ""),
    ("Reflect", ItemKind::Variable, "Reflect", "Opérations sur les objets, version fonctionnelle.", ""),
    ("Intl", ItemKind::Variable, "Intl", "Internationalisation : dates, nombres, tri.", ""),
    ("ArrayBuffer", ItemKind::Class, "ArrayBufferConstructor", "Tampon de données binaires.", ""),
    ("Uint8Array", ItemKind::Class, "Uint8ArrayConstructor", "Tableau d'octets.", ""),
    ("Float64Array", ItemKind::Class, "Float64ArrayConstructor", "Tableau de flottants 64 bits.", ""),
    ("URL", ItemKind::Class, "typeof URL", "Analyse et construction d'URL.", "URL"),
    ("URLSearchParams", ItemKind::Class, "typeof URLSearchParams", "Paramètres de requête d'une URL.", ""),
    ("TextEncoder", ItemKind::Class, "typeof TextEncoder", "Encode du texte en UTF-8.", ""),
    ("TextDecoder", ItemKind::Class, "typeof TextDecoder", "Décode de l'UTF-8 en texte.", ""),
    ("AbortController", ItemKind::Class, "typeof AbortController", "Annule des opérations asynchrones.", ""),
    ("Buffer", ItemKind::Class, "typeof Buffer", "Données binaires (Node.js).", ""),
    ("parseInt", ItemKind::Function, "(text: string, radix?: number): number", "Convertit une chaîne en entier.", ""),
    ("parseFloat", ItemKind::Function, "(text: string): number", "Convertit une chaîne en nombre décimal.", ""),
    ("isNaN", ItemKind::Function, "(value: any): boolean", "Indique si la valeur est NaN après conversion.", ""),
    ("isFinite", ItemKind::Function, "(value: any): boolean", "Indique si la valeur est un nombre fini.", ""),
    ("encodeURIComponent", ItemKind::Function, "(text: string): string", "Encode un composant d'URL.", ""),
    ("decodeURIComponent", ItemKind::Function, "(text: string): string", "Décode un composant d'URL.", ""),
    ("encodeURI", ItemKind::Function, "(uri: string): string", "Encode une URL complète.", ""),
    ("decodeURI", ItemKind::Function, "(uri: string): string", "Décode une URL complète.", ""),
    ("setTimeout", ItemKind::Function, "(callback: Function, delay?: number, ...args: any[]): Timeout", "Exécute une fonction après un délai en millisecondes.", ""),
    ("clearTimeout", ItemKind::Function, "(id: Timeout): void", "Annule un setTimeout.", ""),
    ("setInterval", ItemKind::Function, "(callback: Function, delay?: number, ...args: any[]): Timeout", "Exécute une fonction à intervalles réguliers.", ""),
    ("clearInterval", ItemKind::Function, "(id: Timeout): void", "Annule un setInterval.", ""),
    ("setImmediate", ItemKind::Function, "(callback: Function, ...args: any[]): Immediate", "Exécute une fonction après les événements d'E/S (Node.js).", ""),
    ("queueMicrotask", ItemKind::Function, "(callback: () => void): void", "Planifie une micro-tâche.", ""),
    ("structuredClone", ItemKind::Function, "(value: T): T", "Copie profonde d'une valeur.", ""),
    ("fetch", ItemKind::Function, "(input: string | URL | Request, init?: object): Promise<Response>", "Requête HTTP.", "Promise"),
    ("atob", ItemKind::Function, "(data: string): string", "Décode du base64.", ""),
    ("btoa", ItemKind::Function, "(data: string): string", "Encode en base64.", ""),
    ("require", ItemKind::Function, "(id: string): any", "Charge un module CommonJS.", ""),
    ("module", ItemKind::Variable, "NodeModule", "Module CommonJS courant.", ""),
    ("exports", ItemKind::Variable, "any", "Exports du module CommonJS courant.", ""),
    ("__dirname", ItemKind::Constant, "string", "Dossier du fichier courant.", "String"),
    ("__filename", ItemKind::Constant, "string", "Chemin du fichier courant.", "String"),
    ("window", ItemKind::Variable, "Window & typeof globalThis", "Fenêtre du navigateur.", ""),
    ("document", ItemKind::Variable, "Document", "Document HTML du navigateur.", ""),
    ("navigator", ItemKind::Variable, "Navigator", "Informations sur le navigateur.", ""),
    ("localStorage", ItemKind::Variable, "Storage", "Stockage persistant du navigateur.", ""),
    ("sessionStorage", ItemKind::Variable, "Storage", "Stockage de session du navigateur.", ""),
    ("alert", ItemKind::Function, "(message?: any): void", "Affiche une boîte de dialogue.", ""),
    ("requestAnimationFrame", ItemKind::Function, "(callback: (time: number) => void): number", "Planifie une animation avant le prochain affichage.", ""),
];

pub const KEYWORDS: &[&str] = &[
    "async", "await", "break", "case", "catch", "class", "const", "continue", "debugger", "default", "delete",
    "do", "else", "export", "extends", "false", "finally", "for", "function", "if", "import", "in",
    "instanceof", "let", "new", "null", "of", "return", "static", "super", "switch", "this", "throw", "true",
    "try", "typeof", "var", "void", "while", "with", "yield",
];

/// Code templates: (trigger, label, text, caret offset in the text, description).
pub const SNIPPETS: &[(&str, &str, &str, usize, &str)] = &[
    ("log", "log", "console.log()", 12, "console.log(…)"),
    ("clog", "clog", "console.log('', )", 14, "console.log('…', …)"),
    ("err", "err", "console.error()", 14, "console.error(…)"),
    ("fori", "fori", "for (let i = 0; i < ; i++) {\n    \n}", 22, "Boucle for avec un compteur"),
    ("forof", "forof", "for (const item of ) {\n    \n}", 18, "Boucle for…of"),
    ("forin", "forin", "for (const key in ) {\n    \n}", 17, "Boucle for…in"),
    ("if", "if", "if () {\n    \n}", 4, "Condition if"),
    ("ife", "ife", "if () {\n    \n} else {\n    \n}", 4, "Condition if / else"),
    ("func", "func", "function name() {\n    \n}", 9, "Déclaration de fonction"),
    ("afunc", "afunc", "async function name() {\n    \n}", 15, "Fonction asynchrone"),
    ("arrow", "arrow", "const name = () => {\n    \n};", 6, "Fonction fléchée"),
    ("try", "try", "try {\n    \n} catch (error) {\n    \n}", 11, "Bloc try / catch"),
    ("class", "class", "class Name {\n    constructor() {\n        \n    }\n}", 6, "Déclaration de classe"),
    ("imp", "imp", "import  from '';", 7, "Import ES"),
    ("req", "req", "const  = require('');", 6, "require CommonJS"),
    ("exp", "exp", "export default ;", 15, "Export par défaut"),
    ("timeout", "timeout", "setTimeout(() => {\n    \n}, 1000);", 23, "setTimeout"),
    ("prom", "prom", "new Promise((resolve, reject) => {\n    \n})", 38, "Nouvelle promesse"),
];

/// The completion items for names that need no declaration: keywords, snippets and globals.
pub fn global_items() -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = Vec::new();
    for (name, kind, detail, doc, _) in GLOBALS {
        let rank = if matches!(*name, "window" | "document" | "navigator" | "localStorage" | "sessionStorage" | "alert" | "requestAnimationFrame") {
            75
        } else {
            60
        };
        let mut item = CompletionItem::new(*name, *kind).detail(*detail).doc(*doc).rank(rank);
        if *kind == ItemKind::Function {
            item = item.insert(format!("{name}()"), Some(name.len() + 1));
        }
        items.push(item);
    }
    for keyword in KEYWORDS {
        items.push(CompletionItem::new(*keyword, ItemKind::Keyword).rank(70));
    }
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

/// The built-in type of a global (the key for [`statics`]), if it has members.
pub fn global_type(name: &str) -> Option<&'static str> {
    GLOBALS
        .iter()
        .find(|g| g.0 == name)
        .map(|g| g.4)
        .filter(|t| !t.is_empty())
}

pub fn global_doc(name: &str) -> Option<(&'static str, &'static str)> {
    GLOBALS.iter().find(|g| g.0 == name).map(|g| (g.2, g.3))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_are_coherent() {
        for ty in ["String", "Array", "Number", "Promise", "Map", "Set", "Date", "RegExp", "Error", "URL"] {
            assert!(!members(ty).is_empty(), "{ty}");
        }
        for name in ["Math", "JSON", "console", "process", "Object", "Array", "String", "Number", "Promise", "Date"] {
            assert!(!statics(name).is_empty(), "{name}");
        }
        assert!(members("String").iter().any(|m| m.0 == "toUpperCase"));
        assert!(statics("console").iter().any(|m| m.0 == "log"));
        assert_eq!(global_type("Math"), Some("Math"));
        assert!(global_type("setTimeout").is_none());
        let items = global_items();
        assert!(items.iter().any(|i| i.label == "setTimeout" && i.insert == "setTimeout()"));
        assert!(items.iter().any(|i| i.label == "const" && i.kind == ItemKind::Keyword));
    }
}
